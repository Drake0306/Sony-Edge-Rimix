# Rimix Phase 4 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the spec's last phase: video scopes drawn from the live view (waveform and false colour, next to the existing zebra), several cameras connected at once with a switchable live view and "Shoot all", browsing the camera's card and downloading stills and video clips with resumable transfers, an SFTP receiver so the camera can push files to the laptop on its own, and Bluetooth (nearby cameras, the credential handoff attempt, and a BLE shutter), plus the leftovers parked at the end of Phase 3 and a dev flag that lets the app be smoke-tested without a webcam prompt.

**Architecture:** Scopes are pure TypeScript over the frame pixels the live view already samples. Multi-camera is a refactor of the app's Rust state from one `Option<Camera>` to a keyed set with an active camera; every camera event carries the camera's id; the camera library is untouched. Card browsing adds standard PTP object operations (storage ids, object handles, object info, partial object) to the library and the simulator, with a `.part`-and-resume downloader built on the capture code's atomic-write helpers. The SFTP receiver is a new crate on top of `russh` (server, already a dependency) and `russh-sftp`, writing into the chosen destination through the same atomic rename. Bluetooth is a new crate on top of `btleplug` behind an adapter trait with a fake for tests; the real radio is only exercised by the owner. Everything is tested against the simulator (two instances for multi-camera, an object store for the card, an SFTP client for the receiver) with no hardware.

**Tech Stack:** As before plus `russh-sftp` 3 (Apache-2.0/MIT) and `btleplug` 0.13 (MIT/Apache-2.0/BSD-3), both pure Rust on macOS (CoreBluetooth via objc) and Windows (WinRT).

**Spec:** `docs/superpowers/specs/2026-11-10-rimix-design.md`, sections 2 (goal 5), 5 (Bluetooth paragraph), 6.2 (overlays and the Scopes panel), 6.7 (video retrieval in Phase 4), 7 (Phase 4 list). Owner's direction on 2026-11-11: "complete all the phases, do not wait for me", test against the simulator, leave a build ready for a real test in the morning.

## Global Constraints

- Licence Apache-2.0; new dependencies must be Apache-2.0, MIT or BSD: `russh-sftp` 3, `btleplug` 0.13.
- Commit messages: plain language, under 60 characters, no "Co-Authored-By" or any Claude attribution.
- Platforms: macOS first, Windows via CI; OS-specific code cfg-gated; nothing may break either build. Bluetooth needs `NSBluetoothAlwaysUsageDescription` in `Info.plist` and the `com.apple.security.device.bluetooth` entitlement.
- Never store or log a camera's Wi-Fi or SSH password. The SFTP receiver's own generated password is Rimix's secret (stored in settings so it survives restarts, shown to the user to type into the camera), never logged.
- Never send guessed vendor operations to a real camera. Card browsing uses only standard PTP operations (0x1004, 0x1005, 0x1007, 0x1008, 0x1009, 0x101B). The Bluetooth handoff reads characteristics; it writes only the documented remote-control codes to FF01.
- Front-end copy: sentence case, plain words; spec names: "Waveform", "False colour", "Zebra", "Cameras", "Shoot all", "Browse card", "Receive from camera".
- All colours through CSS custom properties; both themes complete; scopes draw with token colours.
- Settings files from Phases 1 to 3 must still load: every new field has a default.
- Tests run without hardware: two simulator instances, the simulator's object store, an in-process SFTP client, a fake Bluetooth adapter.

## Review Focus

1. Two cameras shooting the same filename (DSC00001.JPG from each): both files must be kept, with the `-N` suffix the capture code already adds, and the filmstrip must show which camera each came from. (Task 2)
2. A card download interrupted mid-file (link drop): the `.part` file must be resumed from its size with `GetPartialObject`, never restarted from zero and never renamed into place short. (Task 3)
3. A file the camera pushes over SFTP while a download to the same name is in progress: no half file is ever visible in the destination, and neither transfer corrupts the other (distinct temp names). (Task 5)
4. Bluetooth unavailable or denied (no adapter, permission refused): the Nearby card says so once and keeps listing SSDP cameras; nothing retries in a loop. (Task 8)
5. Switching the active camera while the interval timer runs: the run keeps shooting the camera it was started on, not the newly active one. (Task 2)

---

## File structure

```
apps/desktop/src/scopes.ts, scopes.test.ts            waveform bins, false-colour LUT (pure)
apps/desktop/src/components/liveview.ts               draw waveform and false colour when switched on
apps/desktop/src-tauri/src/state.rs                   cameras: keyed set + active; events carry camera_id; shoot_all
apps/desktop/src-tauri/src/commands.rs, lib.rs        camera_id on commands; list_cameras, set_active_camera, shoot_all
apps/desktop/src/{ipc,state,actions,app}.ts           cameras map, activeId, per-camera captures
apps/desktop/src/components/{sidePanels,connectScreen,filmstrip,dock,topbar}.ts  switching, Shoot all, Add a camera / Back
crates/sony-ptpip/src/ptp/codes.rs                    storage/object op codes and format codes
crates/sony-ptpip/src/sony/card.rs                    list_storages, list_objects, download (resumable)
crates/sony-ptpip/src/camera.rs                       Camera::card_list, Camera::card_download with progress events
crates/sony-ptpip-sim/src/card.rs                     object store, GetStorageIDs/Info, GetObjectHandles, GetPartialObject
apps/desktop/src-tauri + src/components/cardBrowser.ts  Browse card panel, downloads with progress
crates/rimix-cli                                      card list|get, sftp serve, ble scan|handoff|shoot
crates/rimix-sftp/                                    SftpReceiver: russh server + russh-sftp handler, atomic writes, on_file callback
apps/desktop/src-tauri/src/sftp.rs + settings.rs      receiver lifecycle, settings (enabled, port, user, password, host key path)
apps/desktop/src/components/appSettings.ts            "Receive from camera" section
crates/rimix-ble/                                     BleAdapter trait, SonyAdvertisement parser, handoff, shutter; btleplug impl; FakeAdapter
apps/desktop/src-tauri/src/ble.rs + Info.plist + Entitlements.plist
apps/desktop/src/components/connectScreen.ts          Bluetooth rows in Nearby; Join via Bluetooth
apps/desktop/src-tauri/src/state.rs                   dev flags (RIMIX_WEBCAM=off)
README.md, docs/camera-testing.md                     scopes, multi-camera, card, SFTP, Bluetooth; steps 9 to 11
```

---

### Task 1: Waveform and false colour scopes

**Files:**
- Create: `apps/desktop/src/scopes.ts`, `apps/desktop/src/scopes.test.ts`
- Modify: `apps/desktop/src/components/liveview.ts`, `liveviewParts.ts`, `components/sidePanels.ts` (the two inert switches become live), `state.ts` (`overlays.waveform`, `overlays.falseColour`), `styles/layout.css`, `styles/tokens.css` (scope colours as tokens in both themes)
- Test: `scopes.test.ts`, `liveviewParts.test.ts`, `sidePanels.test.ts`

**Interfaces:**
- `scopes.ts` (pure):
  - `export const WAVEFORM_COLUMNS = 128; export const WAVEFORM_ROWS = 64;`
  - `export function waveform(lum: Uint8ClampedArray | number[], width: number, height: number): Uint16Array` → a `WAVEFORM_COLUMNS × WAVEFORM_ROWS` histogram (column = x bucket, row = luminance bucket 0 bottom to 255 top), counts per cell.
  - `export interface FalseColourBand { upTo: number; colour: string; label: string }`; `export const FALSE_COLOUR_BANDS: FalseColourBand[]` (IRE-style bands: 0–7 purple "crushed", 8–20 blue "shadows", 21–42 grey-blue, 43–47 green "skin dark", 48–52 grey, 53–58 pink "skin", 59–77 grey, 78–92 yellow "highlights", 93–99 orange, 100 red "clipped"; `upTo` in 0–255 luminance);
  - `export function falseColour(lum: Uint8ClampedArray | number[], out: Uint8ClampedArray, palette: Record<string, [number, number, number]>): void` fills `out` RGBA from the band colours (palette keys are the band labels, resolved from CSS tokens by the component).
  - `export function bandFor(l: number): FalseColourBand`.
- `liveview.ts`: when `overlays.waveform`, a 128×64 canvas in a small glass box bottom-left shows the waveform redrawn on the existing `HIST_MS` tick (same downscaled sample, `GRID` → use a 128×72 sample for the waveform); when `overlays.falseColour`, a full-size canvas over the image is filled with the false-colour mapping of the current frame at the downscaled resolution and scaled up with `image-rendering: pixelated`; both respect `prefers-reduced-motion` (lower tick rate), both off by default.
- `sidePanels.ts`: `Waveform` and `False colour` switches flip `overlays.waveform` / `overlays.falseColour`.

- [ ] **Step 1: Write the failing tests** (`waveform` of a 4×4 all-black frame puts all counts in row 0; a left-black/right-white frame fills column buckets 0..63 at row 0 and 64..127 at the top row; `bandFor(0)` is "crushed", `bandFor(255)` is "clipped", `bandFor(128)` is "skin" per the table; `falseColour` writes the band colour's RGB and alpha 255; the two switches dispatch the overlay flips).
- [ ] **Step 2: Run to verify failure.** — [ ] **Step 3: Implement.** — [ ] **Step 4: Verify** (`pnpm -C apps/desktop test`, tsc). — [ ] **Step 5: Commit** — `Draw a waveform and false colour over live view`.

---

### Task 2: Several cameras at once

**Files:**
- Modify: `apps/desktop/src-tauri/src/state.rs` (big), `commands.rs`, `lib.rs`, `apps/desktop/src/{ipc,state,actions,app}.ts`, `components/{sidePanels,connectScreen,filmstrip,dock,ring,topbar,intervalTimer}.ts`, `styles/layout.css`
- Test: `state.rs` tests (two simulators), `actions.test.ts`, `state.test.ts`, `sidePanels.test.ts`, `filmstrip.test.ts`, `app.test.ts`

**Interfaces:**
- Rust:
  - `pub struct CameraId(pub String)`; the id is the remembered key: the ssid when connected through Wi-Fi, else `ip:port`.
  - `AppState.cameras: tokio::sync::Mutex<Vec<Slot>>` with `pub struct Slot { pub id: CameraId, pub label: String /* model */, pub camera: Arc<Camera> }`; `AppState.active: StdMutex<Option<CameraId>>`.
  - `connect(...)` adds a slot (replacing one with the same id), makes it active, and returns `(CameraId, CameraState, receiver)`; `disconnect(state, id: Option<CameraId>)` removes one (or the active one) and, if it was active, activates the next remaining; `disconnect_all`.
  - Every forwarded event becomes `TaggedEvent { camera_id: CameraId, event: CameraEvent }` on the Tauri event `camera-event`; the fixture pins the new JSON shape (`{"camera_id":"…","event":{…}}`).
  - All camera commands take `camera_id: Option<CameraId>` (None = active): `get_state`, `set_property`, `step_property`, `shoot`, `focus`, `toggle_record`, `live_view_frame`, `live_view_meta`, `apply_values`, `disconnect`.
  - New commands: `list_cameras() -> Vec<CameraSummary { id, label, connected, secured, active }>`, `set_active_camera(id)`, `shoot_all() -> Vec<(CameraId, Option<String /* error */>)>` (shoots every connected camera concurrently with `join_all`).
  - The rejoin hook, destination and recovery stay per camera. Captures from different cameras land in the same destination; the existing `-N` suffixing keeps both; `CaptureSaved` carries the camera id through the tag.
- TypeScript:
  - `AppState.cameras: Record<string, CameraState>`, `activeId: string | null`, `camera` becomes a derived getter `activeCamera(state)` (keep the field name `camera` for the active one to limit churn: `camera` is set from `cameras[activeId]` on every change).
  - `captures` items gain `cameraId` and `cameraLabel`; the filmstrip tile shows a small label when more than one camera is connected.
  - `actions.switchCamera(id)`, `actions.shootAll()`, `actions.disconnect(id?)`; `actions.addCamera()` shows the connect screen while keeping the connected cameras (`screen: 'connect'` with `keepCameras: true`), and the connect screen shows a `Back to <label>` button when any camera is connected.
  - Cameras panel: one row per connected camera (dot, label, `this` for the active), click switches; `Shoot all` button under the rows when more than one is connected (also in the dock next to Shoot when more than one); `Add a camera` row; `Disconnect` acts on the active camera, `Disconnect all` when more than one.
  - The interval timer run records `cameraId` at start and shoots that camera (`actions.shoot(cameraId)`), stopping only when that camera disconnects.
  - The live view polls the active camera; switching swaps the stream without unmounting.

- [ ] **Step 1: Write the failing tests** (Rust: connect to two simulators → two slots, second active; `shoot_all` saves two files with distinct `-N` names; `disconnect(active)` activates the other; events carry the right ids; the tagged fixture pinned. TS: `applyEvent` routes a `StateChanged` for a non-active camera into `cameras` without changing `camera`; `switchCamera` swaps `camera`; the filmstrip label; the Cameras panel rows; `addCamera` keeps cameras and shows Back; interval keeps its `cameraId` after a switch).
- [ ] **Step 2: Run to verify failure.** — [ ] **Step 3: Implement** (Rust first, then TS; keep single-camera behaviour identical when one camera is connected). — [ ] **Step 4: Verify** all suites plus a manual run with two simulators (`--port 15740` and `--port 15741`, connect both by address). — [ ] **Step 5: Commit** — `Connect several cameras and shoot them all`.

---

### Task 3: Card browsing in the library and simulator

**Files:**
- Create: `crates/sony-ptpip/src/sony/card.rs`, `crates/sony-ptpip-sim/src/card.rs`, `crates/sony-ptpip/tests/card.rs`
- Modify: `crates/sony-ptpip/src/ptp/codes.rs`, `src/sony/mod.rs`, `src/camera.rs` (+ `CameraEvent::CardDownloadProgress`), `src/lib.rs`, `crates/sony-ptpip-sim/src/{lib,server}.rs`, `src/quirks.rs` (`card_browse: CardBrowse::{SameSession, NeedsTransferMode}` default `SameSession`)

**Interfaces:**
- `codes.rs`: `OP_GET_STORAGE_IDS 0x1004`, `OP_GET_STORAGE_INFO 0x1005`, `OP_GET_OBJECT_HANDLES 0x1007`, `OP_GET_PARTIAL_OBJECT 0x101B`; formats `FMT_JPEG 0x3801`, `FMT_MP4 0xB984`, `FMT_SONY_RAW 0xB101`, `FMT_ASSOCIATION 0x3001`.
- `card.rs`:
  - `pub struct CardObject { pub handle: u32, pub storage: u32, pub filename: String, pub bytes: u64, pub format: u16, pub kind: CardKind, pub captured: Option<chrono::NaiveDateTime> }`, `pub enum CardKind { Still, Raw, Video, Other }` (from the format code and extension).
  - `pub async fn list_storages(conn) -> Result<Vec<u32>, Error>`, `pub async fn list_objects(conn, storage: u32) -> Result<Vec<CardObject>, Error>` (handles of all formats under parent 0xFFFFFFFF, info for each, associations skipped), `pub async fn download(conn, obj: &CardObject, dest_dir: &Path, chunk: u32, progress: impl FnMut(u64, u64)) -> Result<PathBuf, Error>`: writes `.rimix-<name>.part`, resumes from its current length with `GetPartialObject(handle, offset, chunk)` until `bytes`, then renames into place (reusing the capture module's `-N` suffix and temp-file helpers; factor them into `pub(crate)` functions if private). `pub const CARD_CHUNK: u32 = 1 << 20;`
- `camera.rs`: `Camera::card_list() -> Result<Vec<CardObject>, Error>` (all storages), `Camera::card_download(handle: u32) -> Result<PathBuf, Error>` emitting `CameraEvent::CardDownloadProgress { handle, filename, received, total }` and finishing with `CaptureSaved` (so the filmstrip shows it); runs on the command connection like a shot (holding the connection mutex per chunk, yielding between chunks so live view keeps going).
- Simulator: `SimConfig.card: Vec<SimCardFile { filename, bytes, format, captured }>` (default: three files: `DSC00010.JPG` 2 MiB, `DSC00010.ARW` 24 MiB, `C0001.MP4` 64 MiB, generated deterministic bytes so tests can hash them), `GetStorageIDs` → one storage, `GetObjectHandles`, `GetObjectInfo`, `GetPartialObject` honouring offset/size, `SimConfig.drop_during_card_download_at: Option<u64>` (drops the link after that many bytes once).

- [ ] **Step 1: Write the failing tests** (`list_objects` returns the three files with kinds; `download` of the MP4 matches the simulator's bytes; a drop at 10 MiB followed by reconnect and a second `download` resumes from the `.part` length (assert `GetPartialObject` offsets seen by the sim start at the resumed offset and the final file matches); the `.part` is never renamed short; `card_list` through `Camera`; `CardDownloadProgress` events ascend).
- [ ] **Step 2: Run to verify failure.** — [ ] **Step 3: Implement.** — [ ] **Step 4: Verify** `cargo test --workspace`, clippy. — [ ] **Step 5: Commit** — `Browse the card and download clips with resume`.

---

### Task 4: Browse card in the app and CLI

**Files:**
- Create: `apps/desktop/src/components/cardBrowser.ts`, `cardBrowser.test.ts`
- Modify: `apps/desktop/src-tauri/src/{state,commands,lib}.rs`, `apps/desktop/src/{ipc,state,actions}.ts`, `components/sidePanels.ts` (a `Browse card` row in the Cameras panel), `styles/layout.css`, `crates/rimix-cli/src/{main,commands}.rs`
- Test: `state.rs`, `cardBrowser.test.ts`, `actions.test.ts`, `cli.rs`

**Interfaces:**
- Commands: `card_list(camera_id?) -> Vec<CardObject>`, `card_download(camera_id?, handle) -> PathBuf`; events `CardDownloadProgress` flow through the tagged event.
- `cardBrowser.ts` `mount(root, store, actions)`: a glass panel (opened from the `Browse card` row, closed with Escape/Close) listing files newest first with kind icon, name, size, date and a `Download` button; progress bar per row during download; downloaded rows show `Saved`; `Refresh`; an empty card says `No files on the card`; while the camera's quirk says `NeedsTransferMode` the panel explains `This camera needs its own transfer mode for card browsing; not available yet`.
- `AppState.card: { items: CardObject[]; loading: boolean; downloads: Record<number, { received: number; total: number }> }`.
- CLI: `rimix card list` (table: kind, name, size, date, handle) and `rimix card get <handle|name> [--out DIR]` with a progress line and resume.

- [ ] **Step 1: Write the failing tests** (Rust command tests with the simulator; the panel renders three rows and Download triggers `actions.cardDownload(handle)`; progress updates the row; CLI list/get round-trip with a hash check).
- [ ] **Step 2–5** as before. Commit — `Show the card's files and download them`.

---

### Task 5: The SFTP receiver crate

**Files:**
- Create: `crates/rimix-sftp/Cargo.toml`, `src/lib.rs`, `src/handler.rs`, `tests/receive.rs`
- Modify: workspace `Cargo.toml`

**Interfaces:**
- `pub struct ReceiverConfig { pub bind: SocketAddr, pub user: String, pub password: String, pub host_key: russh::keys::PrivateKey, pub destination: PathBuf }`
- `pub struct Receiver { .. }` with `pub async fn start(cfg: ReceiverConfig, on_file: impl Fn(ReceivedFile) + Send + Sync + 'static) -> Result<Receiver, Error>`, `pub fn local_addr(&self) -> SocketAddr`, `pub fn fingerprint_sha256(&self) -> String`, `pub async fn stop(self)`; `pub struct ReceivedFile { pub path: PathBuf, pub bytes: u64, pub remote_name: String }`.
- Behaviour: password auth for the one user only (3 failures close the connection); the SFTP subsystem only (no shell/exec); the client sees a root directory `/` mapped to `destination` (plus date subfolders are created when the client writes into a folder path); uploads go to `.rimix-sftp-<name>.part` and are renamed into place on `close`, with the `-N` suffix rule from the capture module (copy the two helpers into a tiny shared crate `crates/rimix-fs`? No: keep it simple and duplicate the 20-line rename helper with a comment naming the original); `on_file` fires after the rename; directory listing, `stat`, `mkdir`, `realpath` work; `remove`/`rename` by the client are allowed inside the root only; path traversal outside the root is refused (`PermissionDenied`).
- `pub fn load_or_create_host_key(path: &Path) -> std::io::Result<PrivateKey>` (ed25519, OpenSSH PEM, mode 0o600 on unix).

- [ ] **Step 1: Write the failing tests** (with `russh` + `russh-sftp` client in dev-deps: upload a 5 MiB file → `on_file` once, bytes equal, no `.part` left; two concurrent uploads of the same name → both kept (`-1`), no half file visible between writes (poll the directory during upload and assert only `.rimix-sftp-*` names appear); `..` traversal refused; wrong password refused; host key stable across `load_or_create`).
- [ ] **Step 2–5** as before. Commit — `Receive files the camera pushes over SFTP`.

---

### Task 6: Receive from camera in the app and CLI

**Files:**
- Create: `apps/desktop/src-tauri/src/sftp.rs`
- Modify: `settings.rs` (`sftp: SftpSettings { enabled: bool, port: u16 (2222), user: "rimix", password: String (generated 12 chars on first use) }`), `state.rs`, `commands.rs`, `lib.rs`, `apps/desktop/src/{ipc,state,actions}.ts`, `components/appSettings.ts` (section `Receive from camera`), `components/filmstrip.ts`, `crates/rimix-cli` (`rimix sftp serve --dir DIR --port 2222 --user rimix --password-stdin`)
- Test: `settings.rs`, `state.rs`, `appSettings.test.ts`, `cli.rs`

**Interfaces:**
- Commands: `sftp_status() -> { running: bool; address: string | null; user: string; password: string; fingerprint: string | null; error: string | null }`, `sftp_set_enabled(enabled: bool)`, `sftp_regenerate_password()`. The receiver starts with the app when enabled, binds `0.0.0.0:<port>`, writes into the current destination (following destination changes), and each `ReceivedFile` is emitted as a `CaptureSaved` tagged with camera id `sftp` so it appears in the filmstrip with the label `Received`.
- App settings section `Receive from camera`: a switch, then when on: `Address <lan-ip>:<port>` (the Wi-Fi interface's IPv4 from `rimix-wifi::current_address`, or `this computer's address` when unknown), `Username`, `Password` (with Show), `Fingerprint`, a `New password` button, and the hint `On the camera: Menu, Network, FTP Transfer Func., Server Setting; choose SFTP, enter this address, username and password.` Errors (port in use) show in the section.
- Host key stored at `<config>/Rimix/sftp_host_key`.

- [ ] **Step 1: Write the failing tests** (settings defaults and migration; `sftp_set_enabled(true)` starts a receiver on a free port in tests (`port: 0` → ephemeral when 0), an upload lands in the destination and the state sees a `CaptureSaved` with id `sftp`; the section renders and toggles; CLI serve accepts one upload).
- [ ] **Step 2–5** as before. Commit — `Let the camera push files to Rimix over SFTP`.

---

### Task 7: The Bluetooth crate

**Files:**
- Create: `crates/rimix-ble/Cargo.toml`, `src/lib.rs` (trait + types + fake), `src/sony.rs` (advertisement parser, uuids, shutter sequence), `src/btleplug_adapter.rs` (real), `tests/fake.rs`
- Modify: workspace `Cargo.toml`, `crates/rimix-cli` (`rimix ble scan [--seconds N]`, `rimix ble handoff <name>`, `rimix ble shoot <name>`)

**Interfaces:**
- `pub struct SonyAdvertisement { pub name: Option<String>, pub model_code: String, pub pairing_open: bool, pub address: String }`; `pub fn parse_sony_manufacturer_data(company_id: u16, data: &[u8]) -> Option<SonyAdvertisement>` per the research (`0x012D`; bytes `03 00` camera, protocol version, two model-code bytes, tag `0x22` status `0xEF` open / `0xAF` not).
- `pub const SERVICE_CAMERA_CONTROL: Uuid = 8000CC00-CC00-FFFF-FFFF-FFFFFFFFFFFF; CHAR_SSID = …CC06…; CHAR_PASSWORD = …CC07…; SERVICE_REMOTE = 8000FF00-FF00-…; CHAR_REMOTE_COMMAND = …FF01…; CHAR_REMOTE_NOTIFY = …FF02…`.
- `pub const SHUTTER_SEQUENCE: [[u8; 2]; 4] = [[0x01,0x07],[0x01,0x09],[0x01,0x08],[0x01,0x06]]` (half down, full down, full up, half up) with 100 ms between writes.
- `#[async_trait] pub trait BleAdapter: Send + Sync { async fn scan(&self, for_: Duration) -> Result<Vec<Found>, BleError>; async fn read(&self, device: &str, char: Uuid) -> Result<Vec<u8>, BleError>; async fn write(&self, device: &str, char: Uuid, data: &[u8]) -> Result<(), BleError>; }` with `Found { address: String, advert: SonyAdvertisement }`; `BleError { Unavailable(String) /* no adapter or permission */, NotFound, Io(String) }`.
- `pub async fn handoff(adapter: &dyn BleAdapter, device: &str) -> Result<WifiCredentials { ssid: String, password: String }, BleError>`: reads CC06 and CC07, trims NULs, accepts only when the ssid starts with `DIRECT-` (the byte format is undocumented; anything else → `BleError::Io("unrecognised credential format: <hex>")` so the owner can report it).
- `pub async fn shoot(adapter: &dyn BleAdapter, device: &str) -> Result<(), BleError>` writes the sequence to FF01.
- `pub struct FakeAdapter` (scripted devices and characteristic values; records writes) and `pub fn system_adapter() -> Result<Box<dyn BleAdapter>, BleError>` (btleplug; `Unavailable` when no adapter).

- [ ] **Step 1: Write the failing tests** (parser on the research's example advertisement `2D 01 03 00 64 00 45 31 22 EF 00` → camera, model code "E1", pairing open; a non-Sony company id → None; `handoff` with the fake returning `DIRECT-abc1:ZV-E10` / `12345678` succeeds and with garbage fails with the hex in the message; `shoot` writes the four frames in order; CLI `ble scan` with `RIMIX_BLE=fake` lists the fake device).
- [ ] **Step 2–5** as before. Commit — `Find Sony cameras over Bluetooth and shoot with it`.

---

### Task 8: Bluetooth in the app

**Files:**
- Create: `apps/desktop/src-tauri/src/ble.rs`
- Modify: `state.rs`, `commands.rs`, `lib.rs`, `Info.plist` (`NSBluetoothAlwaysUsageDescription` = `Rimix uses Bluetooth to find nearby Sony cameras and read their Wi-Fi details.`), `Entitlements.plist`, `settings.rs` (`bluetooth: bool` default true), `apps/desktop/src/{ipc,actions,copy}.ts`, `components/connectScreen.ts` (Nearby card), `components/appSettings.ts` (`Bluetooth` switch under Connection)
- Test: `state.rs` (with `RIMIX_BLE=fake`), `connectScreen.test.ts`, `copy.test.ts`

**Interfaces:**
- Commands: `ble_scan(seconds) -> Vec<Found>`, `ble_handoff(address) -> WifiCredentials`, `ble_shoot(address)`; the adapter is built once (`RIMIX_BLE=fake` → fake with one scripted ZV-E10; `off` → `Unavailable`).
- Nearby card: when `settings.bluetooth`, a scan runs every 10 s alongside SSDP; Bluetooth rows show `<model> nearby` with `pairing open` or `pairing off`, and a `Join via Bluetooth` button that calls `ble_handoff` then `connectWithWifi(creds)`; a handoff failure shows `Rimix could not read the Wi-Fi details over Bluetooth (<detail>). Scan the QR code or type the name and password instead.` once per device until the list changes; `Unavailable` shows one quiet line `Bluetooth is off or not allowed; nearby cameras come from Wi-Fi only.` and stops scanning (Review Focus 4).
- Dock: no BLE shutter button in this phase (the Wi-Fi shutter is the product); BLE shoot stays CLI-only. Ruling recorded.

- [ ] **Step 1: Write the failing tests** (state: fake scan lists the device; handoff returns creds; `off` → the Unavailable text; connect screen renders BLE rows and the join flow; copy strings).
- [ ] **Step 2–5** as before. Commit — `Show Bluetooth cameras and join through them`.

---

### Task 9: Leftovers, the webcam dev flag, and docs

**Files:**
- Modify: `apps/desktop/src/components/connectScreen.ts` (no rescan while the Access Authentication box is open; empty-username message names the username field), `actions.ts` (`connecting` set before the pre-join disconnect), `apps/desktop/src-tauri/src/state.rs` + `commands.rs` (`dev_flags() -> { webcam: bool }` from `RIMIX_WEBCAM` (`off` → false)), `connectScreen.ts` (skip the scanner when `webcam` is false, status `Webcam off for this run`), `README.md`, `docs/camera-testing.md`
- Test: `connectScreen.test.ts`, `actions.test.ts`, `state.rs`

Docs: README features (scopes, several cameras, Browse card, Receive from camera, Bluetooth), the env vars (`RIMIX_WEBCAM`, `RIMIX_BLE`), CLI additions (`card`, `sftp`, `ble`); `camera-testing.md` steps 9 (Browse card: what the ZV-E10 shows; whether files list in PC Remote mode), 10 (SFTP: camera menu path `FTP Transfer Func.`, send a test file), 11 (Bluetooth: `rimix ble scan`, the handoff output, whether `ble shoot` fires the shutter after pairing from the camera's Bluetooth menu); each with "What to send back".

- [ ] **Step 1: Write the failing tests** — [ ] **Step 2–5** as before. Commit — `Smoke-test flag, Bluetooth and card docs, small fixes`.

---

## Self-review notes

- Spec coverage for Phase 4 (section 7): multi-camera (Task 2), scopes (Task 1, zebra already existed), Bluetooth nearby/handoff/shutter (Tasks 7, 8; shutter is CLI-only, ruled), card browsing with video download (Tasks 3, 4; the camera's separate transfer mode is unknown, so browsing runs on the control session and a quirk flag records the alternative), SFTP receiver (Tasks 5, 6). Section 6.2's overlays: waveform and false colour (Task 1).
- Deferred for good, on purpose: BLE wake (no power-on command found), the "connect without pairing" variant (wire details unknown), the EE00 pairing exchange (undocumented).
- Only a real camera can prove: whether the ZV-E10 answers card operations in PC Remote mode, the handoff characteristic format, the FF01 shutter on a paired Mac, the SFTP push from the camera's FTP function.
- Type consistency: `CameraId` is a string in TS; tagged events `{camera_id, event}`; `CardObject` identical in Rust and TS (`handle`, `storage`, `filename`, `bytes`, `format`, `kind`, `captured`); `WifiCredentials` from the BLE crate matches the TS type from Phase 2.
