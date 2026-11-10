//! Client-side tests: a russh client with an SFTP session talks to a
//! [`Receiver`] the way a camera's FTP Transfer function would.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rimix_sftp::{load_or_create_host_key, ReceivedFile, Receiver, ReceiverConfig};
use russh::client;
use russh::keys::PublicKeyOrCertificate;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{OpenFlags, StatusCode};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

const USER: &str = "camera";
const PASSWORD: &str = "s3cret-pass";

struct AnyHost;

impl client::Handler for AnyHost {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

type Received = Arc<Mutex<Vec<ReceivedFile>>>;

async fn start_in(tmp: &Path, destination: PathBuf) -> (Receiver, Received) {
    let host_key = load_or_create_host_key(&tmp.join("host_key")).unwrap();
    let received: Received = Arc::default();
    let sink = received.clone();
    let receiver = Receiver::start(
        ReceiverConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            user: USER.into(),
            password: PASSWORD.into(),
            host_key,
            destination,
        },
        move |file| sink.lock().unwrap().push(file),
    )
    .await
    .unwrap();
    (receiver, received)
}

async fn start(tmp: &Path) -> (Receiver, Received, PathBuf) {
    let dest = tmp.join("dest");
    let (receiver, received) = start_in(tmp, dest.clone()).await;
    (receiver, received, dest)
}

async fn ssh(addr: SocketAddr) -> client::Handle<AnyHost> {
    client::connect(Arc::new(client::Config::default()), addr, AnyHost)
        .await
        .expect("ssh connect")
}

async fn sftp(addr: SocketAddr) -> (client::Handle<AnyHost>, SftpSession) {
    let mut handle = ssh(addr).await;
    let auth = handle.authenticate_password(USER, PASSWORD).await.unwrap();
    assert!(auth.success(), "{auth:?}");
    let channel = handle.channel_open_session().await.unwrap();
    channel.request_subsystem(true, "sftp").await.unwrap();
    let session = SftpSession::new(channel.into_stream()).await.unwrap();
    (handle, session)
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn status(err: &SftpError) -> Option<StatusCode> {
    match err {
        SftpError::Status(s) => Some(s.status_code),
        _ => None,
    }
}

fn expect_err<T>(result: Result<T, SftpError>) -> SftpError {
    match result {
        Ok(_) => panic!("expected an error"),
        Err(e) => e,
    }
}

async fn wait_closed(handle: &client::Handle<AnyHost>, what: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !handle.is_closed() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{what}"));
}

#[tokio::test]
async fn upload_lands_once_with_every_byte() {
    let tmp = tempfile::tempdir().unwrap();
    let (receiver, received, dest) = start(tmp.path()).await;
    let (_ssh, sftp) = sftp(receiver.local_addr()).await;

    let data: Vec<u8> = (0..5 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
    let mut file = sftp.create("/DSC00001.JPG").await.unwrap();
    file.write_all(&data).await.unwrap();
    file.shutdown().await.unwrap();

    let got = received.lock().unwrap().clone();
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].path, dest.join("DSC00001.JPG"));
    assert_eq!(got[0].bytes, data.len() as u64);
    assert_eq!(got[0].remote_name, "/DSC00001.JPG");
    assert_eq!(std::fs::read(&got[0].path).unwrap(), data);
    assert_eq!(names(&dest), vec!["DSC00001.JPG".to_string()]);

    // The camera may read back what it sent, stat it and list the folder.
    let mut back = Vec::new();
    let mut file = sftp.open("/DSC00001.JPG").await.unwrap();
    file.read_to_end(&mut back).await.unwrap();
    assert_eq!(back, data);
    let meta = sftp.metadata("/DSC00001.JPG").await.unwrap();
    assert_eq!(meta.len(), data.len() as u64);
    let listed: Vec<String> = sftp
        .read_dir("/")
        .await
        .unwrap()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(listed, vec!["DSC00001.JPG".to_string()]);
    receiver.stop().await;
}

#[tokio::test]
async fn writes_may_arrive_at_any_offset() {
    let tmp = tempfile::tempdir().unwrap();
    let (receiver, received, dest) = start(tmp.path()).await;
    let (_ssh, sftp) = sftp(receiver.local_addr()).await;

    let mut file = sftp
        .open_with_flags("ODD.TXT", OpenFlags::CREATE | OpenFlags::WRITE)
        .await
        .unwrap();
    file.seek(std::io::SeekFrom::Start(5)).await.unwrap();
    file.write_all(b" world").await.unwrap();
    file.flush().await.unwrap();
    file.seek(std::io::SeekFrom::Start(0)).await.unwrap();
    file.write_all(b"hello").await.unwrap();
    file.shutdown().await.unwrap();

    assert_eq!(std::fs::read(dest.join("ODD.TXT")).unwrap(), b"hello world");
    assert_eq!(received.lock().unwrap()[0].bytes, 11);
    receiver.stop().await;
}

/// Review focus: two uploads of one name at once keep both files, and no
/// half-written file ever shows under a final name.
#[tokio::test]
async fn concurrent_uploads_of_one_name_keep_both_and_hide_half_files() {
    let tmp = tempfile::tempdir().unwrap();
    let (receiver, received, dest) = start(tmp.path()).await;
    let (_a_ssh, a) = sftp(receiver.local_addr()).await;
    let (_b_ssh, b) = sftp(receiver.local_addr()).await;

    let mut fa = a.create("/DSC00002.JPG").await.unwrap();
    let mut fb = b.create("/DSC00002.JPG").await.unwrap();
    let chunk_a = vec![0xAAu8; 64 * 1024];
    let chunk_b = vec![0xBBu8; 64 * 1024];
    for _ in 0..16 {
        fa.write_all(&chunk_a).await.unwrap();
        fa.flush().await.unwrap();
        for name in names(&dest) {
            assert!(
                name.starts_with(".rimix-sftp-"),
                "visible mid-upload: {name}"
            );
        }
        fb.write_all(&chunk_b).await.unwrap();
        fb.flush().await.unwrap();
        for name in names(&dest) {
            assert!(
                name.starts_with(".rimix-sftp-"),
                "visible mid-upload: {name}"
            );
        }
        // The client never sees the partial files.
        assert_eq!(a.read_dir("/").await.unwrap().count(), 0);
    }
    let (ra, rb) = tokio::join!(fa.shutdown(), fb.shutdown());
    ra.unwrap();
    rb.unwrap();

    assert_eq!(
        names(&dest),
        vec!["DSC00002-1.JPG".to_string(), "DSC00002.JPG".to_string()]
    );
    let mut contents: Vec<u8> = ["DSC00002.JPG", "DSC00002-1.JPG"]
        .iter()
        .map(|n| {
            let bytes = std::fs::read(dest.join(n)).unwrap();
            assert_eq!(bytes.len(), 16 * 64 * 1024);
            assert!(bytes.iter().all(|b| *b == bytes[0]), "{n} mixes uploads");
            bytes[0]
        })
        .collect();
    contents.sort();
    assert_eq!(contents, vec![0xAA, 0xBB]);
    assert_eq!(received.lock().unwrap().len(), 2);
    receiver.stop().await;
}

#[tokio::test]
async fn paths_stay_inside_the_destination() {
    let tmp = tempfile::tempdir().unwrap();
    let (receiver, received, dest) = start(tmp.path()).await;
    let (_ssh, sftp) = sftp(receiver.local_addr()).await;

    assert_eq!(sftp.canonicalize(".").await.unwrap(), "/");
    assert_eq!(sftp.canonicalize("/a/./b/..").await.unwrap(), "/a");
    for path in ["/..", "../escape.txt", "/sub/../../escape.txt"] {
        let err = sftp.canonicalize(path).await.unwrap_err();
        assert_eq!(
            status(&err),
            Some(StatusCode::PermissionDenied),
            "{path}: {err:?}"
        );
    }
    let err = expect_err(sftp.create("../escape.txt").await);
    assert_eq!(status(&err), Some(StatusCode::PermissionDenied), "{err:?}");
    assert!(!tmp.path().join("escape.txt").exists());
    std::fs::write(tmp.path().join("outside.txt"), b"keep").unwrap();
    let err = sftp.remove_file("/../outside.txt").await.unwrap_err();
    assert_eq!(status(&err), Some(StatusCode::PermissionDenied), "{err:?}");
    let err = expect_err(sftp.open("/../outside.txt").await);
    assert_eq!(status(&err), Some(StatusCode::PermissionDenied), "{err:?}");
    assert!(sftp.read_dir("/..").await.is_err());

    // Folders: made on request, and on the fly for an upload into a path.
    sftp.create_dir("/2026-11-11").await.unwrap();
    assert!(dest.join("2026-11-11").is_dir());
    let mut file = sftp.create("/2026-11-12/DSC00003.JPG").await.unwrap();
    file.write_all(b"jpeg").await.unwrap();
    file.shutdown().await.unwrap();
    assert_eq!(
        received.lock().unwrap()[0].path,
        dest.join("2026-11-12").join("DSC00003.JPG")
    );

    // Rename and remove work inside the root, never on partial files.
    sftp.rename("/2026-11-12/DSC00003.JPG", "/2026-11-11/DSC00003.JPG")
        .await
        .unwrap();
    assert!(dest.join("2026-11-11/DSC00003.JPG").is_file());
    let err = sftp
        .rename("/2026-11-11/DSC00003.JPG", "/../stolen.JPG")
        .await
        .unwrap_err();
    assert_eq!(status(&err), Some(StatusCode::PermissionDenied), "{err:?}");
    std::fs::write(dest.join(".rimix-sftp-x.part"), b"half").unwrap();
    let err = sftp.remove_file("/.rimix-sftp-x.part").await.unwrap_err();
    assert_eq!(status(&err), Some(StatusCode::PermissionDenied), "{err:?}");
    sftp.remove_file("/2026-11-11/DSC00003.JPG").await.unwrap();
    assert!(!dest.join("2026-11-11/DSC00003.JPG").exists());
    assert_eq!(
        std::fs::read(tmp.path().join("outside.txt")).unwrap(),
        b"keep"
    );
    receiver.stop().await;
}

#[tokio::test]
async fn wrong_password_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let (receiver, _received, _dest) = start(tmp.path()).await;

    let mut other = ssh(receiver.local_addr()).await;
    let auth = other.authenticate_password("root", PASSWORD).await.unwrap();
    assert!(!auth.success(), "{auth:?}");

    let mut bad = ssh(receiver.local_addr()).await;
    for _ in 0..2 {
        let auth = bad.authenticate_password(USER, "wrong").await.unwrap();
        assert!(
            matches!(auth, client::AuthResult::Failure { .. }),
            "{auth:?}"
        );
    }
    let third = bad.authenticate_password(USER, "wrong").await;
    assert!(!matches!(third, Ok(ref a) if a.success()), "{third:?}");
    wait_closed(&bad, "closed after three bad passwords").await;
    receiver.stop().await;
}

#[tokio::test]
async fn only_the_sftp_subsystem_is_served() {
    let tmp = tempfile::tempdir().unwrap();
    let (receiver, _received, _dest) = start(tmp.path()).await;
    let mut handle = ssh(receiver.local_addr()).await;
    assert!(handle
        .authenticate_password(USER, PASSWORD)
        .await
        .unwrap()
        .success());
    let mut channel = handle.channel_open_session().await.unwrap();
    channel.exec(true, "ls").await.unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(5), channel.wait())
        .await
        .expect("exec reply");
    assert!(
        matches!(reply, Some(russh::ChannelMsg::Failure)),
        "{reply:?}"
    );
    let mut channel = handle.channel_open_session().await.unwrap();
    channel.request_shell(true).await.unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(5), channel.wait())
        .await
        .expect("shell reply");
    assert!(
        matches!(reply, Some(russh::ChannelMsg::Failure)),
        "{reply:?}"
    );
    receiver.stop().await;
}

#[tokio::test]
async fn stop_closes_live_sessions() {
    let tmp = tempfile::tempdir().unwrap();
    let (receiver, _received, dest) = start(tmp.path()).await;
    let addr = receiver.local_addr();
    let (handle, sftp) = sftp(addr).await;
    let mut file = sftp.create("/HALF.JPG").await.unwrap();
    file.write_all(&[1u8; 4096]).await.unwrap();
    file.flush().await.unwrap();

    receiver.stop().await;
    wait_closed(&handle, "session closed on stop").await;
    assert!(tokio::net::TcpStream::connect(addr).await.is_err());
    // The interrupted upload leaves nothing behind, not even its part file.
    tokio::time::timeout(Duration::from_secs(5), async {
        while !names(&dest).is_empty() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("left behind: {:?}", names(&dest)));
}

#[tokio::test]
async fn stale_part_files_are_cleaned_at_start() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("dest");
    std::fs::create_dir_all(dest.join("2026-11-10")).unwrap();
    std::fs::write(dest.join(".rimix-sftp-A.JPG.1-0.part"), b"half").unwrap();
    std::fs::write(dest.join("2026-11-10/.rimix-sftp-B.JPG.1-1.part"), b"half").unwrap();
    std::fs::write(dest.join("2026-11-10/KEEP.JPG"), b"whole").unwrap();
    let (receiver, _received) = start_in(tmp.path(), dest.clone()).await;
    assert_eq!(names(&dest), vec!["2026-11-10".to_string()]);
    assert_eq!(
        names(&dest.join("2026-11-10")),
        vec!["KEEP.JPG".to_string()]
    );
    receiver.stop().await;
}

#[tokio::test]
async fn host_key_is_stable_across_loads() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("keys").join("rimix_sftp_ed25519");
    let first = load_or_create_host_key(&path).unwrap();
    let second = load_or_create_host_key(&path).unwrap();
    assert_eq!(first.public_key(), second.public_key());
    assert_eq!(first.algorithm(), russh::keys::Algorithm::Ed25519);
    let pem = std::fs::read_to_string(&path).unwrap();
    assert!(
        pem.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"),
        "{pem}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "{mode:o}");
    }

    // The receiver shows the fingerprint the way the simulator does.
    let (receiver, _received, _dest) = start(tmp.path()).await;
    let reloaded = load_or_create_host_key(&tmp.path().join("host_key")).unwrap();
    let expected = reloaded
        .public_key()
        .fingerprint(russh::keys::HashAlg::Sha256)
        .to_string();
    let shown = receiver.fingerprint_sha256();
    assert_eq!(shown, expected);
    assert!(
        shown.starts_with("SHA256:") && !shown.ends_with('='),
        "{shown}"
    );
    receiver.stop().await;
}
