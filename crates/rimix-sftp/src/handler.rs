//! The SSH side (login, the `sftp` subsystem only) and the SFTP side (a
//! file system rooted at the destination folder, uploads through partial
//! files).

use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::fs::OpenOptions;
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use russh::server::{self, Auth, ChannelOpenHandle, Msg, Response, Session};
use russh::{Channel, ChannelId, Pty};
use russh_sftp::protocol::{
    Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode,
};
use russh_sftp::server::StatusReply;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;

use crate::{ReceivedFile, Shared, MAX_FAILURES};

/// Prefix of every partial file; such names are invisible and untouchable
/// for the client.
const HIDDEN_PREFIX: &str = ".rimix-";
const PART_PREFIX: &str = ".rimix-sftp-";
const PART_SUFFIX: &str = ".part";

/// Directory entries sent per READDIR reply.
const DIR_BATCH: usize = 100;
/// The most bytes one READ reply carries.
const MAX_READ: u32 = 256 * 1024;
/// Highest `-N` suffix tried for a taken name.
const MAX_SUFFIX: u32 = 999;

const PASSWORD_PROMPT: &[(Cow<'static, str>, bool)] = &[(Cow::Borrowed("Password:"), false)];

static PART_SEQ: AtomicU64 = AtomicU64::new(0);
/// Held while a free name is picked and taken, so two uploads finishing at
/// once never pick the same name.
static NAMING: Mutex<()> = Mutex::new(());

// ---------------------------------------------------------------- SSH side

/// Accepts connections until the task is aborted; aborting it drops every
/// connection with it.
pub(crate) async fn serve(listener: TcpListener, config: Arc<server::Config>, shared: Arc<Shared>) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((tcp, peer)) => {
                    connections.spawn(serve_connection(tcp, peer, config.clone(), shared.clone()));
                }
                Err(e) => tracing::warn!(error = %e, "SFTP accept failed"),
            },
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

/// Runs one SSH session. russh runs the session in a task of its own, so it
/// is fed through an in-memory pipe that this task pumps: when this task is
/// aborted the pipe closes and the session ends with it.
async fn serve_connection(
    mut tcp: TcpStream,
    peer: SocketAddr,
    config: Arc<server::Config>,
    shared: Arc<Shared>,
) {
    tracing::info!(%peer, "SFTP connection opened");
    let handler = Connection {
        shared,
        peer,
        failures: 0,
        channels: HashMap::new(),
    };
    let (mut ours, theirs) = tokio::io::duplex(64 * 1024);
    let session = async {
        match server::run_stream(config, theirs, handler).await {
            Ok(running) => running.await,
            Err(e) => Err(e),
        }
    };
    tokio::select! {
        _ = tokio::io::copy_bidirectional(&mut tcp, &mut ours) => {}
        result = session => {
            if let Err(e) = result {
                tracing::debug!(%peer, error = %e, "SFTP session error");
            }
        }
    }
    tracing::info!(%peer, "SFTP connection closed");
}

/// One connection's SSH handler.
struct Connection {
    shared: Arc<Shared>,
    peer: SocketAddr,
    failures: u32,
    /// Session channels opened but not yet given a subsystem.
    channels: HashMap<ChannelId, Channel<Msg>>,
}

impl Connection {
    fn check(&mut self, method: &str, user: &str, password: &[u8]) -> Result<Auth, russh::Error> {
        if user == self.shared.user && password == self.shared.password.as_bytes() {
            tracing::info!(peer = %self.peer, user, method, "SFTP login accepted");
            return Ok(Auth::Accept);
        }
        self.failures += 1;
        tracing::info!(
            peer = %self.peer,
            user,
            method,
            failures = self.failures,
            "SFTP login refused"
        );
        if self.failures >= MAX_FAILURES {
            // An error from the handler ends the session.
            return Err(russh::Error::NotAuthenticated);
        }
        Ok(Auth::reject())
    }
}

impl server::Handler for Connection {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        self.check("password", user, password.as_bytes())
    }

    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        user: &str,
        _submethods: &str,
        response: Option<Response<'a>>,
    ) -> Result<Auth, Self::Error> {
        match response {
            None => Ok(Auth::Partial {
                name: Cow::Borrowed(""),
                instructions: Cow::Borrowed(""),
                prompts: Cow::Borrowed(PASSWORD_PROMPT),
            }),
            Some(mut answers) => {
                let password = answers.next().unwrap_or_default();
                self.check("keyboard-interactive", user, &password)
            }
        }
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.channels.remove(&channel);
        session.close(channel)
    }

    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        match self.channels.remove(&channel) {
            Some(stream) if name == "sftp" => {
                session.channel_success(channel)?;
                tracing::info!(peer = %self.peer, "SFTP session started");
                let fs = Sftp::new(self.shared.clone(), self.peer);
                // Ends when the channel does, which the session's end closes.
                russh_sftp::server::run(stream.into_stream(), fs).await;
                Ok(())
            }
            Some(stream) => {
                self.channels.insert(channel, stream);
                tracing::debug!(peer = %self.peer, name, "subsystem refused");
                session.channel_failure(channel)
            }
            None => session.channel_failure(channel),
        }
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_failure(channel)
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        _data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_failure(channel)
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _col_width: u32,
        _row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_failure(channel)
    }
}

// --------------------------------------------------------------- SFTP side

/// An open SFTP handle.
enum Open {
    Upload(Upload),
    Read(std::fs::File),
    /// Entries not yet sent; `None` until the first READDIR.
    Dir(PathBuf, Option<VecDeque<File>>),
}

/// A file being received into `part`, to be placed as `dir/name`.
struct Upload {
    file: Option<std::fs::File>,
    part: PathBuf,
    dir: PathBuf,
    name: String,
    remote: String,
}

impl Drop for Upload {
    /// An upload never closed (or that failed to land) leaves nothing
    /// behind. After a successful rename the part file is gone already.
    fn drop(&mut self) {
        self.file = None;
        let _ = std::fs::remove_file(&self.part);
    }
}

/// One SFTP session: a file system rooted at the destination.
struct Sftp {
    shared: Arc<Shared>,
    peer: SocketAddr,
    handles: HashMap<String, Open>,
    next_handle: u64,
}

fn ok(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: "Ok".to_string(),
        language_tag: "en-US".to_string(),
    }
}

fn denied() -> StatusReply {
    StatusReply::new(StatusCode::PermissionDenied)
}

fn io_status(e: std::io::Error) -> StatusReply {
    let code = match e.kind() {
        ErrorKind::NotFound => StatusCode::NoSuchFile,
        ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
        _ => StatusCode::Failure,
    };
    code.with_message(e.to_string())
}

fn failure(message: &str) -> StatusReply {
    StatusCode::Failure.with_message(message)
}

/// The client path's components, with `.` and `..` resolved against `/`.
/// Climbing above `/`, a hidden `.rimix-*` name, or a component no file
/// system on every platform accepts, is refused.
fn components(path: &str) -> Result<Vec<String>, StatusReply> {
    let mut parts: Vec<String> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop().ok_or_else(denied)?;
            }
            _ if part.starts_with(HIDDEN_PREFIX) || part.contains(['\\', ':', '\0']) => {
                return Err(denied());
            }
            _ => parts.push(part.to_string()),
        }
    }
    Ok(parts)
}

fn virtual_path(parts: &[String]) -> String {
    format!("/{}", parts.join("/"))
}

impl Sftp {
    fn new(shared: Arc<Shared>, peer: SocketAddr) -> Self {
        Sftp {
            shared,
            peer,
            handles: HashMap::new(),
            next_handle: 0,
        }
    }

    /// The real path for `parts`, refused if a symlink on the way leads
    /// out of the root.
    fn real(&self, parts: &[String]) -> Result<PathBuf, StatusReply> {
        let mut path = self.shared.destination.clone();
        path.extend(parts);
        let mut probe: &Path = &path;
        loop {
            match probe.canonicalize() {
                Ok(real) if real.starts_with(&self.shared.root) => return Ok(path),
                Ok(_) => return Err(denied()),
                Err(e) if e.kind() == ErrorKind::NotFound => {
                    probe = probe.parent().ok_or_else(denied)?;
                }
                Err(e) => return Err(io_status(e)),
            }
        }
    }

    /// The real path for client `path`, which must not be the root itself.
    fn real_entry(&self, path: &str) -> Result<(Vec<String>, PathBuf), StatusReply> {
        let parts = components(path)?;
        if parts.is_empty() {
            return Err(denied());
        }
        let real = self.real(&parts)?;
        Ok((parts, real))
    }

    fn add(&mut self, open: Open) -> String {
        self.next_handle += 1;
        let handle = self.next_handle.to_string();
        self.handles.insert(handle.clone(), open);
        handle
    }

    fn start_upload(&self, parts: Vec<String>, pflags: OpenFlags) -> Result<Open, StatusReply> {
        let (name, parent) = parts.split_last().ok_or_else(denied)?;
        let dir = self.real(parent)?;
        std::fs::create_dir_all(&dir).map_err(io_status)?;
        if pflags.contains(OpenFlags::EXCLUDE) && dir.join(name).exists() {
            return Err(failure("file exists"));
        }
        let seq = PART_SEQ.fetch_add(1, Ordering::Relaxed);
        let part = dir.join(format!(
            "{PART_PREFIX}{name}.{}-{seq}{PART_SUFFIX}",
            std::process::id()
        ));
        let file = OpenOptions::new()
            .write(true)
            .read(true)
            .create_new(true)
            .open(&part)
            .map_err(io_status)?;
        tracing::debug!(peer = %self.peer, remote = %virtual_path(&parts), "SFTP upload started");
        Ok(Open::Upload(Upload {
            file: Some(file),
            part,
            dir,
            name: name.clone(),
            remote: virtual_path(&parts),
        }))
    }

    /// Flushes the upload, renames it into place and reports it.
    fn finish(&self, mut upload: Upload) -> Result<(), StatusReply> {
        let file = upload.file.take().ok_or_else(|| failure("closed"))?;
        file.sync_data().map_err(io_status)?;
        let bytes = file.metadata().map_err(io_status)?.len();
        // Closed before the rename, which Windows needs.
        drop(file);
        let (path, _) = place(&upload.part, &upload.dir, &upload.name).map_err(io_status)?;
        tracing::info!(
            peer = %self.peer,
            path = %path.display(),
            bytes,
            remote = %upload.remote,
            "SFTP file received"
        );
        (self.shared.on_file)(ReceivedFile {
            path,
            bytes,
            remote_name: upload.remote.clone(),
        });
        Ok(())
    }
}

impl Drop for Sftp {
    fn drop(&mut self) {
        let unfinished = self
            .handles
            .values()
            .filter(|h| matches!(h, Open::Upload(_)))
            .count();
        tracing::info!(peer = %self.peer, unfinished, "SFTP session ended");
    }
}

impl russh_sftp::server::Handler for Sftp {
    type Error = StatusReply;

    fn unimplemented(&self) -> Self::Error {
        StatusReply::new(StatusCode::OpUnsupported)
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        let parts = components(&filename)?;
        if parts.is_empty() {
            return Err(failure("is a directory"));
        }
        let writing = pflags.intersects(
            OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::APPEND,
        );
        let open = if writing {
            self.start_upload(parts, pflags)?
        } else {
            let real = self.real(&parts)?;
            if !real.is_file() {
                return Err(StatusReply::new(StatusCode::NoSuchFile));
            }
            Open::Read(std::fs::File::open(&real).map_err(io_status)?)
        };
        let handle = self.add(open);
        Ok(Handle { id, handle })
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        match self.handles.remove(&handle) {
            Some(Open::Upload(upload)) => self.finish(upload).map(|()| ok(id)),
            Some(_) => Ok(ok(id)),
            None => Err(failure("no such handle")),
        }
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        let Some(Open::Read(file)) = self.handles.get_mut(&handle) else {
            return Err(failure("not open for reading"));
        };
        file.seek(SeekFrom::Start(offset)).map_err(io_status)?;
        let mut data = Vec::new();
        file.take(u64::from(len.min(MAX_READ)))
            .read_to_end(&mut data)
            .map_err(io_status)?;
        if data.is_empty() {
            return Err(StatusReply::new(StatusCode::Eof));
        }
        Ok(Data { id, data })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, Self::Error> {
        let Some(Open::Upload(Upload {
            file: Some(file), ..
        })) = self.handles.get_mut(&handle)
        else {
            return Err(StatusReply::new(StatusCode::PermissionDenied));
        };
        file.seek(SeekFrom::Start(offset)).map_err(io_status)?;
        file.write_all(&data).map_err(io_status)?;
        Ok(ok(id))
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let real = self.real(&components(&path)?)?;
        let meta = std::fs::symlink_metadata(real).map_err(io_status)?;
        Ok(Attrs {
            id,
            attrs: FileAttributes::from(&meta),
        })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let real = self.real(&components(&path)?)?;
        let meta = std::fs::metadata(real).map_err(io_status)?;
        Ok(Attrs {
            id,
            attrs: FileAttributes::from(&meta),
        })
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, Self::Error> {
        let meta = match self.handles.get(&handle) {
            Some(Open::Read(file))
            | Some(Open::Upload(Upload {
                file: Some(file), ..
            })) => file.metadata(),
            Some(Open::Dir(path, _)) => std::fs::metadata(path),
            _ => return Err(failure("no such handle")),
        }
        .map_err(io_status)?;
        Ok(Attrs {
            id,
            attrs: FileAttributes::from(&meta),
        })
    }

    /// Attribute changes (times, modes) are accepted and ignored.
    async fn setstat(
        &mut self,
        id: u32,
        path: String,
        _attrs: FileAttributes,
    ) -> Result<Status, Self::Error> {
        self.real(&components(&path)?)?;
        Ok(ok(id))
    }

    async fn fsetstat(
        &mut self,
        id: u32,
        handle: String,
        _attrs: FileAttributes,
    ) -> Result<Status, Self::Error> {
        if self.handles.contains_key(&handle) {
            Ok(ok(id))
        } else {
            Err(failure("no such handle"))
        }
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        let real = self.real(&components(&path)?)?;
        if !real.is_dir() {
            return Err(StatusReply::new(StatusCode::NoSuchFile));
        }
        let handle = self.add(Open::Dir(real, None));
        Ok(Handle { id, handle })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        let Some(Open::Dir(path, entries)) = self.handles.get_mut(&handle) else {
            return Err(failure("no such handle"));
        };
        if entries.is_none() {
            let mut list = VecDeque::new();
            for entry in std::fs::read_dir(&*path).map_err(io_status)? {
                let entry = entry.map_err(io_status)?;
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                if name.starts_with(HIDDEN_PREFIX) {
                    continue;
                }
                // Follows symlinks, as `ls -l` in an SFTP client shows targets.
                let Ok(meta) = std::fs::metadata(entry.path()) else {
                    continue;
                };
                list.push_back(File::new(name, FileAttributes::from(&meta)));
            }
            *entries = Some(list);
        }
        let pending = entries.as_mut().expect("listed above");
        if pending.is_empty() {
            return Err(StatusReply::new(StatusCode::Eof));
        }
        let take = pending.len().min(DIR_BATCH);
        let files = pending.drain(..take).collect();
        Ok(Name { id, files })
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
        let (_, real) = self.real_entry(&filename)?;
        if std::fs::symlink_metadata(&real)
            .map_err(io_status)?
            .is_dir()
        {
            return Err(failure("is a directory"));
        }
        std::fs::remove_file(&real).map_err(io_status)?;
        tracing::info!(peer = %self.peer, path = %real.display(), "SFTP client removed a file");
        Ok(ok(id))
    }

    async fn mkdir(
        &mut self,
        id: u32,
        path: String,
        _attrs: FileAttributes,
    ) -> Result<Status, Self::Error> {
        let (_, real) = self.real_entry(&path)?;
        std::fs::create_dir(real).map_err(io_status)?;
        Ok(ok(id))
    }

    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, Self::Error> {
        let (_, real) = self.real_entry(&path)?;
        std::fs::remove_dir(real).map_err(io_status)?;
        Ok(ok(id))
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        let parts = components(&path)?;
        Ok(Name {
            id,
            files: vec![File::dummy(virtual_path(&parts))],
        })
    }

    async fn rename(
        &mut self,
        id: u32,
        oldpath: String,
        newpath: String,
    ) -> Result<Status, Self::Error> {
        let (_, from) = self.real_entry(&oldpath)?;
        let (_, to) = self.real_entry(&newpath)?;
        let _naming = NAMING.lock().unwrap_or_else(|e| e.into_inner());
        if std::fs::symlink_metadata(&to).is_ok() {
            return Err(failure("target exists"));
        }
        std::fs::rename(&from, &to).map_err(io_status)?;
        Ok(ok(id))
    }
}

// ------------------------------------------------------- placing the file

// `place` and `free_name` are copied from
// `crates/sony-ptpip/src/sony/capture.rs` (same names there), which this
// crate does not depend on.

/// Renames the complete file `part` to a free name for `name` in `dir`
/// (see [`free_name`]), holding the naming lock; returns the path and name.
fn place(part: &Path, dir: &Path, name: &str) -> std::io::Result<(PathBuf, String)> {
    let _naming = NAMING.lock().unwrap_or_else(|e| e.into_inner());
    let (path, filename) = free_name(dir, name)?;
    std::fs::rename(part, &path)?;
    Ok((path, filename))
}

/// `dir/name`, or `dir/<stem>-N.<ext>` with the first free N up to 999.
fn free_name(dir: &Path, name: &str) -> std::io::Result<(PathBuf, String)> {
    let path = dir.join(name);
    if !path.exists() {
        return Ok((path, name.to_string()));
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
        _ => (name, None),
    };
    for n in 1..=MAX_SUFFIX {
        let candidate = match ext {
            Some(ext) => format!("{stem}-{n}.{ext}"),
            None => format!("{stem}-{n}"),
        };
        let path = dir.join(&candidate);
        if !path.exists() {
            return Ok((path, candidate));
        }
    }
    Err(std::io::Error::new(
        ErrorKind::AlreadyExists,
        format!("every name from {name} to suffix -{MAX_SUFFIX} is taken"),
    ))
}

/// Removes `.rimix-sftp-*.part` files a crashed session left in `root` or
/// one folder below it.
pub(crate) fn clean_stale_parts(root: &Path) {
    fn clean(dir: &Path, descend: bool) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if kind.is_file() && name.starts_with(PART_PREFIX) && name.ends_with(PART_SUFFIX) {
                match std::fs::remove_file(entry.path()) {
                    Ok(()) => {
                        tracing::info!(path = %entry.path().display(), "removed a partial SFTP upload")
                    }
                    Err(e) => {
                        tracing::warn!(path = %entry.path().display(), error = %e, "cannot remove a partial SFTP upload")
                    }
                }
            } else if kind.is_dir() && descend {
                clean(&entry.path(), false);
            }
        }
    }
    clean(root, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_resolve_dots_and_refuse_escapes() {
        assert_eq!(components("/a/./b/../c").unwrap(), vec!["a", "c"]);
        assert!(components("").unwrap().is_empty());
        assert!(components("/..").is_err());
        assert!(components("a/../../b").is_err());
        assert!(components("/.rimix-sftp-x.part").is_err());
        assert!(components("C:\\x").is_err());
    }

    #[test]
    fn free_name_suffixes_before_the_extension() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("DSC00001.ARW"), b"").unwrap();
        assert_eq!(
            free_name(tmp.path(), "DSC00001.ARW").unwrap().1,
            "DSC00001-1.ARW"
        );
        std::fs::write(tmp.path().join("NOEXT"), b"").unwrap();
        assert_eq!(free_name(tmp.path(), "NOEXT").unwrap().1, "NOEXT-1");
    }
}
