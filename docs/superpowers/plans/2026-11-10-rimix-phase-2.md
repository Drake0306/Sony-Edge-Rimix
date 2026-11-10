# Rimix Phase 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Onboard the camera the way the phone does: scan the camera's QR code with the webcam (or type the Wi-Fi name and password it shows), have the Mac join the camera's Wi-Fi Direct network by itself, find the camera and connect, with no IP address in sight; remember cameras by network so the next connection is one click; add presets, the deferred keyboard behaviour, an offline font, and a release workflow that produces a DMG.

**Architecture:** Everything the camera says stays in `crates/sony-ptpip` (unchanged except one backoff). OS integration (joining Wi-Fi) is a new module in the Tauri crate that shells out to macOS's `networksetup` behind a small runner trait so tests never touch the real radio; a fake mode driven by environment variables lets the whole flow run against the simulator. QR decoding happens inside the webview (webcam frames through `jsQR`, Apache-2.0); the decoded text goes through a permissive parser that accepts the standard `WIFI:` grammar, the Sony-style `W01:` grammar and any `S:`/`P:` field pair, and falls back to showing the text and letting the user fill two fields. The connect screen's three ways become: Scan the QR code, Nearby, Type the Wi-Fi name and password (with connect-by-address under an Advanced disclosure). Presets are app-side bundles stored in settings and written through the existing `Camera::set`.

**Tech Stack:** As Phase 1 (Rust 1.93, tokio, Tauri 2.12, Vite + TypeScript, Vitest + jsdom, pnpm) plus `jsqr` 1.4 (Apache-2.0) and `qrcode` (MIT, dev only, to make test QR codes), `@fontsource-variable/inter` (OFL-1.1), `tauri-apps/tauri-action` in GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-11-10-rimix-design.md`, sections 5 (Wi-Fi joining, onboarding gate, pairing fallback), 6.2 (keyboard), 6.5 (App settings), 6.6 (Connect screen), 6.8 (Presets), 7 (Phase 2 list). Owner's direction on 2026-11-10: "for devices we have SSIDs and password along with QR, not IP", so the typed path is name and password, and IP entry becomes the advanced fallback.

## Global Constraints

- Licence Apache-2.0 for everything here. New dependencies must be Apache-2.0, MIT, BSD or OFL (fonts). Nothing from GPL or LGPL projects.
- Commit messages: plain language, under 60 characters, no "Co-Authored-By" or any Claude attribution. Example: `Scan the camera's QR code with the webcam`.
- Platforms: macOS 14+ first; nothing may break a Windows build. Anything that runs a macOS command lives under `#[cfg(target_os = "macos")]`, with an `Unsupported` result elsewhere.
- Never store a Wi-Fi password on disk. The OS remembers the network once joined; Rimix stores only the network name.
- Never send guessed vendor operations to a real camera. Presets write only values that appear in that property's own settable list or range (the `Camera::set` path already enforces this).
- Front-end copy: sentence case, plain words, the spec's names ("Shoot", "Record", "Focus", "Save to", "Connect", "App settings"). Errors say what happened and what to do.
- All colours through CSS custom properties; both themes complete; the glass language of `design/mockups/20-rimix-final.html`.
- Settings files written by Phase 1 must still load: every new field has a default.
- Tests run without hardware: Rust tests use the fake runner and the simulator; Vitest uses jsdom with mocked `@tauri-apps/api`.

## Review Focus

Inputs the spec implies but no happy path exercises. Each line's test is in the task that owns the code.

1. QR text that is not camera Wi-Fi credentials (a URL, a product link, random text) must show "This code isn't a camera Wi-Fi code" together with the decoded text and the two editable fields, never crash and never silently ignore the scan. (Task 3, Task 5)
2. The Mac joins the network but no camera answers on port 15740 within the wait (camera not on its PC Remote screen): one clear message naming the camera's menu path after the deadline, all buttons usable again, no endless spinner. (Task 2, Task 5)
3. The webcam is denied, missing, or `navigator.mediaDevices` is undefined (an origin the webview does not treat as secure): the QR card explains and points at paste and typing; no unhandled promise rejection. (Task 4)
4. A preset applied while the camera's mode makes a value read-only (aperture in P mode): that value is reported by its setting name, the other values are still written, and the slot never stays busy. (Task 7)
5. A settings file from Phase 1 (no presets, Wi-Fi join, webcam or network fields) loads with defaults and saves back with them; a preset saved for one camera model is not shown for another. (Task 1, Task 7)

---

## File structure

```
apps/desktop/src-tauri/
  Info.plist                              NSCameraUsageDescription (merged by tauri-build)
  Entitlements.plist                      com.apple.security.device.camera for signed builds
  tauri.conf.json                         CSP without Google Fonts; bundle.macOS entitlements + minimumSystemVersion
  src/settings.rs                         + WifiJoin, Preset, PresetValue, RecentCamera.ssid, webcam_id, remember_camera(.., ssid, ..), forget_camera
  src/wifi.rs                             Runner trait, SystemRunner, Wifi { device, current_network, join }, parse_* helpers, WifiError
  src/state.rs                            + join_network, current_network, wait_for_camera, apply_values; read_thumbnail restricted to Rimix folders
  src/commands.rs, src/lib.rs             register the four new commands
apps/desktop/src/
  qr/payload.ts                           parseWifiQr(text) -> WifiCredentials | null
  qr/scanner.ts                           webcamSupported, listWebcams, decodeFrame (jsQR), createScanner
  qr/payload.test.ts, qr/scanner.test.ts
  presets.ts                              presetFor, captureValues, newPreset, describeFailures
  presets.test.ts
  components/presetsControl.ts            the C1..C4 control shared by dock and ring
  components/connectScreen.ts             rewritten: QR card with live preview + paste, Nearby, name+password card with Advanced address
  components/appSettings.ts               + Connection section (Wi-Fi join, Webcam, Remembered cameras)
  components/dock.ts, components/ring.ts  use presetsControl; arrow keys step the open setting
  actions.ts                              + connectWithWifi, connectRecent, forgetCamera, applyPreset, savePreset, deletePreset
  copy.ts                                 + join/find error copy, webcam error copy
  ipc.ts                                  + new types and command wrappers
  state.ts                                + connecting, presetBusy, activePreset
  main.ts                                 import the bundled Inter font
  styles/layout.css                       connect screen additions, presets menu, busy state
crates/sony-ptpip/src/camera/loops.rs     live view backoff after repeated failures
.github/workflows/release.yml             tag -> universal macOS DMG, signed and notarized when secrets exist
README.md, docs/camera-testing.md         install, release and fake-Wi-Fi development; step 2 uses the app
```

---

### Task 1: Settings model for networks, presets and connection preferences

**Files:**
- Modify: `apps/desktop/src-tauri/src/settings.rs`
- Modify: `apps/desktop/src-tauri/src/state.rs` (the `connect` call to `remember_camera`)
- Modify: `apps/desktop/src/ipc.ts`, `apps/desktop/src/state.ts` (`defaultSettings`)
- Test: `settings.rs` unit tests, `apps/desktop/src/state.test.ts`

**Interfaces:**
- Produces (Rust, all `Serialize + Deserialize`, camelCase like the rest of `AppSettings`):
  - `pub enum WifiJoin { Automatic (default), Ask }`
  - `pub struct PresetValue { pub code: u16, pub raw: PtpValue }`
  - `pub struct Preset { pub id: String, pub name: String, pub model: String, pub slot: u8, pub values: Vec<PresetValue> }` (slot 1 to 4)
  - `RecentCamera` gains `#[serde(default)] pub ssid: Option<String>`
  - `AppSettings` gains `wifi_join: WifiJoin`, `webcam_id: Option<String>`, `presets: Vec<Preset>`
  - `pub fn remember_camera(&mut self, ip: &str, model: &str, ssid: Option<&str>, now_millis: u64)`: an entry matches an older one when the ip matches, or when both ssids are `Some` and equal; the new entry keeps the newest non-`None` ssid.
  - `pub fn forget_camera(&mut self, ip: &str)`: removes entries with that ip; clears `last_ip` if it was that ip.
- Produces (TypeScript in `ipc.ts`): `WifiJoin = 'Automatic' | 'Ask'`, `PresetValue { code: number; raw: PtpValue }`, `Preset { id; name; model; slot; values }`, `RecentCamera.ssid: string | null`, `AppSettings.wifiJoin`, `AppSettings.webcamId: string | null`, `AppSettings.presets: Preset[]`.

- [ ] **Step 1: Write the failing Rust tests** in `settings.rs`:

```rust
#[test]
fn phase_1_settings_file_loads_with_new_defaults() {
    // Exactly what Phase 1 wrote: no ssid, wifiJoin, webcamId or presets.
    let text = r#"{"appearance":"Light","layout":"Dock","destinationMode":"FolderPerDate",
      "destinationBase":"/tmp/x","recentCameras":[{"ip":"192.168.122.1","model":"ZV-E10","lastUsed":5}],
      "lastIp":"192.168.122.1"}"#;
    let s: AppSettings = serde_json::from_str(text).unwrap();
    assert_eq!(s.wifi_join, WifiJoin::Automatic);
    assert_eq!(s.webcam_id, None);
    assert!(s.presets.is_empty());
    assert_eq!(s.recent_cameras[0].ssid, None);
    let back = serde_json::to_string(&s).unwrap();
    assert!(back.contains("\"wifiJoin\":\"Automatic\""), "{back}");
    assert!(back.contains("\"presets\":[]"), "{back}");
}

#[test]
fn remember_camera_merges_by_ip_or_ssid_and_keeps_the_network() {
    let mut s = AppSettings::default();
    s.remember_camera("192.168.122.1", "ZV-E10", Some("DIRECT-abc1:ZV-E10"), 1);
    s.remember_camera("192.168.122.1", "ZV-E10", None, 2);          // same ip, no ssid known
    assert_eq!(s.recent_cameras.len(), 1);
    assert_eq!(s.recent_cameras[0].ssid.as_deref(), Some("DIRECT-abc1:ZV-E10"));
    assert_eq!(s.recent_cameras[0].last_used, 2);
    s.remember_camera("10.0.0.9", "ZV-E10", Some("DIRECT-abc1:ZV-E10"), 3); // same network, router ip
    assert_eq!(s.recent_cameras.len(), 1);
    assert_eq!(s.recent_cameras[0].ip, "10.0.0.9");
    s.remember_camera("10.0.0.7", "ILCE-7M4", Some("DIRECT-zz99:ILCE-7M4"), 4);
    assert_eq!(s.recent_cameras.len(), 2);
    s.forget_camera("10.0.0.7");
    assert_eq!(s.recent_cameras.len(), 1);
    assert_eq!(s.last_ip.as_deref(), Some("10.0.0.9"));
    s.forget_camera("10.0.0.9");
    assert!(s.recent_cameras.is_empty());
    assert_eq!(s.last_ip, None);
}

#[test]
fn preset_json_shape_is_stable() {
    let p = Preset { id: "p1".into(), name: "Talking head".into(), model: "ZV-E10".into(), slot: 1,
        values: vec![PresetValue { code: 0xD21E, raw: PtpValue::U32(800) }] };
    assert_eq!(serde_json::to_string(&p).unwrap(),
        r#"{"id":"p1","name":"Talking head","model":"ZV-E10","slot":1,"values":[{"code":53790,"raw":{"U32":800}}]}"#);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p rimix-desktop settings`
Expected: compile errors for the missing types and fields.

- [ ] **Step 3: Add the types and fields; update `remember_camera` and the existing tests that call it; update the one call in `state.rs::connect` to pass `None` for ssid (Task 5 passes the real one)**

- [ ] **Step 4: Run the crate's tests**

Run: `cargo test -p rimix-desktop`
Expected: PASS.

- [ ] **Step 5: Mirror the types in `ipc.ts`; extend `defaultSettings()` in `state.ts` with `wifiJoin: 'Automatic'`, `webcamId: null`, `presets: []`; add to `state.test.ts`:**

```ts
it('default settings carry the connection and preset fields', () => {
  const s = defaultSettings('/x');
  expect(s.wifiJoin).toBe('Automatic');
  expect(s.webcamId).toBeNull();
  expect(s.presets).toEqual([]);
});
```

Run: `pnpm -C apps/desktop test` and `pnpm -C apps/desktop exec tsc --noEmit`
Expected: PASS, no type errors (fix any fixture object literals that now miss fields).

- [ ] **Step 6: Commit**

```bash
git commit -am "Remember cameras by network and make room for presets"
```

---

### Task 2: Joining the camera's Wi-Fi and waiting for the camera

**Files:**
- Create: `apps/desktop/src-tauri/src/wifi.rs`
- Modify: `apps/desktop/src-tauri/src/state.rs`, `commands.rs`, `lib.rs`
- Modify: `apps/desktop/src/ipc.ts` (wrappers)
- Test: `wifi.rs` unit tests, `state.rs` tests

**Interfaces:**
- Produces (`wifi.rs`):
  - `pub struct Output { pub success: bool, pub stdout: String, pub stderr: String }`
  - `pub trait Runner: Send + Sync { fn run(&self, program: &str, args: &[&str]) -> std::io::Result<Output>; }`
  - `pub struct SystemRunner;` (uses `std::process::Command`)
  - `pub enum WifiError { NoWifi, Unsupported, Failed(String) }` with `Display`: `"This computer has no Wi-Fi"`, `"Joining Wi-Fi networks is not supported on this system yet"`, and the `Failed` text as is.
  - `pub struct Wifi { .. }` with `pub fn new(runner: Arc<dyn Runner>) -> Wifi`, `pub fn from_env(runner: Arc<dyn Runner>) -> Wifi` (fake when `RIMIX_WIFI=fake`: `device()` is `"fake0"`, `join` succeeds at once and `current_network()` returns the last joined name), `pub fn device(&self) -> Result<String, WifiError>`, `pub fn current_network(&self) -> Result<Option<String>, WifiError>`, `pub fn join(&self, ssid: &str, password: Option<&str>) -> Result<(), WifiError>`.
  - `pub fn parse_wifi_device(listing: &str) -> Option<String>` (the `Device:` line after `Hardware Port: Wi-Fi`), `pub fn parse_current_network(output: &str) -> Option<String>` (`Current Wi-Fi Network: NAME` → `Some(NAME)`; `You are not associated with an AirPort network.` → `None`).
  - `pub const JOIN_WAIT: Duration = Duration::from_secs(20)`; `pub const JOIN_POLL: Duration = Duration::from_secs(1)`.
  - macOS join: if `current_network()? == Some(ssid)` return Ok; run `networksetup -setairportnetwork <device> <ssid> [<password>]` (password argument only when `Some` and non-empty); if the command fails or its stdout contains `Failed` or `Could not`, return `Failed(trimmed stdout+stderr)`; then poll `current_network()` every `JOIN_POLL` until it equals `ssid` or `JOIN_WAIT` passes (then `Failed(format!("{ssid} did not come up"))`). Polling uses a `sleep: fn(Duration)` field so tests do not wait; the `Wifi` struct holds `sleep: Box<dyn Fn(Duration) + Send + Sync>` defaulting to `std::thread::sleep`.
  - Non-macOS: `device`, `join` → `Err(Unsupported)`; `current_network` → `Ok(None)`.
- Produces (`state.rs`):
  - `AppState` gains `pub wifi: Wifi` (built with `Wifi::from_env(Arc::new(SystemRunner))` in `AppState::new`).
  - `pub async fn join_network(state: &AppState, ssid: &str, password: Option<&str>) -> Result<(), String>` (runs `join` in `spawn_blocking`; error is the `WifiError` Display text).
  - `pub async fn current_network(state: &AppState) -> Result<Option<String>, String>`.
  - `pub const NO_CAMERA: &str = "No camera answered on this network";`
  - `pub async fn wait_for_camera(timeout: Duration) -> Result<DiscoveredCamera, String>`: when `RIMIX_CAMERA_ADDR` is set, return at once a `DiscoveredCamera { model: "Simulator", friendly_name: "Simulator", ip, ptp_port, ptp_version: None, pairing_required: None, ssh_supported: None, source: DiscoverySource::Manual }` parsed from it (`host:port`, port default 15740); otherwise call `sony_ptpip::discover(Duration::from_millis(1500))` repeatedly until it returns a camera (prefer `source == Gateway`, else the first) or the deadline passes, then `Err(NO_CAMERA)`.
- Produces (commands): `join_network(ssid: String, password: Option<String>) -> Result<(), String>`, `current_network() -> Result<Option<String>, String>`, `wait_for_camera(timeout_ms: u64) -> Result<DiscoveredCamera, String>`.
- Produces (`ipc.ts`): `joinNetwork(ssid, password: string | null)`, `currentNetwork()`, `waitForCamera(timeoutMs)`.

- [ ] **Step 1: Write the failing tests** in `wifi.rs` with a `FakeRunner` that records calls and answers from a scripted `VecDeque<Output>`:

```rust
#[test]
fn finds_the_wifi_device_in_the_hardware_port_listing() {
    let listing = "Hardware Port: Ethernet Adapter\nDevice: en5\nEthernet Address: ..\n\nHardware Port: Wi-Fi\nDevice: en0\nEthernet Address: 60:3e:5f:22:78:90\n";
    assert_eq!(parse_wifi_device(listing).as_deref(), Some("en0"));
    assert_eq!(parse_wifi_device("Hardware Port: Thunderbolt 1\nDevice: en1\n"), None);
}

#[test]
fn reads_the_current_network() {
    assert_eq!(parse_current_network("Current Wi-Fi Network: DIRECT-abc1:ZV-E10\n").as_deref(), Some("DIRECT-abc1:ZV-E10"));
    assert_eq!(parse_current_network("You are not associated with an AirPort network.\n"), None);
}

#[cfg(target_os = "macos")]
#[test]
fn join_runs_networksetup_and_waits_until_the_network_is_up() {
    // listallhardwareports, getairportnetwork (other network), setairportnetwork, getairportnetwork (still other), getairportnetwork (joined)
    let runner = FakeRunner::scripted([...]);
    let wifi = Wifi::new(runner.clone()).with_sleep(|_| {});
    wifi.join("DIRECT-abc1:ZV-E10", Some("12345678")).unwrap();
    let calls = runner.calls();
    assert!(calls.iter().any(|c| c == &["networksetup", "-setairportnetwork", "en0", "DIRECT-abc1:ZV-E10", "12345678"]));
}

#[cfg(target_os = "macos")]
#[test]
fn join_without_password_omits_the_argument_and_already_joined_runs_nothing() { .. }

#[cfg(target_os = "macos")]
#[test]
fn join_reports_networksetup_failure_text() {
    // setairportnetwork prints "Failed to join network DIRECT-abc1:ZV-E10." with success=true (networksetup exits 0)
    let err = wifi.join("DIRECT-abc1:ZV-E10", Some("x")).unwrap_err();
    assert!(matches!(err, WifiError::Failed(ref t) if t.contains("Failed to join")));
}

#[cfg(target_os = "macos")]
#[test]
fn no_wifi_port_is_a_plain_error() {
    assert!(matches!(wifi.join("x", None), Err(WifiError::NoWifi)));
    assert_eq!(WifiError::NoWifi.to_string(), "This computer has no Wi-Fi");
}

#[test]
fn fake_mode_joins_at_once_and_remembers_the_name() {
    let wifi = Wifi::fake();   // what from_env builds under RIMIX_WIFI=fake
    wifi.join("DIRECT-abc1:ZV-E10", None).unwrap();
    assert_eq!(wifi.current_network().unwrap().as_deref(), Some("DIRECT-abc1:ZV-E10"));
}
```

And in `state.rs`:

```rust
#[tokio::test]
async fn wait_for_camera_honours_the_dev_override_and_times_out_otherwise() {
    // Use a per-test env guard; tokio tests run in parallel so set and clear under a static Mutex.
    std::env::set_var("RIMIX_CAMERA_ADDR", "127.0.0.1:15740");
    let cam = wait_for_camera(Duration::from_millis(10)).await.unwrap();
    assert_eq!(cam.ip, "127.0.0.1"); assert_eq!(cam.ptp_port, 15740);
    std::env::remove_var("RIMIX_CAMERA_ADDR");
    let err = wait_for_camera(Duration::from_millis(1)).await.unwrap_err();
    assert_eq!(err, NO_CAMERA);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p rimix-desktop wifi`
Expected: compile errors.

- [ ] **Step 3: Implement `wifi.rs`, the three `state.rs` functions, the commands, registration in `lib.rs` and the `ipc.ts` wrappers**

- [ ] **Step 4: Run the tests and lint**

Run: `cargo test -p rimix-desktop && cargo clippy -p rimix-desktop --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git commit -am "Join the camera's Wi-Fi and wait for it to answer"
```

---

### Task 3: Reading Wi-Fi credentials out of QR text

**Files:**
- Create: `apps/desktop/src/qr/payload.ts`, `apps/desktop/src/qr/payload.test.ts`

**Interfaces:**
- Produces: `export interface WifiCredentials { ssid: string; password: string; model?: string }`; `export function parseWifiQr(text: string): WifiCredentials | null`.
- Grammar: the text is a prefix (`WIFI:`, `W01:`, or none) followed by `KEY:VALUE;` fields; a value runs to the next `;` that is not escaped; escapes are `\;`, `\:`, `\\`, `\"`; keys are case-insensitive; `S` is the name, `P` the password, `C` the model (also accept `T:nopass` meaning empty password). A result needs a non-empty `S`. Trim surrounding whitespace. Anything else is `null`.

- [ ] **Step 1: Write the failing tests**

```ts
it('reads the standard Wi-Fi grammar, including a colon in the name', () => {
  expect(parseWifiQr('WIFI:S:DIRECT-abc1:ZV-E10;T:WPA;P:12345678;;'))
    .toEqual({ ssid: 'DIRECT-abc1:ZV-E10', password: '12345678' });
});
it('reads the Sony-style grammar with a model', () => {
  expect(parseWifiQr('W01:S:DIRECT-abc1:ZV-E10;P:12345678;C:ZV-E10;M:aabbccddeeff;'))
    .toEqual({ ssid: 'DIRECT-abc1:ZV-E10', password: '12345678', model: 'ZV-E10' });
});
it('unescapes the special characters and accepts an open network', () => {
  expect(parseWifiQr('WIFI:S:Caf\\;e\\:1;T:nopass;;')).toEqual({ ssid: 'Caf;e:1', password: '' });
  expect(parseWifiQr('S:a;P:p\\\\q;')).toEqual({ ssid: 'a', password: 'p\\q' });
});
it('rejects text that is not credentials', () => {
  for (const t of ['https://www.sony.net/app', '', 'hello world', 'WIFI:T:WPA;;', 'P:onlypassword;'])
    expect(parseWifiQr(t)).toBeNull();
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm -C apps/desktop test qr/payload`
Expected: FAIL, module not found.

- [ ] **Step 3: Implement `parseWifiQr`**

- [ ] **Step 4: Run to verify pass**

Run: `pnpm -C apps/desktop test qr/payload`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/qr
git commit -m "Read the camera's Wi-Fi name and password from QR text"
```

---

### Task 4: Scanning QR codes with the webcam

**Files:**
- Create: `apps/desktop/src/qr/scanner.ts`, `apps/desktop/src/qr/scanner.test.ts`
- Create: `apps/desktop/src-tauri/Info.plist`
- Modify: `apps/desktop/package.json` (add `jsqr` ^1.4.0; devDependency `qrcode` ^1.5.4 and `@types/qrcode`)
- Modify: `apps/desktop/src/copy.ts` (webcam error copy)

**Interfaces:**
- Produces (`scanner.ts`):
  - `export interface Webcam { id: string; label: string }`
  - `export function webcamSupported(nav: Pick<Navigator, 'mediaDevices'> = navigator): boolean` (true when `mediaDevices?.getUserMedia` is a function).
  - `export async function listWebcams(nav = navigator): Promise<Webcam[]>` (video inputs from `enumerateDevices`; label falls back to `Camera 1`, `Camera 2`).
  - `export function decodeFrame(data: Uint8ClampedArray, width: number, height: number): string | null` (jsQR with `inversionAttempts: 'dontInvert'`; returns `result.data` or null).
  - `export interface ScannerDeps { getUserMedia(c: MediaStreamConstraints): Promise<MediaStream>; decode: typeof decodeFrame; now(): number }`
  - `export interface Scanner { start(video: HTMLVideoElement, canvas: HTMLCanvasElement): Promise<void>; stop(): void }`
  - `export function createScanner(opts: { deviceId?: string | null; intervalMs?: number; onResult(text: string): void }, deps?: Partial<ScannerDeps>): Scanner`. `start` requests `{ video: { deviceId: { exact } | undefined, facingMode: 'user', width: { ideal: 1280 } } }`, sets `video.srcObject`, plays, then every `intervalMs` (default 125) draws the frame to the canvas at most 640 px wide and calls `decode`; the first non-null result calls `onResult` once and stops sampling (the preview keeps running until `stop`). `stop` ends every track and clears the timer. Errors from `getUserMedia` reject `start` unchanged.
- Produces (`copy.ts`): `export function webcamErrorMessage(e: unknown): string`: `NotAllowedError` → `"Rimix needs permission to use the webcam. Allow it in System Settings, Privacy & Security, Camera, then try again."`; `NotFoundError`/`OverconstrainedError` → `"No webcam found. Paste the code's text or type the name and password instead."`; anything else → `"The webcam isn't available here. Paste the code's text or type the name and password instead."`
- `Info.plist` (XML plist dict) with `NSCameraUsageDescription` = `Rimix uses the webcam to scan the QR code on your camera's screen.`

- [ ] **Step 1: Install the packages**

Run: `pnpm -C apps/desktop add jsqr@^1.4.0 && pnpm -C apps/desktop add -D qrcode@^1.5.4 @types/qrcode`

- [ ] **Step 2: Write the failing tests**

```ts
// Rasterises a QR module matrix from `qrcode` into RGBA at `scale` px per module with a 4-module quiet zone.
function raster(text: string, scale = 4): { data: Uint8ClampedArray; width: number; height: number } { /* QRCode.create(text).modules */ }

it('decodes a QR code from pixels', () => {
  const img = raster('W01:S:DIRECT-abc1:ZV-E10;P:12345678;C:ZV-E10;');
  expect(decodeFrame(img.data, img.width, img.height)).toBe('W01:S:DIRECT-abc1:ZV-E10;P:12345678;C:ZV-E10;');
});
it('returns null for a blank frame', () => {
  const blank = new Uint8ClampedArray(64 * 64 * 4).fill(255);
  expect(decodeFrame(blank, 64, 64)).toBeNull();
});
it('knows when the webcam API is missing', () => {
  expect(webcamSupported({ mediaDevices: undefined } as unknown as Navigator)).toBe(false);
  expect(webcamSupported({ mediaDevices: { getUserMedia: async () => ({}) } } as unknown as Navigator)).toBe(true);
});
it('reports the first decode once and stops sampling', async () => {
  // fake getUserMedia resolving a stream with one track { stop: vi.fn() }, fake decode returning null twice then text, vi.useFakeTimers()
  // after start + 3 intervals: onResult called once with the text; after 3 more intervals still once; stop() stops the track.
});
it('words webcam errors for the user', () => {
  expect(webcamErrorMessage(Object.assign(new Error('x'), { name: 'NotAllowedError' }))).toMatch(/Privacy & Security/);
  expect(webcamErrorMessage(Object.assign(new Error('x'), { name: 'NotFoundError' }))).toMatch(/No webcam found/);
  expect(webcamErrorMessage(new Error('weird'))).toMatch(/isn't available here/);
});
```

jsdom has no canvas: the sampling test injects `decode` and stubs `canvas.getContext` to return an object with `drawImage` and `getImageData` returning a fixed `ImageData`-like value.

- [ ] **Step 3: Run to verify failure**

Run: `pnpm -C apps/desktop test qr/scanner`
Expected: FAIL.

- [ ] **Step 4: Implement `scanner.ts`, the copy, and `Info.plist`**

- [ ] **Step 5: Run tests and types**

Run: `pnpm -C apps/desktop test && pnpm -C apps/desktop exec tsc --noEmit`
Expected: PASS.

- [ ] **Step 6: Check the dev build asks for the camera**

Run: `pnpm -C apps/desktop tauri dev` for a few seconds (needs a display). In the webview devtools console, `typeof navigator.mediaDevices?.getUserMedia` must print `function`. Note the result in the report; the production-origin check is Task 10.

- [ ] **Step 7: Commit**

```bash
git add -A apps/desktop
git commit -m "Scan QR codes with the webcam"
```

---

### Task 5: The connect screen: scan, nearby, or type the name and password

**Files:**
- Modify: `apps/desktop/src/components/connectScreen.ts` (rewrite), `apps/desktop/src/actions.ts`, `apps/desktop/src/state.ts`, `apps/desktop/src/copy.ts`, `apps/desktop/src/styles/layout.css`
- Modify: `apps/desktop/src-tauri/src/state.rs` (`connect` takes the ssid to remember)
- Modify: `apps/desktop/src/components/sidePanels.ts` (Cameras panel rows use `connectRecent`)
- Test: `apps/desktop/src/actions.test.ts`, `apps/desktop/src/copy.test.ts`, new `apps/desktop/src/components/connectScreen.test.ts`

**Interfaces:**
- Consumes: Task 2 commands, Task 3 `parseWifiQr`, Task 4 scanner and copy, Task 1 settings fields.
- Rust: `connect(state, ip, ssid: Option<&str>)` and the command `connect(ip: String, ssid: Option<String>)`; `ipc.connect(ip, ssid: string | null = null)`.
- `state.ts`: `connecting: { phase: 'joining' | 'finding' | 'connecting'; text: string } | null` in `AppState` (null initially).
- `actions.ts`:
  - `connect(ip: string, ssid?: string | null): Promise<boolean>` (as before, plus passing the ssid on).
  - `connectWithWifi(creds: WifiCredentials): Promise<boolean>`: when `settings.wifiJoin === 'Automatic'`: set `connecting` to joining with `copy.joiningText(ssid)`, call `joinNetwork(ssid, password || null)`; then finding with `copy.findingText(ssid)`, `waitForCamera(FIND_MS)`; then connecting, `connect(camera.ip, ssid)`. When `'Ask'`: skip the join, set finding with `copy.askToJoinText(ssid)` and wait `ASK_FIND_MS`. Any failure: `connecting: null`, `error` from `copy.wifiErrorMessage(step, ssid, detail)`, return false. `export const FIND_MS = 30_000; export const ASK_FIND_MS = 90_000;`
  - `connectRecent(rc: RecentCamera): Promise<boolean>`: with `rc.ssid` → `connectWithWifi({ ssid, password: '' })` (the OS has the password); without → `connect(rc.ip)`.
  - `forgetCamera(rc: RecentCamera): Promise<void>`: saves settings without that entry (filter by ip) and clears `lastIp` if it matched.
- `copy.ts`:
  - `joiningText(ssid)` → `Joining ${ssid}`; `findingText(ssid)` → `Looking for the camera on ${ssid}`; `askToJoinText(ssid)` → `Join ${ssid} from the Wi-Fi menu. Rimix connects as soon as the camera appears.`; `connectingText(name)` → `Connecting to ${name}. If the camera asks, press OK on it.`
  - `wifiErrorMessage(step: 'join' | 'find', ssid: string, detail: string): string`: join → `Could not join ${ssid}: ${detail}. Join it from the Wi-Fi menu, then try again.` except when `detail` is `This computer has no Wi-Fi` → `This computer has no Wi-Fi. Put the camera and this computer on the same router and use Nearby.`; find → `Joined ${ssid} but no camera answered. On the camera, open Menu, Network, PC Remote Function, and stay on that screen.`
  - `NOT_A_WIFI_CODE = "This code isn't a camera Wi-Fi code. Check the text below, or type the name and password."`
- Connect screen layout (same `.ways` grid, same glass cards, in this order):
  1. **Scan the QR code**: `.preview` holds a `<video muted playsinline>` and a hidden `<canvas>`; a status line under it; hint `On the camera: Menu, Network, Send to Smartphone Func., Send to Smartphone.`; a text button `Paste the code's text` that reveals a `<textarea>` and a `Use this text` button. Scanning starts when the screen mounts (if `webcamSupported()`, with `settings.webcamId`), restarts 3 s after a failed connect, stops on unmount. A decoded text goes through `parseWifiQr`: credentials → `connectWithWifi`; null → `NOT_A_WIFI_CODE` in the message, the text shown in the textarea, and the name field focused.
  2. **Nearby**: unchanged.
  3. **Type the Wi-Fi name and password**: two inputs (`id="ssid"` placeholder `DIRECT-xxxx:ZV-E10`, `id="pw"` type password with a `Show` toggle), a `Connect` button, hint `Shown on the camera under Menu, Network, PC Remote Function, Wi-Fi Direct Info.` Below, a disclosure button `Advanced: connect by address` (aria-expanded) revealing the Phase 1 IP field prefilled `192.168.122.1` with its own Connect.
  - The message line shows `connecting.text` while connecting (not an error), errors in red; all Connect buttons disabled while `connecting` is set.
  - Recently used rows: key = model, value = `rc.ssid ?? rc.ip`, click → `connectRecent(rc)`.
- `sidePanels.ts` Cameras panel: recent rows show `rc.ssid ?? rc.ip` and call `connectRecent(rc)`.

- [ ] **Step 1: Write the failing tests**

`actions.test.ts` (mock `./ipc`):

```ts
it('connectWithWifi joins, finds and connects, remembering the network', async () => {
  // settings.wifiJoin = 'Automatic'; ipc.joinNetwork resolves; waitForCamera resolves { ip: '192.168.122.1', model: 'ZV-E10', ... }; connect resolves a CameraState
  const phases: string[] = [];
  store.subscribe((s) => { if (s.connecting) phases.push(s.connecting.phase); });
  expect(await actions.connectWithWifi({ ssid: 'DIRECT-abc1:ZV-E10', password: '12345678' })).toBe(true);
  expect(phases).toEqual(['joining', 'finding', 'connecting']);
  expect(ipc.joinNetwork).toHaveBeenCalledWith('DIRECT-abc1:ZV-E10', '12345678');
  expect(ipc.connect).toHaveBeenCalledWith('192.168.122.1', 'DIRECT-abc1:ZV-E10');
  expect(store.get().connecting).toBeNull();
  expect(store.get().screen).toBe('main');
});
it('connectWithWifi explains when no camera answers and frees the screen', async () => {
  // waitForCamera rejects 'No camera answered on this network'
  expect(await actions.connectWithWifi({ ssid: 'DIRECT-abc1:ZV-E10', password: 'x' })).toBe(false);
  expect(store.get().connecting).toBeNull();
  expect(store.get().error).toMatch(/no camera answered.*PC Remote Function/);
  expect(ipc.connect).not.toHaveBeenCalled();
});
it('Ask mode skips joining and waits longer', async () => { /* wifiJoin 'Ask': joinNetwork not called; waitForCamera called with ASK_FIND_MS; connecting.text mentions the Wi-Fi menu */ });
it('connectRecent uses the network when known and the address otherwise', async () => { /* ssid → joinNetwork(ssid, null); no ssid → ipc.connect(ip, null) */ });
it('forgetCamera drops the entry and clears lastIp', async () => { .. });
```

`copy.test.ts`: the five copy functions with the exact strings above, including the no-Wi-Fi special case.

`connectScreen.test.ts` (mount into jsdom with a store and fake actions; stub `webcamSupported` false so no stream is requested):

```ts
it('shows three ways with the address field behind Advanced', () => {
  expect([...root.querySelectorAll('.way h2')].map((h) => h.textContent))
    .toEqual(['Scan the QR code', 'Nearby', 'Type the Wi-Fi name and password']);
  const adv = root.querySelector<HTMLButtonElement>('button[aria-expanded]')!;
  expect(root.querySelector('#ip')!.closest('[hidden]')).not.toBeNull();
  adv.click();
  expect(root.querySelector('#ip')!.closest('[hidden]')).toBeNull();
});
it('typing a name and password connects over Wi-Fi', () => { /* fill #ssid/#pw, submit → actions.connectWithWifi({ssid, password}) */ });
it('pasted text that is not a Wi-Fi code is explained and left for editing', () => {
  /* click "Paste the code's text", set textarea to 'https://sony.net', click Use this text →
     message has NOT_A_WIFI_CODE, textarea still shows the URL, #ssid is document.activeElement, connectWithWifi not called */
});
it('pasted Sony text connects', () => { /* 'W01:S:DIRECT-abc1:ZV-E10;P:12345678;C:ZV-E10;' → connectWithWifi called with ssid/password */ });
it('disables Connect buttons and shows progress while connecting', () => { /* store.set({ connecting: { phase: 'joining', text: 'Joining X' } }) → every .pbtn disabled, message text 'Joining X', not .error */ });
it('recent cameras show the network and connect through it', () => { /* settings.recentCameras = [{ ip, model, ssid, lastUsed }] → row value is the ssid; click → actions.connectRecent(rc) */ });
```

Rust `state.rs` test update: `connect_returns_state_and_remembers_the_camera` also passes `Some("DIRECT-abc1:ZV-E10")` and asserts `saved.recent_cameras[0].ssid`.

- [ ] **Step 2: Run to verify failure**

Run: `pnpm -C apps/desktop test` and `cargo test -p rimix-desktop`
Expected: FAIL on the new tests.

- [ ] **Step 3: Implement the actions, copy, state field, Rust `connect` change, the connect screen and its styles (the video fills `.preview` with `object-fit: cover`; the textarea and the Advanced block use `.field` styling; the message line already exists)**

- [ ] **Step 4: Run everything**

Run: `pnpm -C apps/desktop test && pnpm -C apps/desktop exec tsc --noEmit && cargo test -p rimix-desktop`
Expected: PASS.

- [ ] **Step 5: Try the whole flow against the simulator**

Run in one terminal: `cargo run -p sony-ptpip-sim -- --port 15740`. In another: `RIMIX_WIFI=fake RIMIX_CAMERA_ADDR=127.0.0.1:15740 pnpm -C apps/desktop tauri dev`. On the connect screen, click `Paste the code's text`, paste `W01:S:DIRECT-abc1:ZV-E10;P:12345678;C:ZV-E10;`, click `Use this text`. Expected: the message steps through Joining, Looking for the camera, Connecting, and the main screen appears; the Cameras panel lists `ZV-E10` with `DIRECT-abc1:ZV-E10`. Disconnect; the Recently used row connects again with one click.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "Connect by scanning or typing the camera's Wi-Fi details"
```

---

### Task 6: App settings: Wi-Fi join, webcam and remembered cameras

**Files:**
- Modify: `apps/desktop/src/components/appSettings.ts`, `apps/desktop/src/styles/layout.css`
- Test: `apps/desktop/src/components/appSettings.test.ts`

**Interfaces:**
- Consumes: `listWebcams` (Task 4), `forgetCamera` (Task 5), settings fields (Task 1).
- A new section `Connection` after `Layout`: a segmented row `Wi-Fi join` with `Automatic` and `Ask me` (maps to `wifiJoin: 'Automatic' | 'Ask'`); a row `Webcam` with a `<select>` (first option `Default` = `webcamId: null`, then each webcam; populated from `listWebcams()` when the popover opens, `Default` only when the API is missing); then a `Remembered cameras` group listing `recentCameras` as rows with key = model, value = `ssid ?? ip`, and a `Forget` button (`aria-label` `Forget <model> on <ssid or ip>`); an empty list shows the quiet text `None yet`.

- [ ] **Step 1: Write the failing tests** (mock `../qr/scanner` `listWebcams` to resolve two cameras):

```ts
it('offers Automatic and Ask me for Wi-Fi join and saves the choice', async () => { /* click 'Ask me' → actions.saveSettings called with wifiJoin 'Ask' */ });
it('lists webcams with Default first and saves the chosen one', async () => { /* open popover, await; select has 3 options; change to second → saveSettings webcamId = its id */ });
it('lists remembered cameras with a Forget button', () => { /* settings.recentCameras of 2 → two rows; click Forget on the first → actions.forgetCamera(rc) */ });
it('says None yet when nothing is remembered', () => { .. });
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm -C apps/desktop test appSettings`
Expected: FAIL.

- [ ] **Step 3: Implement the section; style the `<select>` like `.field input` and the Forget button like `.pathbtn`**

- [ ] **Step 4: Run to verify pass**

Run: `pnpm -C apps/desktop test && pnpm -C apps/desktop exec tsc --noEmit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git commit -am "Choose how Rimix joins Wi-Fi and which webcam it uses"
```

---

### Task 7: Presets

**Files:**
- Create: `apps/desktop/src/presets.ts`, `apps/desktop/src/presets.test.ts`, `apps/desktop/src/components/presetsControl.ts`, `apps/desktop/src/components/presetsControl.test.ts`
- Modify: `apps/desktop/src-tauri/src/state.rs`, `commands.rs`, `lib.rs` (`apply_values`), `apps/desktop/src/ipc.ts`, `actions.ts`, `state.ts`, `components/dock.ts`, `components/ring.ts`, `styles/layout.css`
- Test: `state.rs` tests, `dock.test.ts`, `ring.test.ts`

**Interfaces:**
- Rust: `pub struct ApplyOutcome { pub code: u16, pub error: Option<String> }`; `pub async fn apply_values(state: &AppState, values: Vec<PresetValue>) -> Result<Vec<ApplyOutcome>, String>`: `NOT_CONNECTED` when no camera; otherwise `camera.set(code, raw)` for each value in order, collecting each error's Display text, never stopping early. Command `apply_values(values: Vec<PresetValue>) -> Result<Vec<ApplyOutcome>, String>`; `ipc.applyValues(values): Promise<ApplyOutcome[]>`.
- `presets.ts`:
  - `export const PRESET_SLOTS = 4;`
  - `export function presetFor(settings: AppSettings | null, model: string | undefined, slot: number): Preset | undefined`
  - `export function captureValues(camera: CameraState): PresetValue[]`: for every code in `CHIP_CODES`, plus `PROP.colourTemp` when `isColourTemp(camera)`, where `isUsable(prop)`: `{ code, raw: prop.raw }`.
  - `export function newPreset(name: string, model: string, slot: number, values: PresetValue[]): Preset` (id from `crypto.randomUUID()` when available, else `String(Date.now())`).
  - `export function upsertPreset(presets: Preset[], preset: Preset): Preset[]` (replaces the one with the same model and slot, or the same id).
  - `export function describeFailures(outcomes: ApplyOutcome[]): string | null`: null when none failed; else `Could not set ${names.join(', ')}` with names from `SETTING_NAMES` (unknown codes as hex).
- `state.ts`: `presetBusy: number | null` (slot) and `activePreset: number | null` (slot last applied without failures).
- `actions.ts`: `applyPreset(preset: Preset): Promise<void>` (sets `presetBusy`, calls `applyValues`, toasts `describeFailures` when not null, sets `activePreset` on full success, clears `presetBusy` in `finally`); `savePreset(preset: Preset): Promise<void>` (`upsertPreset` into settings and `saveSettings`); `deletePreset(id: string): Promise<void>`.
- `presetsControl.ts`: `export function presetsControl(store: AppStore, actions: Actions): { el: HTMLElement; destroy(): void }` renders the `.segc` radiogroup with four buttons `C1`..`C4`. A filled slot shows its name, click applies, `aria-checked` = active, `aria-busy` + class `busy` while `presetBusy === slot`. An empty slot shows `Empty`; click opens a small inline glass form (`.preset-form`) above the control: label `Save current settings as C2`, a name input (default `Preset 2`), `Save` and `Cancel`; Save → `savePreset(newPreset(name, camera.model, slot, captureValues(camera)))`. Each filled slot has a `…` button (`aria-label` `Options for C1`) opening a menu with `Rename`, `Replace with current settings`, `Delete`. All controls disabled when `camera` is null or the camera has no usable values. Replaces `presets()` in `dock.ts`; `ring.ts` imports it instead of `presets` from dock.

- [ ] **Step 1: Write the failing tests**

`state.rs`:

```rust
#[tokio::test]
async fn apply_values_writes_in_order_and_reports_each_failure() {
    // connected sim; values: ISO U32 1600 (ok), an aperture value that is not in the settable list (fails), shutter (ok)
    let out = apply_values(&state, values).await.unwrap();
    assert_eq!(out.len(), 3);
    assert!(out[0].error.is_none()); assert!(out[1].error.is_some()); assert!(out[2].error.is_none());
    assert_eq!(short_of(&get_state(&state).await.unwrap(), PROP_ISO), "1600");
}
#[tokio::test]
async fn apply_values_needs_a_camera() { assert_eq!(apply_values(&state, vec![]).await.unwrap_err(), NOT_CONNECTED); }
```

`presets.test.ts`:

```ts
it('captures only usable chip values and the colour temperature when active', () => { /* camera-state.json fixture; a disabled prop is left out */ });
it('finds a preset by model and slot only', () => { expect(presetFor(settings, 'ILCE-7M4', 1)).toBeUndefined(); expect(presetFor(settings, 'ZV-E10', 1)?.name).toBe('Talking head'); });
it('upsert replaces the same slot for the same model', () => { .. });
it('describes failures by setting name', () => {
  expect(describeFailures([{ code: 0x5007, error: 'nope' }, { code: 0xd21e, error: null }, { code: 0x500e, error: 'x' }])).toBe('Could not set Aperture, Mode');
  expect(describeFailures([{ code: 0x5007, error: null }])).toBeNull();
});
```

`actions.test.ts`: `applyPreset` sets busy then clears it, toasts `Could not set Aperture` when one outcome fails, sets `activePreset` only on full success; `savePreset` and `deletePreset` call `saveSettings` with the right list.

`presetsControl.test.ts`: four buttons; empty slot click shows the form with `Save current settings as C2`; Save calls `actions.savePreset` with model `ZV-E10`, slot 2 and the captured values; filled slot click calls `applyPreset`; `presetBusy: 1` gives `aria-busy="true"` on C1; `…` menu lists `Rename`, `Replace with current settings`, `Delete`; nothing clickable when `camera` is null.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p rimix-desktop apply_values` and `pnpm -C apps/desktop test presets`
Expected: FAIL.

- [ ] **Step 3: Implement the Rust command, the helpers, the control, wire it into dock and ring, and update the dock and ring tests that referenced the inert presets**

- [ ] **Step 4: Run everything**

Run: `cargo test -p rimix-desktop && pnpm -C apps/desktop test && pnpm -C apps/desktop exec tsc --noEmit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Save and apply camera presets from the dock"
```

---

### Task 8: Arrow keys adjust the open setting

**Files:**
- Modify: `apps/desktop/src/components/settingsPopover.ts` (expose the open code), `components/dock.ts`, `components/ring.ts`
- Test: `apps/desktop/src/components/dock.test.ts`, `ring.test.ts`

**Interfaces:**
- `SettingsPopover` gains `activeCode(): number | null` (the code of the row highlighted when opened from a chip or segment; null when closed or opened without a code).
- In the dock and the ring, a `keydown` listener on `document`: when the popover is open with a code, not typing, no modifier: `ArrowRight` or `ArrowUp` → `actions.stepProperty(code, 1)`; `ArrowLeft` or `ArrowDown` → `stepProperty(code, -1)`; `preventDefault`. Key repeat is allowed (holding steps repeatedly). When the property is not usable (`isUsable` false), do nothing.

- [ ] **Step 1: Write the failing tests**

```ts
it('arrow keys step the open chip and nothing when closed', () => {
  // mount dock with a camera; press ArrowRight → stepProperty not called; open the Shutter chip; ArrowRight → stepProperty(PROP.shutter, 1); ArrowLeft → (PROP.shutter, -1); Escape closes; ArrowRight → still 2 calls
});
it('arrow keys do nothing while typing', () => { /* focus an <input> appended to the body, dispatch ArrowRight → no call */ });
```

Same shape in `ring.test.ts` for a highlighted segment.

- [ ] **Step 2: Run to verify failure**

Run: `pnpm -C apps/desktop test dock ring`
Expected: FAIL.

- [ ] **Step 3: Implement**

- [ ] **Step 4: Run to verify pass**

Run: `pnpm -C apps/desktop test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git commit -am "Adjust the open setting with the arrow keys"
```

---

### Task 9: Offline font, thumbnail folder check, live view backoff

**Files:**
- Modify: `apps/desktop/package.json` (add `@fontsource-variable/inter`), `apps/desktop/src/main.ts`, `apps/desktop/index.html` (remove the three Google Fonts tags), `apps/desktop/src/styles/tokens.css` (`--font: "Inter Variable", "Inter", ...`), `apps/desktop/src-tauri/tauri.conf.json` (CSP: drop `https://fonts.googleapis.com` and `https://fonts.gstatic.com`; `font-src 'self' data:`)
- Modify: `apps/desktop/src-tauri/src/state.rs` (`read_thumbnail`), `commands.rs`
- Modify: `crates/sony-ptpip/src/camera/loops.rs`
- Test: `state.rs` tests, `crates/sony-ptpip/tests/camera.rs`

**Interfaces:**
- `pub fn read_thumbnail(path: &Path, allowed: &[PathBuf]) -> Result<Vec<u8>, String>`: besides the Phase 1 checks, the canonicalised `path` must start with the canonicalised form of one of `allowed`, else `Err(NOT_A_CAPTURE)` where `pub const NOT_A_CAPTURE: &str = "Only files Rimix saved can be shown";`. The command passes `[settings.destination_base, sony_ptpip::capture::default_recovery_dir()]`.
- `loops.rs`: `pub(super) fn live_view_wait(consecutive_failures: u32) -> Duration`: `NO_FRAME_WAIT` (15 ms) for 0 to 4 failures, 1 s for 5 to 9, 3 s from 10 on. The live view loop counts consecutive `Err` (non-lost) and `Ok(None)` results, resets on a frame, and sleeps `live_view_wait(count)`.

- [ ] **Step 1: Write the failing tests**

```rust
// state.rs
#[test]
fn read_thumbnail_refuses_files_outside_the_capture_folders() {
    // a JPEG in tmp/shots (allowed) is returned; the same bytes in tmp/elsewhere → Err(NOT_A_CAPTURE)
}
// loops.rs
#[test]
fn live_view_wait_backs_off_after_repeated_failures() {
    assert_eq!(live_view_wait(0), NO_FRAME_WAIT);
    assert_eq!(live_view_wait(4), NO_FRAME_WAIT);
    assert_eq!(live_view_wait(5), Duration::from_secs(1));
    assert_eq!(live_view_wait(10), Duration::from_secs(3));
    assert_eq!(live_view_wait(500), Duration::from_secs(3));
}
```

And in `crates/sony-ptpip/tests/camera.rs`: with `SimConfig { live_view_info_denied: 12, .. }` the camera connects and a frame still arrives within 20 s (the backoff slows polling but never stops it).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p rimix-desktop read_thumbnail && cargo test -p sony-ptpip live_view`
Expected: FAIL.

- [ ] **Step 3: Implement; install the font (`pnpm -C apps/desktop add @fontsource-variable/inter`), import `@fontsource-variable/inter` in `main.ts`, change the token and the CSP**

- [ ] **Step 4: Run everything, then build the front end and check no request leaves the machine**

Run: `cargo test --workspace && pnpm -C apps/desktop test && pnpm -C apps/desktop build && grep -rl "fonts.googleapis" apps/desktop/dist apps/desktop/index.html apps/desktop/src-tauri/tauri.conf.json`
Expected: tests PASS; the grep prints nothing; `apps/desktop/dist/assets` contains a `.woff2` file.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Work offline with a bundled font and safer thumbnails"
```

---

### Task 10: Release workflow, install instructions and the owner's guide

**Files:**
- Create: `.github/workflows/release.yml`, `apps/desktop/src-tauri/Entitlements.plist`
- Modify: `apps/desktop/src-tauri/tauri.conf.json` (`bundle.macOS.minimumSystemVersion: "14.0"`, `bundle.macOS.entitlements: "Entitlements.plist"`), `apps/desktop/src-tauri/tauri.macos.conf.json` (keep `dmg` and `app`), `README.md`, `docs/camera-testing.md`

**Interfaces:**
- `release.yml`: on `push` of tags `v*`; job on `macos-latest`; steps: checkout, `pnpm/action-setup`, Node 22, `dtolnay/rust-toolchain@stable` with targets `aarch64-apple-darwin, x86_64-apple-darwin`, `pnpm -C apps/desktop install --frozen-lockfile`, then `tauri-apps/tauri-action@v0` with `projectPath: apps/desktop`, `args: --target universal-apple-darwin`, `tagName: ${{ github.ref_name }}`, `releaseName: Rimix ${{ github.ref_name }}`, `releaseDraft: true`, `releaseBody` pointing at the README install section; `env` passes `GITHUB_TOKEN` and the six Apple variables from secrets (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`). Signing and notarization happen only when those secrets exist; without them the DMG is unsigned and the release notes say so.
- `Entitlements.plist`: `com.apple.security.device.camera` true, `com.apple.security.network.client` true, `com.apple.security.network.server` true (SSDP replies).
- README: a new `Install on a Mac` section (download the DMG from Releases; for an unsigned build: right-click, Open, or `xattr -d com.apple.quarantine /Applications/Rimix.app`), `Connect your camera` (the three ways, in the owner's words: scan, nearby, type the name and password), `Release a version` (tag `vX.Y.Z`, push, draft release appears; the Apple secrets to add for a signed build), and `Develop without a camera` (`RIMIX_WIFI=fake RIMIX_CAMERA_ADDR=127.0.0.1:15740`).
- `docs/camera-testing.md` step 2: scan the QR with Rimix first (what the message says), then with the phone, and paste the phone's text into `Paste the code's text`; what to send back now includes whether Rimix joined the network by itself and what the status line said.

- [ ] **Step 1: Write the files**

- [ ] **Step 2: Check the workflow parses and the bundle builds**

Run: `pnpm -C apps/desktop exec tauri build` (several minutes; needs about 3 GiB of disk). Expected: `apps/desktop/src-tauri/target/release/bundle/dmg/Rimix_0.1.0_aarch64.dmg` (or under the workspace `target/`) exists; `du -sh` of the `.app` is under 30 MB.

- [ ] **Step 3: Check the production origin can use the webcam**

Run: `open <path to Rimix.app>`; on the connect screen the Scan the QR code card must either show the webcam preview (after the permission prompt) or the fallback message from Task 4. Record which in the report. If it is the fallback, add to the README's known issues: "In the packaged app the webcam preview is not available yet; paste the code's text or type the name and password." (The flow still works through those two paths.)

- [ ] **Step 4: Validate the YAML**

Run: `node -e "require('js-yaml')"` is not available; instead `ruby -ryaml -e 'YAML.load_file(".github/workflows/release.yml"); puts "ok"'`
Expected: `ok`.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Build a Mac release from a tag and explain how to install"
```

---

## Self-review notes

- Spec coverage for Phase 2 (section 7): QR scan (Tasks 3 to 5), Wi-Fi Direct auto-join with manual fallback (Task 2, Task 5 `Ask` mode and error copy), remembered cameras (Tasks 1, 5, 6), presets (Task 7), light and system themes, ring layout, App settings and keyboard shortcuts (done in Phase 1; arrows in Task 8), DMG with notarization (Task 10, gated on the owner's Apple secrets), GitHub release (Task 10). Section 6.5 App settings items: webcam and Wi-Fi join behaviour and remembered cameras (Task 6). Section 6.6: three ways, the pairing note, recently used, worded errors (Task 5).
- Deferred again, on purpose: the "connect without pairing" handshake variant (section 5). The variant's exact wire difference is not documented in anything we may read, so it waits for the camera test rather than being guessed. Bluetooth stays in Phase 4. Windows Wi-Fi joining returns `Unsupported` until the Windows build in Phase 3.
- Type consistency: `WifiCredentials` (Task 3) is what Task 5 passes to `connectWithWifi`; `PresetValue` is the same shape in Rust and TypeScript (`code`, `raw`); `RecentCamera.ssid` is `Option<String>` / `string | null`; `DiscoveredCamera` is the Phase 1 type.
- Proportion: the spec's Phase 2 paragraph is one line; this plan is long because the onboarding flow has three OS boundaries (webcam, Wi-Fi, camera). Code blocks are tests and signatures only.
