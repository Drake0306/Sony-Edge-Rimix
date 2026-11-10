# Rimix Phase 3 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Wi-Fi join dependable and diagnosable (the owner's first real attempt ended in "Could not join"), rejoin the camera's network by itself after a drop, support 2022-and-later Sony bodies that require Access Authentication (an SSH tunnel), grow the per-model quirk table, add the interval timer, and produce a Windows build and installer from CI; plus the leftovers parked at the end of Phase 2.

**Architecture:** Wi-Fi joining moves into its own small crate (`crates/rimix-wifi`) so the CLI and the app share one implementation with macOS (`networksetup`) and Windows (`netsh wlan`) back ends behind the existing `Runner` trait. The app gets a rolling log file through `tracing-appender`. SSH lives in `crates/sony-ptpip` as a transport option: a `russh` client opens a local listener and forwards each accepted connection through a `direct-tcpip` channel to the camera's port 15740, so the PTP/IP code is untouched; the simulator grows a matching `russh` server so the whole path is tested without hardware. The camera facade gains a `before_reconnect` hook the app uses to rejoin Wi-Fi. The interval timer is app-side (TypeScript) and drives the existing shoot action. Windows is built and tested only in GitHub Actions (no Windows machine here).

**Tech Stack:** As before plus `russh` 0.65 (Apache-2.0, client in `sony-ptpip`, server in the simulator), `tracing-appender` and `tracing-subscriber` (MIT) in the app, `tempfile` (already present) for the Windows profile file.

**Spec:** `docs/superpowers/specs/2026-11-10-rimix-design.md`, section 5 (transport step 2: SSH with username, password and host-key fingerprint; keepalive and reconnect; per-model quirks), 6.2 (interval timer in the Scopes and timer panel), 7 (Phase 3 list). Owner's input on 2026-11-11: the first real join attempt failed with "Could not join"; the owner asked to complete Phase 3.

## Global Constraints

- Licence Apache-2.0; new dependencies must be Apache-2.0, MIT or BSD: `russh` 0.65 (Apache-2.0), `tracing-appender`, `tracing-subscriber` (MIT).
- Commit messages: plain language, under 60 characters, no "Co-Authored-By" or any Claude attribution.
- Platforms: macOS 14+ first; Windows 11 second. Windows code is compiled and tested in CI (`windows-latest`); nothing may break the macOS build. OS commands live under `#[cfg(target_os = "...")]`; other targets return `Unsupported`.
- Never store or log a Wi-Fi password or an SSH password. SSH usernames and host-key fingerprints may be stored. Log files must never contain either password.
- Never send guessed vendor operations to a real camera. The SSH path changes only how bytes reach port 15740.
- Front-end copy: sentence case, plain words, errors say what happened and what to do. Spec names: "Access Authentication", "Interval timer", "Shoot".
- All colours through CSS custom properties; both themes complete.
- Settings files from Phases 1 and 2 must still load: every new field has a default.
- Tests run without hardware: the fake runner for Wi-Fi, the simulator (now with an SSH front) for the camera.

## Review Focus

1. The camera's network is not visible for the first few seconds (camera just put on its Wi-Fi Direct Info screen): the join must retry for about ten seconds before reporting "This Mac can't see <network> yet…", and the message must say what to check on the camera. (Task 1)
2. A wrong SSH password: the camera refuses, and the app must say "The camera refused the username or password. Check Access Authen. Info on the camera." without locking the UI or retrying in a loop (Sony warns the camera may stop accepting connections after repeated bad attempts). (Task 4, Task 5)
3. A host-key fingerprint that differs from the stored one (camera reset, or a different camera at the same address): the app must stop and show both fingerprints, never connect silently. (Task 4, Task 5)
4. The interval timer with a camera that becomes busy (previous shot still being written): a missed tick must be skipped and counted as missed, the timer must keep going, and Stop must always work. (Task 7)
5. Windows `netsh` profile handling: the profile XML holding the password must be deleted after the connect command, even when the command fails. (Task 8)

---

## File structure

```
crates/rimix-wifi/                      moved from apps/desktop/src-tauri/src/wifi.rs, plus windows.rs
  src/lib.rs                            Runner, Output, WifiError, Wifi (fake, from_env, device, current_network, join), JoinReport
  src/macos.rs                          networksetup back end (retry on "Could not find network")
  src/windows.rs                        netsh wlan back end (profile XML in a temp file, deleted after)
crates/sony-ptpip/src/ssh.rs            SshTunnel: russh client, keyboard-interactive/password auth, fingerprint check, local forwarder
crates/sony-ptpip/src/camera.rs         CameraConfig.ssh, CameraConfig.before_reconnect; Camera::connect through the tunnel
crates/sony-ptpip/src/camera/loops.rs   supervise calls before_reconnect before each attempt after the first
crates/sony-ptpip/src/error.rs          Error::SshAuth, Error::HostKeyMismatch { expected, actual }, Error::HostKeyUnknown { sha256, md5 }
crates/sony-ptpip/src/quirks.rs         Generation, ssh flag, rows for known bodies
crates/sony-ptpip-sim/src/ssh.rs        russh server: password + keyboard-interactive, direct-tcpip to the PTP port; fingerprint exposed
crates/rimix-cli/src/{main,commands}.rs `rimix wifi join|status`, `--ssh-user`, `--ssh-password-stdin`, `--fingerprint`
apps/desktop/src-tauri/src/logging.rs   rolling file log under <config>/Rimix/logs, plus `open_logs` command
apps/desktop/src-tauri/src/{state,commands,settings}.rs  SSH commands, RecentCamera.ssh_user/ssh_fingerprint, rejoin hook
apps/desktop/src/components/connectScreen.ts  Access Authentication fields and fingerprint confirmation
apps/desktop/src/components/intervalTimer.ts  the timer popover and status
apps/desktop/src/interval.ts            timer state machine (pure)
apps/desktop/src/components/sidePanels.ts     Interval timer row opens the popover; Cameras rows keyed by network
apps/desktop/src/copy.ts                new copy
.github/workflows/{ci,release}.yml      windows-latest jobs
README.md, docs/camera-testing.md       log file, Windows, Access Authentication
```

---

### Task 1: A dependable join, a log file, and a Wi-Fi command in the CLI

**Files:**
- Create: `crates/rimix-wifi/Cargo.toml`, `crates/rimix-wifi/src/lib.rs`, `crates/rimix-wifi/src/macos.rs` (moved from `apps/desktop/src-tauri/src/wifi.rs`)
- Create: `apps/desktop/src-tauri/src/logging.rs`
- Modify: `Cargo.toml` (workspace member), `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/src/{lib,state,commands}.rs`, `apps/desktop/src/copy.ts`, `apps/desktop/src/components/appSettings.ts`, `crates/rimix-cli/{Cargo.toml,src/main.rs,src/commands.rs}`
- Test: `crates/rimix-wifi/src/lib.rs` unit tests, `apps/desktop/src/copy.test.ts`, `crates/rimix-cli/tests/cli.rs`

**Interfaces:**
- `rimix-wifi` exports what `wifi.rs` exported (`Runner`, `Output`, `SystemRunner`, `WifiError`, `Wifi`, `JOIN_WAIT`, `JOIN_POLL`, `parse_wifi_device`, `parse_current_network`) unchanged, plus:
  - `pub const FIND_RETRIES: u32 = 3; pub const FIND_RETRY_WAIT: Duration = Duration::from_secs(4);`
  - `WifiError::NotVisible(String)` with Display `"This computer can't see the network <name> yet"`: returned when `networksetup -setairportnetwork` prints `Could not find network` on every one of `FIND_RETRIES` attempts spaced `FIND_RETRY_WAIT` apart (the Wi-Fi scan cache refreshes within a few seconds).
  - The address branch of the join poll is rejected when the SSID reads back as a different non-empty network (fixes the Phase 2 parked item "join poll accepts the old address").
  - `pub struct JoinReport { pub attempts: u32, pub seen_ssid: Option<String>, pub address: Option<String> }` returned by `pub fn join_report(&self, ssid, password) -> Result<JoinReport, WifiError>`; `join` stays and discards the report. Every `networksetup` call is logged at `info` with its argv minus the password (`[password]` in its place) and its trimmed output.
- `logging.rs`: `pub fn init(dir: &Path) -> std::io::Result<tracing_appender::non_blocking::WorkerGuard>` installs a `tracing_subscriber` with a daily-rolling file `rimix.log` under `dir` (`<config>/Rimix/logs`), level `info` (overridable by `RIMIX_LOG`), and a stderr layer in debug builds. The guard is kept in app state. `pub fn redact(args: &[&str], secret_positions: &[usize]) -> String` is what the Wi-Fi logging uses.
- Command `open_logs()` opens the logs folder with the opener plugin; App settings gets a row `Logs` with a button `Show log folder` under the Connection section.
- `copy.ts`: `wifiErrorMessage('join', ssid, detail)` where `detail` starts with `This computer can't see the network` → `"This Mac can't see ${ssid} yet. On the camera, open Menu, Network, PC Remote Function, Wi-Fi Direct Info and stay on that screen, then try again."`
- CLI: `rimix wifi status` prints the device, the current network name or `hidden by macOS`, and the address; `rimix wifi join <ssid> [--password-stdin]` runs `join_report` with `-v` logging and prints the report (exit 3 on `NotVisible`, 4 on other `Failed`).

- [ ] **Step 1: Move `wifi.rs` into the new crate** (same code, same tests; `apps/desktop` depends on `rimix-wifi`), run `cargo test --workspace` to see everything still pass, commit `Move Wi-Fi joining into its own crate`.

- [ ] **Step 2: Write the failing tests** in `rimix-wifi`:

```rust
#[cfg(target_os = "macos")]
#[test]
fn join_retries_when_the_network_is_not_visible_yet() {
    // ports, getairportnetwork(not associated), set → "Could not find network X.", set → "Could not find network X.", set → "" (ok), getifaddr → "192.168.122.5"
    let report = wifi.join_report("DIRECT-abc1:ZV-E10", Some("pw")).unwrap();
    assert_eq!(report.attempts, 3);
    assert_eq!(runner.calls().iter().filter(|c| c.contains(&"-setairportnetwork".to_string())).count(), 3);
}
#[cfg(target_os = "macos")]
#[test]
fn join_gives_not_visible_after_the_last_retry() {
    let err = wifi.join("DIRECT-abc1:ZV-E10", Some("pw")).unwrap_err();
    assert!(matches!(err, WifiError::NotVisible(ref n) if n == "DIRECT-abc1:ZV-E10"));
    assert_eq!(err.to_string(), "This computer can't see the network DIRECT-abc1:ZV-E10 yet");
}
#[cfg(target_os = "macos")]
#[test]
fn an_address_on_a_different_named_network_does_not_count_as_joined() {
    // set ok; poll: getairportnetwork → "Current Wi-Fi Network: Home", getifaddr → "192.168.31.5" … until the deadline
    assert!(matches!(wifi.join("DIRECT-abc1:ZV-E10", None), Err(WifiError::Failed(_))));
}
#[test]
fn redact_hides_the_password_position() {
    assert_eq!(redact(&["networksetup", "-setairportnetwork", "en0", "X", "secret"], &[4]), "networksetup -setairportnetwork en0 X [password]");
}
```

`copy.test.ts`: the new join message. `cli.rs`: `rimix wifi status` exits 0 and prints `device:` on macOS (skip on other targets); `rimix wifi join` with `RIMIX_WIFI=fake` prints `attempts: 1`.

- [ ] **Step 3: Run them to verify they fail** — `cargo test -p rimix-wifi`, `pnpm -C apps/desktop test copy`, `cargo test -p rimix-cli`.

- [ ] **Step 4: Implement** the retry, the name-mismatch rejection, `JoinReport`, the logging and redaction, `logging.rs` wired in `lib.rs::run` (log dir from `settings::config_dir()`), the `open_logs` command and App settings row, and the CLI subcommand.

- [ ] **Step 5: Verify** — `cargo test --workspace && pnpm -C apps/desktop test && pnpm -C apps/desktop exec tsc --noEmit`; then `cargo run -p rimix-cli -- wifi status` prints this Mac's device and address.

- [ ] **Step 6: Commit** — `Retry Wi-Fi joins and keep a log file`.

---

### Task 2: Rejoin the camera's Wi-Fi during reconnect

**Files:**
- Modify: `crates/sony-ptpip/src/camera.rs`, `crates/sony-ptpip/src/camera/loops.rs`, `apps/desktop/src-tauri/src/state.rs`, `apps/desktop/src/components/topbar.ts`, `apps/desktop/src/copy.ts`
- Test: `crates/sony-ptpip/tests/camera.rs`, `apps/desktop/src-tauri/src/state.rs` tests

**Interfaces:**
- `pub type ReconnectHook = Arc<dyn Fn(u32) -> BoxFuture<'static, ()> + Send + Sync>;` `CameraConfig` gains `#[serde(skip)] pub before_reconnect: Option<ReconnectHook>` (so `CameraConfig` keeps `Clone`; drop `PartialEq`/`Debug` derives if they no longer hold and implement `Debug` by hand printing `before_reconnect: <set|none>`). `supervise` awaits the hook with the attempt number before every reconnect attempt from attempt 2 on (the first attempt is immediate, since most drops are not Wi-Fi).
- `state.rs::connect` passes a hook when `ssid` is `Some`: it calls `wifi.join(ssid, None)` in `spawn_blocking` and logs the outcome; errors are logged, never surfaced (the PTP reconnect reports its own failure).
- `CameraEvent::Reconnecting { attempt }` is unchanged; the top bar text after attempt 5 becomes `Reconnecting… if the camera's Wi-Fi dropped, rejoin it from the Wi-Fi menu` (copy function `reconnectingText(attempt)`).

- [ ] **Step 1: Write the failing tests**

```rust
// tests/camera.rs
#[tokio::test]
async fn the_reconnect_hook_runs_before_the_second_attempt() {
    // connect to the sim with reconnect: true and a hook that records attempt numbers; DropConnection twice quickly via the sim
    // assert the hook saw attempt 2 (not 1) within 10 s and the camera came back Connected
}
```

`state.rs`: `connect(state, ip, Some("DIRECT-x"))` builds a config whose `before_reconnect` is `Some`; `connect(state, ip, None)` gives `None` (expose a test-only accessor or test through `Camera`'s debug output).

`copy.test.ts`: `reconnectingText(1)` → `Reconnecting`, `reconnectingText(5)` → the longer text.

- [ ] **Step 2: Run to verify failure** — `cargo test -p sony-ptpip --test camera reconnect_hook`.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Verify** — `cargo test --workspace`, Vitest, tsc.
- [ ] **Step 5: Commit** — `Rejoin the camera's Wi-Fi when the link drops`.

---

### Task 3: An SSH front for the simulator

**Files:**
- Create: `crates/sony-ptpip-sim/src/ssh.rs`
- Modify: `crates/sony-ptpip-sim/Cargo.toml` (russh 0.65), `src/lib.rs` (`SimConfig.ssh`, `SimHandle.ssh_addr`, `SimHandle.fingerprint_sha256`), `src/ssdp.rs` (`X_SSH_Support` true when ssh is on), `src/main.rs` (`--ssh USER:PASSWORD`)

**Interfaces:**
- `pub struct SshSimConfig { pub user: String, pub password: String }`; `SimConfig.ssh: Option<SshSimConfig>` (default `None`).
- When set, `spawn` starts a `russh` server on a free port with a fresh ed25519 host key, accepting `auth_password` and `auth_keyboard_interactive` (one prompt `Password:`) for that user and password only (three bad tries close the connection, like the camera's lockout), and serving `channel_open_direct_tcpip` only to port 15740 (any host string) by connecting to the sim's own PTP listener and piping both ways. Other requests are rejected.
- `SimHandle.ssh_addr: Option<SocketAddr>`, `SimHandle.fingerprint_sha256: Option<String>` (OpenSSH form `SHA256:<base64 without padding>`) and `fingerprint_md5: Option<String>` (`aa:bb:…`).
- The SSDP/HTTP description reports `<X_SSH_Support>true</X_SSH_Support>` when ssh is on.

- [ ] **Step 1: Write the failing test** in `crates/sony-ptpip-sim/src/ssh.rs`:

```rust
#[tokio::test]
async fn ssh_front_forwards_a_ptp_session() {
    // spawn with ssh user "cam" password "12345678"; connect with russh client, authenticate_password, open direct-tcpip to ("127.0.0.1", 15740)
    // write a PTP/IP Init_Command_Request through the channel and read an Init_Command_Ack (use sony_ptpip::ptpip::packet to encode/decode)
    // wrong password → AuthResult::Failure; after 3 failures the connection is closed
}
```

- [ ] **Step 2: Run to verify failure.** — `cargo test -p sony-ptpip-sim ssh`
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Verify** — `cargo test -p sony-ptpip-sim`, `cargo run -p sony-ptpip-sim -- --ssh cam:12345678` prints the SSH port and fingerprint.
- [ ] **Step 5: Commit** — `Give the simulator an SSH front like newer cameras`.

---

### Task 4: The SSH tunnel transport in the camera library

**Files:**
- Create: `crates/sony-ptpip/src/ssh.rs`
- Modify: `crates/sony-ptpip/Cargo.toml` (russh 0.65), `src/error.rs`, `src/lib.rs`, `src/camera.rs`, `src/camera/loops.rs` (`connect_once` reopens the tunnel)
- Test: `crates/sony-ptpip/tests/ssh.rs`

**Interfaces:**
- `pub struct SshConfig { pub user: String, pub password: String, pub expected_fingerprint: Option<String> }` (`password` is `#[serde(skip)]` and not `Debug`-printed).
- `pub struct SshTunnel { .. }` with `pub async fn open(camera: SocketAddr /* ssh port 22 */, cfg: &SshConfig, forward_to_port: u16) -> Result<SshTunnel, Error>` and `pub fn local_addr(&self) -> SocketAddr`; dropping it closes the listener and the session. `open` connects, verifies the host key: with `expected_fingerprint` set, the key's SHA256 or MD5 fingerprint must match after normalisation (`normalize_fingerprint`: strip `SHA256:`/`MD5:`, colons, whitespace, case); a mismatch is `Error::HostKeyMismatch { expected, actual_sha256 }`; with none set, the result is `Error::HostKeyUnknown { sha256, md5 }` and no auth is attempted. Auth tries keyboard-interactive first (answering every prompt with the password), then password; failure is `Error::SshAuth`. Each accepted local connection opens `direct-tcpip` to `("127.0.0.1", forward_to_port)` and copies both ways.
- `pub fn normalize_fingerprint(s: &str) -> String`.
- `CameraConfig.ssh: Option<SshConfig>`; when set, `Camera::connect` opens the tunnel first and dials `local_addr()`; `connect_once` (reconnect) reopens it. `CameraState` gains `pub secured: bool` (true when connected through SSH).
- `Error::SshAuth` Display: `the camera refused the username or password`; `HostKeyMismatch` Display: `the camera's fingerprint changed (expected {expected}, got {actual_sha256})`; `HostKeyUnknown` Display: `the camera's fingerprint needs confirming: {sha256}`.

- [ ] **Step 1: Write the failing tests** in `tests/ssh.rs`:

```rust
#[tokio::test] async fn tunnel_connects_a_camera_behind_ssh() { /* sim with ssh; Camera::connect with ssh config carrying the sim's fingerprint; state.connected && state.secured; shoot works */ }
#[tokio::test] async fn unknown_fingerprint_is_reported_before_auth() { /* expected_fingerprint None → Err(HostKeyUnknown { sha256, .. }) equal to sim.fingerprint_sha256; the sim's auth log shows no attempt */ }
#[tokio::test] async fn wrong_fingerprint_is_refused() { /* expected "SHA256:AAAA" → HostKeyMismatch */ }
#[tokio::test] async fn wrong_password_is_ssh_auth() { .. }
#[tokio::test] async fn tunnel_is_reopened_on_reconnect() { /* reconnect: true; sim DropConnection; camera returns Connected; secured stays true */ }
#[test] fn fingerprints_normalise() { assert_eq!(normalize_fingerprint("SHA256:AbC/+="), normalize_fingerprint("abc/+")); assert_eq!(normalize_fingerprint("MD5:aa:BB:cc"), "aabbcc"); }
```

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.** (The tunnel's forwarder task runs under the camera's runtime; use `tokio::io::copy_bidirectional` between the accepted socket and the channel's stream.)
- [ ] **Step 4: Verify** — `cargo test -p sony-ptpip`, clippy clean.
- [ ] **Step 5: Commit** — `Connect through SSH for cameras with Access Authentication`.

---

### Task 5: Access Authentication in the app and the CLI

**Files:**
- Modify: `apps/desktop/src-tauri/src/{settings,state,commands}.rs`, `apps/desktop/src/{ipc,actions,copy,state}.ts`, `apps/desktop/src/components/{connectScreen,sidePanels,topbar}.ts`, `apps/desktop/src/styles/layout.css`, `crates/rimix-cli/src/{main,commands}.rs`
- Test: `settings.rs`, `state.rs`, `actions.test.ts`, `connectScreen.test.ts`, `copy.test.ts`, `crates/rimix-cli/tests/cli.rs`

**Interfaces:**
- `RecentCamera` gains `#[serde(default)] pub ssh_user: Option<String>` and `pub ssh_fingerprint: Option<String>` (never a password). `remember_camera` gains an `ssh: Option<(&str, &str)>` (user, fingerprint) argument.
- Rust `connect(state, ip, ssid, ssh: Option<SshLogin>)` where `pub struct SshLogin { pub user: String, pub password: String, pub fingerprint: Option<String> }`; the error string for `HostKeyUnknown` is `"HOSTKEY " + sha256 + " " + md5` (the front end parses it); `SshAuth` and `HostKeyMismatch` pass through as text. Command `connect(ip, ssid, ssh)`.
- `ipc.connect(ip, ssid, ssh?: SshLogin)`; `actions.connect(ip, ssid?, ssh?)`; `actions.connectWithWifi(creds, ssh?)`.
- `AppState.pendingFingerprint: { sha256: string; md5: string } | null`. When a connect fails with `HOSTKEY`, the connect screen shows a confirmation block: `This camera's fingerprint is <sha256>. Check it against Access Authen. Info on the camera.` with buttons `It matches, connect` (re-run the same connect with `fingerprint: sha256`) and `Cancel`.
- Connect screen: under the name-and-password card, a disclosure `This camera uses Access Authentication` (open automatically when the chosen Nearby camera reports `ssh_supported`, or when a remembered camera has `ssh_user`) with `Username` and `Password` fields; the username is prefilled from the remembered camera. The password is never written to settings; the fingerprint is stored once confirmed.
- Top bar: `secured` shows a small lock glyph with `aria-label` `Connected through Access Authentication`.
- Copy: `sshErrorMessage(err)`: `SshAuth` → `The camera refused the username or password. Check Access Authen. Info on the camera and try again.`; `HostKeyMismatch` → `The camera's fingerprint changed. If you reset the camera, forget it in App settings and connect again; otherwise do not connect.`
- CLI: `rimix connect|props|shoot|… --ssh-user U --ssh-password-stdin --fingerprint F` on every camera command (a shared `SshArgs` group); an unknown fingerprint prints it and exits 3 with `Confirm it on the camera and pass --fingerprint`.

- [ ] **Step 1: Write the failing tests** (Rust: settings fields default and round-trip with ssh_user; `connect` with the ssh sim returns `secured: true`; `HOSTKEY` string shape pinned. TS: actions parse `HOSTKEY` into `pendingFingerprint`; "It matches, connect" re-calls `ipc.connect` with the fingerprint; the disclosure opens for an `ssh_supported` camera; `sshErrorMessage` strings; CLI: `rimix shoot --ssh-user cam --ssh-password-stdin` against the ssh sim saves a file; without `--fingerprint` exit 3 and the fingerprint printed.)
- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Verify** — all suites; then the flow by hand against `sony-ptpip-sim --ssh cam:12345678` with `RIMIX_WIFI=fake RIMIX_CAMERA_ADDR=127.0.0.1:15740`: the fingerprint confirmation appears, confirming connects, the lock shows.
- [ ] **Step 5: Commit** — `Let the app and CLI sign in to Access Authentication cameras`.

---

### Task 6: The per-model quirk table

**Files:**
- Modify: `crates/sony-ptpip/src/quirks.rs`, `src/camera.rs` (quirks chosen from model and device info), `crates/sony-ptpip-sim/fixtures/zv-e10.json` (unchanged values; add a second fixture `ilce-7m4.json` with model `ILCE-7M4`)
- Test: `quirks.rs` unit tests, `tests/camera.rs`

**Interfaces:**
- `pub enum Generation { Gen3, Gen4 }`; `Quirks` gains `pub generation: Generation`, `pub access_authentication: bool` (the body offers SSH), `pub direct_write_properties: &'static [u16]` (empty means "try direct, then step" as today).
- `pub fn quirks_for(model: &str) -> Quirks` rows: `ZV-E10` (Gen3, no SSH, defaults), `ILCE-7M4`, `ILCE-7RM5`, `ILCE-6700`, `ILCE-7CM2`, `ZV-E1`, `ILME-FX3`, `ILME-FX30` (Gen4, SSH, `WriteMode::Direct`), unknown → ZV-E10 defaults. `pub fn quirks_from(model: &str, ssh_supported: Option<bool>) -> Quirks` overrides `access_authentication` with the discovery flag when known.
- `CameraState` gains `pub generation: String` (`"3"`/`"4"`) for the top bar tooltip.

- [ ] **Step 1: Write the failing tests** (`quirks_for("ILCE-7M4").generation == Gen4` and `.access_authentication`; unknown model equals ZV-E10; `quirks_from("ZV-E10", Some(true)).access_authentication`; camera test with the `ilce-7m4.json` fixture reports `generation == "4"`).
- [ ] **Step 2: Run to verify failure.** — [ ] **Step 3: Implement.** — [ ] **Step 4: Verify.** — [ ] **Step 5: Commit** — `Describe more camera models in the quirk table`.

---

### Task 7: Interval timer

**Files:**
- Create: `apps/desktop/src/interval.ts`, `apps/desktop/src/interval.test.ts`, `apps/desktop/src/components/intervalTimer.ts`, `apps/desktop/src/components/intervalTimer.test.ts`
- Modify: `apps/desktop/src/{state,actions,app}.ts`, `apps/desktop/src/components/{sidePanels,dock,ring}.ts`, `apps/desktop/src/styles/layout.css`
- Test: `sidePanels.test.ts`, `dock.test.ts`

**Interfaces:**
- `interval.ts` (pure): `export const MIN_EVERY_S = 2; export interface IntervalPlan { everyS: number; count: number | null }`; `export interface IntervalRun { plan; taken: number; missed: number; nextAt: number; running: boolean }`; `export function start(plan, now): IntervalRun`; `export function tick(run, now, cameraBusy: boolean): { run: IntervalRun; shoot: boolean }` (when `now >= nextAt`: if busy, `missed += 1`, else `shoot = true, taken += 1`; `nextAt += everyS * 1000`; `running = false` when `count !== null && taken + missed >= count`); `export function statusText(run, now): string` → `Interval 3 of 20, next in 4 s` or `Interval 3 taken, next in 4 s` when unlimited, plus `, 1 missed` when missed > 0.
- `state.ts`: `interval: IntervalRun | null`; `camera.recording` or a transfer in flight for the newest file counts as busy.
- `actions.ts`: `startInterval(plan)`, `stopInterval()`; a 250 ms ticker in `app.ts` applies `tick` and calls `actions.shoot()` when told; the run stops on disconnect and on `Stop`.
- `intervalTimer.ts`: the Scopes and timer row `Interval timer` shows `Off` or `Every 5 s`; clicking opens a glass popover with `Every` (seconds, number input, min 2), `Shots` (number or empty for unlimited), `Start`/`Stop`. The dock's status text shows `statusText` while running.

- [ ] **Step 1: Write the failing tests** (pure: `tick` shoots on time, skips and counts when busy, stops at count; `statusText` strings; component: Start calls `startInterval({ everyS: 5, count: 20 })`, `Every` below 2 is clamped, Stop clears; app: after `startInterval` and 5 s of fake time with a connected camera, `actions.shoot` was called once; disconnect clears `interval`).
- [ ] **Step 2: Run to verify failure.** — [ ] **Step 3: Implement.** — [ ] **Step 4: Verify** — Vitest, tsc, and a run against the simulator: 3 shots at 2 s land in the filmstrip. — [ ] **Step 5: Commit** — `Add an interval timer that shoots on a schedule`.

---

### Task 8: Windows: Wi-Fi join, CI, installer

**Files:**
- Create: `crates/rimix-wifi/src/windows.rs`
- Modify: `crates/rimix-wifi/src/lib.rs`, `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `apps/desktop/src-tauri/tauri.conf.json` (`bundle.windows` with `nsis` target), `README.md`
- Test: `windows.rs` unit tests with the fake runner (gated `#[cfg(target_os = "windows")]`), plus parser tests that run everywhere

**Interfaces:**
- Windows back end: `device()` → the first `Name` under `netsh wlan show interfaces`; `current_network()` → the `SSID` line of that output when `State : connected`; `join(ssid, password)` → write a WPA2-PSK profile XML (`profile_xml(ssid, password)`, pure, tested on every OS) to a `tempfile::NamedTempFile`, run `netsh wlan add profile filename=<path> user=current`, `netsh wlan connect name=<ssid>`, then poll `netsh wlan show interfaces` for `State : connected` and the SSID (or `ipconfig` for an address) under `JOIN_WAIT`; the temp file is removed in every path (the `NamedTempFile` drop guarantees it; the test checks the path no longer exists after a failing connect). Without a password, skip the profile step. `Could not find`-style failures (`There is no profile` / `The network specified is not available`) map to `NotVisible` after the same retries as macOS.
- CI: a second job `windows` on `windows-latest` running `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` (Tauri's build deps need the `windows-latest` toolchain; the simulator tests use loopback sockets and must pass there).
- Release: a `windows` job building `--target x86_64-pc-windows-msvc` with `tauri-action` into the same draft release (NSIS `.exe` installer); README `Install on Windows` (unsigned installer, SmartScreen "More info, Run anyway") and a note that Wi-Fi joining uses `netsh` and needs no admin rights.

- [ ] **Step 1: Write the failing tests** (`profile_xml` contains the ssid in `<name>` and `<SSID><name>`, the password in `<keyMaterial>`, `WPA2PSK`/`AES`; `parse_interfaces` extracts `Name`, `State`, `SSID`; the join flow under the fake runner on Windows; YAML of both workflows parses).
- [ ] **Step 2: Run to verify failure** (parser tests here; the Windows-gated ones in CI).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Verify** — macOS suites green; `cargo check -p rimix-wifi --target x86_64-pc-windows-msvc` if the target is installed (`rustup target add x86_64-pc-windows-msvc` is allowed; it checks `rimix-wifi` only, not Tauri); YAML parses.
- [ ] **Step 5: Commit** — `Build and test Rimix on Windows in CI`.

---

### Task 9: Phase 2 leftovers

**Files:**
- Modify: `apps/desktop/src/components/sidePanels.ts` (Cameras rows keyed by network: the "this camera" row matches by ssid when the entry has one, by ip otherwise; Phase 1 entries without a port match entries with `:15740`), `apps/desktop/src-tauri/src/settings.rs` (`remember_camera` treats `ip` and `ip:15740` as the same address), `.github/workflows/release.yml` (`printf '%s\n'` for secret values), `apps/desktop/src/components/presetsControl.ts` (Rename/Replace/Delete disabled while `presetBusy`)
- Test: `sidePanels.test.ts`, `settings.rs`, `presetsControl.test.ts`

- [ ] **Step 1: Write the failing tests** (two remembered Wi-Fi Direct cameras at the same address both show in the Cameras panel while connected to one; `remember_camera("192.168.122.1", …)` then `("192.168.122.1:15740", …)` yields one entry; menu items disabled while busy).
- [ ] **Step 2: Run to verify failure.** — [ ] **Step 3: Implement.** — [ ] **Step 4: Verify.** — [ ] **Step 5: Commit** — `Tell cameras apart in the Cameras panel`.

---

### Task 10: Docs for the owner

**Files:**
- Modify: `README.md`, `docs/camera-testing.md`

- `camera-testing.md` step 2 gains "If Rimix says it can't see the network": check the camera is on its Wi-Fi Direct Info screen, wait ten seconds, try again; then run `rimix wifi join "<name>"` in Terminal and send back its output and the newest file in `~/Library/Application Support/Rimix/logs`. A new step 8 for Access Authentication bodies (the owner has none; keep it short). The "If something goes wrong" section points at the log folder and the `Show log folder` button.
- README: the log folder, `rimix wifi`, Access Authentication, the interval timer, Windows install, and the CI matrix.

- [ ] **Step 1: Write the docs.** — [ ] **Step 2: Check every command and menu name in them against the code** (`grep` the copy strings). — [ ] **Step 3: Commit** — `Explain logs, Access Authentication and Windows`.

---

## Self-review notes

- Spec coverage for Phase 3 (section 7): Access Authentication (Tasks 3 to 5), per-model quirk table (Task 6), interval timer (Task 7), Windows build and installer (Task 8). Section 5 keepalive/reconnect: Wi-Fi rejoin (Task 2). The owner's failure: Task 1. Phase 2 parked items: Tasks 1 and 9.
- Deferred again, on purpose: the "connect without pairing" handshake variant (its wire difference is still undocumented in anything we may read); BLE stays in Phase 4.
- Type consistency: `SshConfig` (library) versus `SshLogin` (IPC) are deliberately different: the IPC one carries the password only for the call. `RecentCamera.ssh_user`/`ssh_fingerprint` are the only persisted SSH facts. `JoinReport` is CLI-facing only.
- Review Focus lines 1 to 5 each have a test in Tasks 1, 4/5, 4/5, 7, 8.
