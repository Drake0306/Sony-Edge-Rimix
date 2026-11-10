# Rimix Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Tauri desktop app and a Rust command-line tool that connect to a Sony ZV-E10 over Wi-Fi (PTP/IP on TCP 15740), show live view, change exposure settings, shoot stills and record video, and save captures into the chosen folder, with automatic reconnect; everything proven against an in-process simulated camera because the real camera is only available to the owner.

**Architecture:** A Cargo workspace. `crates/sony-ptpip` implements Sony's PTP/IP dialect with no UI dependency (packet codec, transport, SDIO handshake, property blob, controls, live view, capture receive, discovery, a `Camera` facade with reconnect). `crates/sony-ptpip-sim` is a fake camera that speaks the same dialect over real sockets so tests and the UI run without hardware. `crates/rimix-cli` drives the crate from a terminal. `apps/desktop` is a Tauri v2 app (Rust commands over the `Camera` facade, Vite + TypeScript front end ported from the approved mockup).

**Tech Stack:** Rust 1.93 (tokio, bytes, thiserror, tracing, serde, serde_json, uuid, directories, quick-xml, image for the simulator), Tauri 2.12 (tauri, tauri-plugin-dialog, tauri-plugin-opener), Vite + TypeScript (no UI framework), Vitest.

**Spec:** `docs/superpowers/specs/2026-11-10-rimix-design.md`. Protocol facts come from the research in `reports/Sony camera wireless desktop control.md` and the open-source references cloned read-only under `/private/tmp/claude-501/-Users-roy-Developer-Github-Sony-Edge-Rimix/a9188942-8515-40dd-bc70-0944c54fbe60/scratchpad/ref/` (alpha2go `docs/protocol.md` Apache-2.0, remote-camera-control-rs Apache-2.0/MIT, sony-alpha-python no licence: read for behaviour only, libgphoto2 `ptp.h`/`ptp-pack.c` LGPL: read for constants and byte layouts only, never copy code).

## Global Constraints

- Licence: Apache-2.0 for everything in this repository (`LICENSE` file at the root). Do not copy code from GPL or LGPL projects; constants and byte layouts are facts and may be used.
- Target camera: Sony ZV-E10, firmware 2.00, pairing generation (no SSH). Code paths must not assume SSH exists but must leave room for it (a `Transport` trait with one TCP implementation now).
- Commit messages: plain language, under 60 characters, no "Co-Authored-By" or any Claude attribution. Example: `Add PTP/IP packet framing`.
- Platforms: macOS 14+ (Apple silicon and Intel) first; nothing may break a Windows build (no Unix-only APIs outside `cfg` blocks).
- Everything integer on the wire is little-endian. Port 15740 for command and event connections. Special object handles: live view `0xFFFFC002`, captured still `0xFFFFC001`.
- Safety: never send vendor operations with guessed parameters to a real camera; the CLI's "experiment" subcommands must be explicit about what they send. Only write property values that appear in that property's own settable list or range.
- No Sony SDK, no legacy JSON API.
- The app bundle target is under 30 MB on macOS; no Electron, no heavyweight UI framework.
- Front-end copy follows the spec: "Shoot", "Record", "Focus", "Save to", "Connect", sentence case.

## Review Focus

Inputs the spec implies but no happy-path test exercises. Each line has a test added to the task that owns the code.

1. A malformed or oversized PTP/IP packet from the camera (length field larger than 64 MiB or smaller than 8) must produce a `ProtocolError`, never a panic or an unbounded allocation. (Task 1)
2. Live view `GetObject` answering `0x200F AccessDenied` (frame not ready) or `0x2009 InvalidObjectHandle` (live view off) must be retried quietly at the next tick, not logged as an error storm or treated as a disconnect. (Task 8)
3. A property entry with an unknown datatype or an array type must be skipped without losing the entries after it. (Task 5)
4. A destination folder that is missing or not writable must surface one clear error in the UI and keep the capture bytes in a recoverable temp file instead of dropping the shot. (Task 9)
5. A socket drop in the middle of a capture download must leave no partial file at the destination and must not produce a duplicate when the camera re-delivers the same object after reconnect. (Task 9 and Task 11)

---

## File structure

```
Cargo.toml                               workspace: crates/*, apps/desktop/src-tauri
LICENSE                                  Apache-2.0
.gitignore                               target/, node_modules/, dist/, .DS_Store
.github/workflows/ci.yml                 cargo fmt --check, clippy, test on macos-latest
crates/sony-ptpip/
  Cargo.toml
  src/lib.rs                             pub mod ptpip, ptp, sony, discovery, camera, guid, quirks, error
  src/error.rs                           Error enum (Io, Protocol, Response{code}, Timeout, Disconnected, Unsupported)
  src/ptpip/packet.rs                    Packet enum + encode/decode
  src/ptpip/transport.rs                 PtpIpConnection: TCP command + event sockets, transactions, event stream
  src/ptp/codes.rs                       op/response/event/property/datatype constants
  src/ptp/value.rs                       PtpValue + DataType encode/decode
  src/ptp/object_info.rs                 ObjectInfo decode
  src/sony/handshake.rs                  sdio_connect(), ExtDeviceInfo
  src/sony/property.rs                   PropertyDesc, parse_all_props(), set_property()
  src/sony/decode.rs                     human labels for shutter/aperture/ISO/EV/WB/mode/focus/drive/format/battery
  src/sony/control.rs                    shutter/record/focus/step controls via 0x9207
  src/sony/liveview.rs                   LiveViewFrame parse + fetch
  src/sony/capture.rs                    poll 0xD215, fetch 0xFFFFC001, Destination writer
  src/discovery/ssdp.rs                  M-SEARCH, parse LOCATION
  src/discovery/upnp.rs                  dd.xml + DigitalImagingDesc.xml → DiscoveredCamera
  src/discovery/probe.rs                 default gateway probe on 15740
  src/discovery/mod.rs                   discover(timeout) → Vec<DiscoveredCamera>
  src/guid.rs                            load_or_create_guid(path)
  src/quirks.rs                          Quirks per model (write mode, live view path)
  src/camera.rs                          Camera facade: connect, state, events, live view, captures, reconnect
  tests/                                 integration tests against the simulator
crates/sony-ptpip-sim/
  Cargo.toml
  src/lib.rs                             SimCamera::spawn(SimConfig) → SimHandle (addr, control knobs)
  src/server.rs                          PTP/IP responder loop
  src/props.rs                           property store + blob encode (mirror of parse)
  src/frames.rs                          synthetic JPEG live view + capture images
  src/ssdp.rs                            SSDP responder + tiny HTTP for dd.xml
  fixtures/zv-e10.json                   property fixture
  src/main.rs                            `sony-ptpip-sim` binary for manual UI development
crates/rimix-cli/
  Cargo.toml
  src/main.rs                            clap subcommands
apps/desktop/
  package.json, vite.config.ts, tsconfig.json, index.html
  src/main.ts, src/app.ts, src/state.ts, src/ipc.ts, src/components/*.ts, src/styles/*.css
  src-tauri/Cargo.toml, tauri.conf.json, capabilities/default.json
  src-tauri/src/main.rs, lib.rs, commands.rs, settings.rs, state.rs
docs/camera-testing.md                   owner checklist for the real ZV-E10
```

---

### Task 1: Workspace, licence, CI and PTP/IP packet codec

**Files:**
- Create: `Cargo.toml`, `LICENSE`, `.gitignore`, `.github/workflows/ci.yml`
- Create: `crates/sony-ptpip/Cargo.toml`, `crates/sony-ptpip/src/lib.rs`, `crates/sony-ptpip/src/error.rs`, `crates/sony-ptpip/src/ptpip/mod.rs`, `crates/sony-ptpip/src/ptpip/packet.rs`
- Test: `crates/sony-ptpip/src/ptpip/packet.rs` (unit tests module)

**Interfaces:**
- Produces:
  ```rust
  pub enum Packet {
      InitCommandRequest { guid: [u8; 16], name: String, version: u32 },   // version 0x0001_0000
      InitCommandAck { conn_number: u32, guid: [u8; 16], name: String, version: u32 },
      InitEventRequest { conn_number: u32 },
      InitEventAck,
      InitFail { reason: u32 },
      CmdRequest { data_phase: u32, op: u16, tid: u32, params: Vec<u32> },
      CmdResponse { code: u16, tid: u32, params: Vec<u32> },
      Event { code: u16, tid: u32, params: Vec<u32> },
      StartData { tid: u32, total_len: u64 },
      Data { tid: u32, payload: Vec<u8> },
      EndData { tid: u32, payload: Vec<u8> },
      Cancel { tid: u32 },
      ProbeRequest,
      ProbeResponse,
  }
  impl Packet { pub fn encode(&self) -> Vec<u8>; pub fn decode(frame: &[u8]) -> Result<Packet, Error>; }
  pub const MAX_PACKET_LEN: u32 = 64 * 1024 * 1024;
  pub fn read_frame<R: tokio::io::AsyncRead + Unpin>(r: &mut R) -> impl Future<Output = Result<Vec<u8>, Error>>; // reads [len u32][type u32][payload], returns the whole frame incl. header; rejects len < 8 or > MAX_PACKET_LEN
  pub enum Error { Io(std::io::Error), Protocol(String), Response { code: u16, op: u16 }, Timeout, Disconnected, Unsupported(&'static str) }
  ```
- Packet type numbers: 1 InitCommandRequest, 2 InitCommandAck, 3 InitEventRequest, 4 InitEventAck, 5 InitFail, 6 CmdRequest, 7 CmdResponse, 8 Event, 9 StartData, 10 Data, 11 Cancel, 12 EndData, 13 ProbeRequest, 14 ProbeResponse. Names in Init packets are UTF-16LE, NUL-terminated. CmdRequest data_phase: 1 = no data or data-in, 2 = data-out.

- [ ] **Step 1: Scaffold the workspace.** Root `Cargo.toml` with `[workspace] members = ["crates/*", "apps/desktop/src-tauri"]`, `resolver = "2"`, shared `[workspace.dependencies]` for tokio (features full), bytes, thiserror, tracing, serde (derive), serde_json, uuid (v4), anyhow. Apache-2.0 `LICENSE`. `.gitignore` with `target/`, `node_modules/`, `dist/`, `.DS_Store`. CI workflow running `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` on `macos-latest`. Until `apps/desktop/src-tauri` exists (Task 13), list only `crates/*` in members.
- [ ] **Step 2: Write failing tests** in `packet.rs`:
  - `encodes_init_command_request`: guid all `0x11`, name `"Rimix"`, version `0x0001_0000` → bytes equal `[len=8+16+12+4][type=1][guid][52 00 69 00 6D 00 69 00 78 00 00 00][00 00 01 00]` (len = 40 → `28 00 00 00`).
  - `decodes_init_command_ack`: build bytes with conn 7, guid `0x22`, name `"ILCE-6700"`, version `0x00010000`; decode equals expected.
  - `cmd_request_roundtrip`: `CmdRequest{data_phase:1, op:0x1002, tid:1, params:[0x0007]}` encode then decode equals original; encoded length is `8 + 4 + 2 + 4 + 4 = 22`.
  - `data_packets_roundtrip`: StartData total 5, Data payload `[1,2,3]`, EndData payload `[4,5]`.
  - `rejects_short_frame`: `Packet::decode(&[1,2,3])` is `Err(Error::Protocol(_))`.
  - `read_frame_rejects_oversized_length`: a reader yielding `[FF FF FF FF][06 00 00 00]` → `Err(Error::Protocol(_))` without allocating (test completes instantly).
  - `read_frame_rejects_tiny_length`: `[04 00 00 00]` → `Err`.
- [ ] **Step 3: Run `cargo test -p sony-ptpip`** and confirm compile failure (types missing).
- [ ] **Step 4: Implement** `Packet::encode`, `Packet::decode`, `read_frame`, `Error` as specified. UTF-16 helpers `encode_utf16z(&str) -> Vec<u8>` and `decode_utf16z(&[u8]) -> Result<(String, usize), Error>` live in `packet.rs` as `pub(crate)`.
- [ ] **Step 5: Run `cargo test -p sony-ptpip`**: all pass. Run `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- [ ] **Step 6: Commit** `Set up the Rust workspace and PTP/IP packets`.

---

### Task 2: PTP datatypes and ObjectInfo

**Files:**
- Create: `crates/sony-ptpip/src/ptp/mod.rs`, `crates/sony-ptpip/src/ptp/codes.rs`, `crates/sony-ptpip/src/ptp/value.rs`, `crates/sony-ptpip/src/ptp/object_info.rs`

**Interfaces:**
- Produces:
  ```rust
  // codes.rs (pub consts, u16 unless noted)
  OP_GET_DEVICE_INFO=0x1001, OP_OPEN_SESSION=0x1002, OP_CLOSE_SESSION=0x1003, OP_GET_OBJECT_INFO=0x1008, OP_GET_OBJECT=0x1009,
  OP_SDIO_CONNECT=0x9201, OP_SDIO_GET_EXT_DEVICE_INFO=0x9202, OP_SDIO_SET_EXT_DEVICE_PROP_VALUE=0x9205, OP_SDIO_CONTROL_DEVICE=0x9207, OP_SDIO_GET_ALL_EXT_DEVICE_PROP_INFO=0x9209, OP_SDIO_GET_EXT_DEVICE_PROP=0x9251,
  RC_OK=0x2001, RC_OPERATION_NOT_SUPPORTED=0x2005, RC_INVALID_OBJECT_HANDLE=0x2009, RC_ACCESS_DENIED=0x200F, RC_DEVICE_BUSY=0x2019, RC_INVALID_PARAMETER=0x201D,
  EV_SONY_OBJECT_ADDED=0xC201, EV_SONY_DEVICE_PROP_CHANGED=0xC203, EV_SONY_CAPTURED=0xC206,
  HANDLE_LIVE_VIEW: u32=0xFFFFC002, HANDLE_CAPTURED_STILL: u32=0xFFFFC001,
  PROP_WHITE_BALANCE=0x5005, PROP_F_NUMBER=0x5007, PROP_FOCUS_MODE=0x500A, PROP_EXPOSURE_PROGRAM=0x500E, PROP_EXPOSURE_BIAS=0x5010, PROP_STILL_CAPTURE_MODE=0x5013, PROP_COMPRESSION=0x5004,
  PROP_SHUTTER_SPEED=0xD20D, PROP_ISO=0xD21E, PROP_CAPTURED_READY=0xD215, PROP_BATTERY=0xD218, PROP_MOVIE_REC_STATE=0xD21D, PROP_LIVE_VIEW_STATUS=0xD221, PROP_STILL_DESTINATION=0xD222, PROP_FOCUS_FOUND=0xD213, PROP_IMAGE_SIZE_TO_HOST=0xD268,
  CTRL_SHUTTER_HALF=0xD2C1, CTRL_SHUTTER_FULL=0xD2C2, CTRL_MOVIE_REC=0xD2C8,
  DT_INT8=0x0001 ... DT_UINT64=0x0008, DT_STR=0xFFFF, DT_ARRAY_FLAG=0x4000
  // value.rs
  pub enum PtpValue { I8(i8), U8(u8), I16(i16), U16(u16), I32(i32), U32(u32), I64(i64), U64(u64), Str(String), Array(Vec<PtpValue>) }
  impl PtpValue {
      pub fn decode(dt: u16, buf: &[u8], pos: &mut usize) -> Result<PtpValue, Error>; // strings: u8 char count then UTF-16LE incl. NUL; count 0 = empty
      pub fn encode(&self, out: &mut Vec<u8>);
      pub fn as_u64(&self) -> Option<u64>; pub fn as_i64(&self) -> Option<i64>;
  }
  // object_info.rs
  pub struct ObjectInfo { pub storage_id: u32, pub object_format: u16, pub compressed_size: u32, pub filename: String, pub capture_date: String }
  pub fn parse_object_info(buf: &[u8]) -> Result<ObjectInfo, Error>; // standard PTP ObjectInfo dataset layout
  ```

- [ ] **Step 1: Write failing tests**: `decodes_each_scalar_type` (one assertion per type with explicit bytes, e.g. `DT_INT16` on `[0xFE,0xFF]` → `I16(-2)`), `decodes_ptp_string` (`[03, 41 00, 42 00, 00 00]` → `"AB"`, pos advances 7), `empty_string_is_one_byte`, `encode_matches_decode_for_u32_and_str`, `unknown_datatype_is_unsupported_error` (dt `0x0042` → `Err(Error::Unsupported(_))`), `parses_object_info_filename` (hand-built dataset with format `0x3801`, size 1234, filename `"DSC01236.JPG"`).
- [ ] **Step 2: Run tests; confirm they fail to compile.**
- [ ] **Step 3: Implement** `codes.rs`, `value.rs`, `object_info.rs`. Array types: `DT_ARRAY_FLAG | scalar` decode as `u32 count` then elements.
- [ ] **Step 4: Run `cargo test -p sony-ptpip`**: pass; clippy clean.
- [ ] **Step 5: Commit** `Add PTP datatypes and object info parsing`.

---

### Task 3: TCP transport with transactions, plus the simulator skeleton

**Files:**
- Create: `crates/sony-ptpip/src/ptpip/transport.rs`
- Create: `crates/sony-ptpip-sim/Cargo.toml`, `crates/sony-ptpip-sim/src/lib.rs`, `crates/sony-ptpip-sim/src/server.rs`
- Test: `crates/sony-ptpip/tests/transport.rs`

**Interfaces:**
- Produces (transport):
  ```rust
  pub struct ConnectOptions { pub guid: [u8; 16], pub name: String, pub timeout: Duration /* default 5 s */, pub pairing_wait: Duration /* default 60 s: how long to wait for InitCommandAck while the camera shows the pairing prompt */ }
  pub struct PtpIpConnection { /* command stream, event task, next tid */ }
  pub struct DataIn(pub Vec<u8>);
  impl PtpIpConnection {
      pub async fn connect(addr: SocketAddr, opts: ConnectOptions) -> Result<(PtpIpConnection, Packet /*InitCommandAck*/), Error>; // opens command socket, InitCommandRequest, awaits ack (pairing_wait), opens event socket, InitEventRequest/Ack, then OpenSession(session 1)
      pub async fn command(&mut self, op: u16, params: &[u32]) -> Result<Vec<u32> /*response params*/, Error>;              // no data phase; Err(Error::Response{code,op}) when code != RC_OK
      pub async fn command_data_in(&mut self, op: u16, params: &[u32]) -> Result<(Vec<u8>, Vec<u32>), Error>;              // collects StartData/Data/EndData
      pub async fn command_data_out(&mut self, op: u16, params: &[u32], data: &[u8]) -> Result<Vec<u32>, Error>;           // sends StartData + EndData(payload)
      pub fn events(&self) -> tokio::sync::broadcast::Receiver<PtpEvent>;  // PtpEvent { code: u16, params: Vec<u32> }
      pub async fn close(self) -> Result<(), Error>;  // CloseSession then drop sockets
  }
  ```
- Produces (simulator):
  ```rust
  pub struct SimConfig { pub model: String /* "ZV-E10" */, pub require_pairing: bool, pub pairing_delay: Duration, pub fixture: PathBuf /* fixtures/zv-e10.json, used from Task 5 */ }
  impl Default for SimConfig;
  pub struct SimHandle { pub addr: SocketAddr, pub events: tokio::sync::mpsc::Sender<SimCommand>, pub log: Arc<Mutex<Vec<String>>> }
  pub enum SimCommand { DropConnection, SetProperty { code: u16, value: PtpValue }, Shutdown }
  pub async fn spawn(config: SimConfig) -> SimHandle;  // binds 127.0.0.1:0 so tests can run in parallel
  ```
  In this task the server handles only: Init packets (two connections matched by conn_number), OpenSession, CloseSession, GetDeviceInfo (vendor extension id 0x11, model string from config, device version "2.00", supported ops list including the Sony ops), unknown ops → `RC_OPERATION_NOT_SUPPORTED`. When `require_pairing`, it delays the InitCommandAck by `pairing_delay`.

- [ ] **Step 1: Write failing integration tests** in `tests/transport.rs` (dev-dependency on `sony-ptpip-sim`): `connects_and_opens_session` (ack name equals "ZV-E10"), `get_device_info_round_trip` (data-in returns payload whose model string parses to "ZV-E10"), `unsupported_op_is_response_error` (op `0x9999` → `Err(Error::Response{code:0x2005,..})`), `events_are_delivered` (sim sends `EV_SONY_DEVICE_PROP_CHANGED` with param `0xD20D` via `SimCommand::SetProperty`; `events()` receiver gets it within 1 s), `pairing_delay_is_tolerated` (sim `pairing_delay` 2 s, client `pairing_wait` 10 s → connects), `drop_connection_yields_disconnected` (after `SimCommand::DropConnection`, next `command` returns `Err(Error::Disconnected)`).
- [ ] **Step 2: Run tests; confirm compile failure.**
- [ ] **Step 3: Implement** `transport.rs` (transaction ids start at 1 and increase; the event socket is read by a spawned task that forwards `Packet::Event` to a broadcast channel of capacity 64 and sets a shared disconnected flag on EOF) and the simulator skeleton (`server.rs` accepts connections, pairs command and event sockets by conn_number, runs a per-session loop).
- [ ] **Step 4: Run `cargo test --workspace`**: pass; clippy clean.
- [ ] **Step 5: Commit** `Add PTP/IP transport and a simulated camera`.

---

### Task 4: Sony SDIO handshake

**Files:**
- Create: `crates/sony-ptpip/src/sony/mod.rs`, `crates/sony-ptpip/src/sony/handshake.rs`
- Modify: `crates/sony-ptpip-sim/src/server.rs` (handle 0x9201, 0x9202)
- Test: `crates/sony-ptpip/tests/handshake.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct ExtDeviceInfo { pub protocol_version: u16 /* e.g. 0x012C */, pub properties: Vec<u16>, pub controls: Vec<u16> }
  pub async fn sdio_connect(conn: &mut PtpIpConnection) -> Result<ExtDeviceInfo, Error>;
  // sequence: 0x9201 [1,0,0]; 0x9201 [2,0,0]; 0x9202 [0x012C] data-in → parse; 0x9201 [3,0,0]
  pub fn parse_ext_device_info(buf: &[u8]) -> Result<ExtDeviceInfo, Error>;
  // layout: u16 version, u16 n, n × u16 property codes, u16 m, m × u16 control codes
  ```
- Simulator: answers 0x9201 with RC_OK; 0x9202 returns version `0x012C`, property codes from the fixture (Task 5; until then a fixed list `[0x5005,0x5007,0x500A,0x500E,0x5010,0xD20D,0xD21E,0xD215,0xD218,0xD222]`) and control codes `[0xD2C1,0xD2C2,0xD2C8]`. Any Sony op before phase 3 completes → `RC_ACCESS_DENIED`.

- [ ] **Step 1: Write failing tests**: `parses_ext_device_info_bytes` (hand-built bytes with 2 properties, 1 control), `handshake_completes_against_sim` (returns version 0x012C and includes 0xD20D), `sony_ops_before_handshake_are_denied` (calling `command(0x9209,&[0])` before `sdio_connect` → `Err(Error::Response{code:0x200F,..})`).
- [ ] **Step 2: Run; confirm failure.** **Step 3: Implement.** **Step 4: Tests pass, clippy clean.**
- [ ] **Step 5: Commit** `Add the Sony connection handshake`.

---

### Task 5: Property blob parsing, reading and writing

**Files:**
- Create: `crates/sony-ptpip/src/sony/property.rs`
- Create: `crates/sony-ptpip-sim/src/props.rs`, `crates/sony-ptpip-sim/fixtures/zv-e10.json`
- Modify: `crates/sony-ptpip-sim/src/server.rs` (0x9209 modes 0 and 1, 0x9205, 0x9251, emit 0xC203 on change)
- Test: unit tests in `property.rs`; `crates/sony-ptpip/tests/properties.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum Form { None, Range { min: PtpValue, max: PtpValue, step: PtpValue }, Enum { settable: Vec<PtpValue>, reportable: Vec<PtpValue> } }
  pub enum Enabled { Off, On, DisplayOnly }
  pub struct PropertyDesc { pub code: u16, pub datatype: u16, pub writable: bool, pub enabled: Enabled, pub default: PtpValue, pub current: PtpValue, pub form: Form }
  pub fn parse_all_props(buf: &[u8]) -> Result<Vec<PropertyDesc>, Error>;
  // layout per alpha2go/libgphoto2: u64 count; per entry: u16 code, u16 datatype, u8 writable(1), u8 enabled(0/1/2), default, current, u8 form (0 none, 1 range: min,max,step; 2 enum: u16 n, n values). After an enum list, peek u16: if < 0x0200 it is the count of a second list (reportable); otherwise it is the next entry's code.
  // An entry whose datatype cannot be decoded is skipped only if its size is knowable; otherwise parsing stops and returns the entries so far plus a tracing::warn. Array datatypes are decoded via PtpValue::decode.
  pub async fn read_all_props(conn: &mut PtpIpConnection) -> Result<Vec<PropertyDesc>, Error>;          // 0x9209 [0]
  pub async fn read_changed_props(conn: &mut PtpIpConnection) -> Result<Vec<PropertyDesc>, Error>;      // 0x9209 [1]
  pub async fn set_property(conn: &mut PtpIpConnection, code: u16, value: &PtpValue) -> Result<(), Error>; // 0x9205 [code] data-out = encoded value
  pub fn encode_all_props(props: &[PropertyDesc]) -> Vec<u8>;  // used by the simulator; must round-trip with parse_all_props
  ```
- Fixture `zv-e10.json`: an array of entries `{code, datatype, writable, enabled, default, current, form}` covering: 0x500E exposure program (u16, enum settable [1 M, 2 P, 3 A, 4 S], current 1), 0xD20D shutter (u32, enum of 20 values encoded as `num<<16 | den` e.g. `0x0001_007D` for 1/125, current 1/125), 0x5007 aperture (u16, enum [400,450,500,560,630,710,800,900,1000,1100], current 400), 0xD21E ISO (u32, enum [0x00FFFFFF auto,100,125,160,200,250,320,400,500,640,800,1000,1250,1600,2000,2500,3200,6400,12800,25600,32000], current 800), 0x5010 EV (i16, enum from -5000 to 5000 step 300 plus 0, current 300), 0x5005 WB (u16, enum [2 auto, 4 daylight, 0x8011 shade, 0x8010 cloudy, 6 tungsten, 0x8001 fluorescent warm, 0x8002 cool, 0x8003 day white, 0x8004 daylight, 7 flash, 0x8007 colour temp, 0x8020 custom 1], current 0x8007), 0xD20F colour temperature (u16, range 2500..9900 step 100, current 5200), 0x500A focus mode (u16, enum [1 MF, 2 AF-S, 0x8004 AF-C, 0x8005 DMF, 0x8006 AF-A], current 0x8004), 0x5013 drive (u16, enum [1 single, 2 continuous hi, 0x8012 continuous lo, 0x8004 self-timer 10 s], current 1), 0x5004 compression (u8, enum [2 std, 3 fine, 4 extra fine, 0x10 RAW, 0x13 RAW+fine], current 0x13), 0xD215 captured ready (u16, read-only, 0), 0xD218 battery (u8, read-only, 72), 0xD21D movie rec state (u8, read-only, 0), 0xD222 destination (u16, enum [1,0x10,0x11], current 0x11), 0xD268 host image size (u8, enum [1,2], current 1), 0xD213 focus found (u8, read-only, 0). Values are plausible, not verified on a ZV-E10; the owner's camera dump will replace them later.
- Simulator: `props.rs` loads the fixture into a `BTreeMap<u16, PropertyDesc>`, tracks a "changed since last 0x9209(1)" set, validates `0x9205` values against the settable list or range (reject with `RC_INVALID_PARAMETER`), and sends `EV_SONY_DEVICE_PROP_CHANGED [code]` on every change (including those made through `SimCommand::SetProperty`).

- [ ] **Step 1: Write failing tests**: `parses_single_enum_entry` (hand-built bytes), `parses_range_entry`, `parses_second_reportable_list` (bytes where after the settable list a u16 `0x0003` and three values follow, then the next entry code `0xD21E`), `skips_entry_with_unknown_datatype_and_keeps_following_entries` (datatype `0x0042` with form 0 and value width unknown → implementation must stop or skip; assert the result includes entries parsed before it and the function returns `Ok`), `encode_then_parse_roundtrips_fixture` (sim fixture through `encode_all_props` then `parse_all_props` equals input), `read_all_props_from_sim_has_shutter_1_125` (current of 0xD20D is `U32(0x0001007D)`), `set_property_to_listed_value_changes_current_and_emits_event` (set 0xD21E to 1600, receive 0xC203 with param 0xD21E, `read_changed_props` contains 0xD21E with current 1600), `set_property_to_unlisted_value_is_invalid_parameter` (ISO 1234 → `Err(Error::Response{code:0x201D,..})`).
- [ ] **Step 2: Run; confirm failure.** **Step 3: Implement.** **Step 4: Pass, clippy clean.**
- [ ] **Step 5: Commit** `Read and write camera settings`.

---

### Task 6: Human-readable values

**Files:**
- Create: `crates/sony-ptpip/src/sony/decode.rs`
- Test: unit tests in the same file

**Interfaces:**
- Produces:
  ```rust
  pub struct Labeled { pub label: String, pub short: String }
  pub fn label_for(code: u16, value: &PtpValue) -> Option<Labeled>;  // None when the code is unknown; callers then show the raw number
  pub fn shutter_label(raw: u32) -> String;     // num<<16|den: 1/125 → "1/125"; 0x000A_0001 → "10\"" ; 0 → "Bulb"; 0x0001_0003 → "1/3"
  pub fn aperture_label(raw: u16) -> String;    // 400 → "f/4.0", 1000 → "f/10", 560 → "f/5.6"
  pub fn iso_label(raw: u32) -> String;         // 0x00FFFFFF → "ISO Auto", 800 → "ISO 800"; upper byte non-zero is mode (0x10 = multi-frame NR) → "ISO 800 NR"
  pub fn ev_label(raw: i16) -> String;          // 300 → "+0.3", -700 → "-0.7", 0 → "0.0"
  pub fn wb_label(raw: u16, colour_temp: Option<u16>) -> String;  // 2 → "Auto", 4 → "Daylight", 0x8007 → "5200K" when colour_temp Some(5200)
  pub fn exposure_program_label(raw: u16) -> String; // 1 "M", 2 "P", 3 "A", 4 "S", 0x8050 "Movie P", 0x8051 "Movie A", 0x8052 "Movie S", 0x8053 "Movie M", other → format "0x{raw:04X}"
  pub fn focus_mode_label(raw: u16) -> String;  // 1 "MF", 2 "AF-S", 0x8004 "AF-C", 0x8005 "DMF", 0x8006 "AF-A"
  pub fn drive_label(raw: u16) -> String;       // 1 "Single", 2 "Continuous Hi", 0x8012 "Continuous Lo", 0x8004 "Self-timer 10 s", 0x8005 "Self-timer 2 s"
  pub fn format_label(raw: u8) -> String;       // 2 "JPEG Standard", 3 "JPEG Fine", 4 "JPEG Extra fine", 0x10 "RAW", 0x13 "RAW+JPEG", 0x12 "RAW+JPEG Standard"
  pub fn battery_label(raw: u8) -> String;      // "72%"
  ```
- [ ] **Step 1: Write failing tests** with the exact pairs listed in the comments above (one `assert_eq!` per pair).
- [ ] **Step 2: Run; confirm failure.** **Step 3: Implement.** **Step 4: Pass, clippy clean.**
- [ ] **Step 5: Commit** `Show camera settings in plain words`.

---

### Task 7: Controls: shoot, record, focus, and stepped writes

**Files:**
- Create: `crates/sony-ptpip/src/sony/control.rs`, `crates/sony-ptpip/src/quirks.rs`
- Modify: `crates/sony-ptpip-sim/src/server.rs` (0x9207 handling, capture queue, recording state)
- Test: `crates/sony-ptpip/tests/control.rs`

**Interfaces:**
- Produces:
  ```rust
  pub async fn control(conn: &mut PtpIpConnection, code: u16, value: &PtpValue) -> Result<(), Error>; // 0x9207 [code] data-out = value
  pub async fn shoot(conn: &mut PtpIpConnection, with_af: bool) -> Result<(), Error>;
  // with_af: D2C1=U16(2); sleep 500 ms; D2C2=U16(2); sleep 35 ms; D2C2=U16(1); sleep 1000 ms; D2C1=U16(1)
  // without: D2C2=U16(2); sleep 35 ms; D2C2=U16(1)
  pub async fn focus(conn: &mut PtpIpConnection) -> Result<(), Error>;        // D2C1 down, 700 ms, D2C1 up
  pub async fn toggle_record(conn: &mut PtpIpConnection) -> Result<(), Error>; // D2C8=U16(2); 35 ms; D2C8=U16(1)
  pub enum WriteMode { Direct, Step }
  pub struct Quirks { pub write_mode: WriteMode, pub live_view_via_http: bool, pub pairing: bool }
  pub fn quirks_for(model: &str) -> Quirks;  // "ZV-E10" → Direct first (fallback to Step at runtime on RC_INVALID_PARAMETER... no: Quirks is static; see set_value below), live_view_via_http false, pairing true; unknown models → same defaults
  pub async fn set_value(conn: &mut PtpIpConnection, desc: &PropertyDesc, target: &PtpValue, quirks: &Quirks) -> Result<(), Error>;
  // Direct: set_property. Step: send control(code, I8(+1 or -1)) repeatedly, re-reading the property via 0x9251 [code] after each step, until current == target or 40 steps; the direction is the sign of (index of target − index of current) within the settable list; Err(Error::Unsupported("step")) if target not in list.
  ```
- Simulator: 0x9207 on D2C2 with value 2 then 1 enqueues a capture (sets 0xD215 current to `0x8000 | queue_len`, emits 0xC203 [0xD215]); D2C8 press+release toggles 0xD21D between 0 and 1 and emits 0xC203; D2C1 value 2 sets 0xD213 to 1 after 200 ms; control with an I8 on a property code (step mode) moves current to the neighbouring settable value. A `SimConfig.write_mode_step: bool` makes 0x9205 answer `RC_INVALID_PARAMETER` so tests can exercise the step path.

- [ ] **Step 1: Write failing tests**: `shoot_enqueues_one_capture` (after `shoot(true)`, `read_changed_props` shows 0xD215 with bit 15 set and count 1), `toggle_record_flips_state` (0xD21D 0 → 1 → 0 across two calls), `focus_sets_focus_found`, `direct_set_value_changes_iso`, `step_set_value_reaches_target_when_direct_writes_are_rejected` (sim in step mode; set ISO from 800 to 1600 → current 1600; sim log shows two or more 0x9207 steps), `set_value_rejects_target_not_in_list`.
- [ ] **Step 2: Run; confirm failure.** **Step 3: Implement.** **Step 4: Pass, clippy clean.**
- [ ] **Step 5: Commit** `Shoot, record and focus from the computer`.

---

### Task 8: Live view

**Files:**
- Create: `crates/sony-ptpip/src/sony/liveview.rs`
- Create: `crates/sony-ptpip-sim/src/frames.rs`
- Modify: `crates/sony-ptpip-sim/src/server.rs` (GetObjectInfo/GetObject on 0xFFFFC002)
- Test: unit tests in `liveview.rs`; `crates/sony-ptpip/tests/liveview.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct FocusFrame { pub kind: u16 /*1 normal, 2 focused/main*/, pub state: u16, pub cx: f32, pub cy: f32, pub w: f32, pub h: f32 } // normalised 0..1 using the block's x_max/y_max
  pub struct LiveViewFrame { pub jpeg: Vec<u8>, pub focus_frames: Vec<FocusFrame>, pub level_deg: Option<f32> }
  pub fn parse_live_view_dataset(buf: &[u8]) -> Result<Option<LiveViewFrame>, Error>;
  // header: u32 image_offset, u32 image_size, u32 meta_offset, u32 meta_size. image_size 0 → Ok(None). JPEG must start FF D8 or Err(Protocol).
  // metadata (when meta_size > 0): u16 version at 0; if version >= 0x64: level block at 0x10 (u32 state, u32 reserved, i32 x, i32 y, i32 z) → level_deg = x / 1000.0 when state == 2 (tenths assumption; document); frame blocks from 0x28: u32 x_max, u32 y_max, u16 count, pad to 16; count × 24 bytes (u16 type, u16 state, u32 reserved, u32 cx, u32 cy, u32 h, u32 w). Parse the first block (focus frames) only; ignore the rest. Any parse failure in metadata yields jpeg with empty frames (never fails the frame).
  pub struct LiveView { /* holds conn borrow pattern: methods take &mut PtpIpConnection */ }
  pub async fn start(conn: &mut PtpIpConnection) -> Result<(), Error>;                 // GetObjectInfo(0xFFFFC002) once; RC_INVALID_OBJECT_HANDLE → Err(Error::Unsupported("live view not available"))
  pub async fn next_frame(conn: &mut PtpIpConnection) -> Result<Option<LiveViewFrame>, Error>; // GetObject(0xFFFFC002); RC_ACCESS_DENIED or RC_DEVICE_BUSY → Ok(None) (caller waits ~15 ms and retries); RC_INVALID_OBJECT_HANDLE → Ok(None) and a tracing::debug once per 5 s (rate-limited by a static); other codes → Err
  ```
- Simulator: `frames.rs` renders a 640×424 JPEG with the `image` crate: grey background, a moving circle whose x position advances each frame, a frame counter; `GetObjectInfo(0xFFFFC002)` returns a dataset with format 0x3801; `GetObject(0xFFFFC002)` returns `[16-byte header][metadata 0x28 + one focus block with 1 frame][jpeg]`. `SimConfig.live_view_busy_every: Option<u32>` makes every Nth GetObject answer `RC_ACCESS_DENIED`.

- [ ] **Step 1: Write failing tests**: `parses_dataset_with_one_focus_frame` (hand-built bytes: cx 327680 of x_max 655360 → cx 0.5), `zero_image_size_is_none`, `bad_jpeg_marker_is_protocol_error`, `metadata_parse_failure_still_returns_jpeg`, `streams_ten_frames_from_sim` (ten `Some` frames, each JPEG starts FF D8 and decodes with `image`), `access_denied_frames_are_skipped_not_errors` (sim busy every 3rd; 9 calls → 6 `Some`, 3 `None`, no `Err`).
- [ ] **Step 2: Run; confirm failure.** **Step 3: Implement.** **Step 4: Pass, clippy clean.**
- [ ] **Step 5: Commit** `Stream live view from the camera`.

---

### Task 9: Receiving captures into the destination folder

**Files:**
- Create: `crates/sony-ptpip/src/sony/capture.rs`
- Modify: `crates/sony-ptpip-sim/src/server.rs` (GetObjectInfo/GetObject on 0xFFFFC001 pop the queue; `SimConfig.drop_during_capture: bool`)
- Test: `crates/sony-ptpip/tests/capture.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum DestinationMode { CustomFolder, LightroomWatched, FolderPerDate }
  pub struct Destination { pub base: PathBuf, pub mode: DestinationMode }
  impl Destination {
      pub fn target_dir(&self, now: chrono::NaiveDate) -> PathBuf; // CustomFolder/LightroomWatched → base; FolderPerDate → base/YYYY-MM-DD
      pub fn ensure_writable(&self, now: NaiveDate) -> Result<PathBuf, Error>; // creates the dir; Err(Error::Io) if not writable
  }
  pub struct Received { pub path: PathBuf, pub bytes: u64, pub filename: String }
  pub async fn captured_ready(conn: &mut PtpIpConnection) -> Result<u16, Error>; // reads 0xD215 via 0x9251 → count if bit 15 set else 0
  pub async fn receive_one(conn: &mut PtpIpConnection, dest: &Destination, on_progress: &mut dyn FnMut(u64, u64)) -> Result<Received, Error>;
  // GetObjectInfo(0xFFFFC001) → filename + size; GetObject(0xFFFFC001) streamed to <target_dir>/.rimix-<filename>.part, calling on_progress(received, total) per Data packet; on success rename to <filename>; if <filename> exists, write <stem>-1.<ext>, -2, ... ; on any Err remove the .part file and return the Err.
  // Lightroom mode uses the same temp-then-rename so Lightroom never sees a partial file.
  ```
- Add `chrono` to workspace dependencies. Duplicate protection: `receive_one` returns `Error::Protocol("duplicate")`? No: it writes `-1` suffix; dedupe by content is the `Camera` facade's job in Task 11 (it remembers (filename, size) of the last 50 received and skips re-deliveries).

- [ ] **Step 1: Write failing tests**: `folder_per_date_creates_dated_dir` (base temp dir, date 2026-11-10 → `base/2026-11-10` exists), `custom_folder_uses_base`, `unwritable_destination_is_io_error` (base is a file, not a dir), `receives_shot_into_folder` (shoot via Task 7 then `receive_one` → file `DSC00001.JPG` exists, size matches, no `.part` left, progress called at least twice with final equal to total), `existing_name_gets_suffix`, `drop_mid_transfer_leaves_no_partial_file` (sim `drop_during_capture` → `Err(Error::Disconnected)` and directory has no `.part` and no `DSC00001.JPG`).
- [ ] **Step 2: Run; confirm failure.** **Step 3: Implement.** **Step 4: Pass, clippy clean.**
- [ ] **Step 5: Commit** `Save new shots into the chosen folder`.

---

### Task 10: Discovery: SSDP, UPnP description and gateway probe

**Files:**
- Create: `crates/sony-ptpip/src/discovery/mod.rs`, `ssdp.rs`, `upnp.rs`, `probe.rs`
- Create: `crates/sony-ptpip-sim/src/ssdp.rs` (SSDP responder on a configurable multicast-less unicast port for tests, plus a tiny HTTP server serving `dd.xml` and `DigitalImagingDesc.xml`)
- Test: unit tests in `upnp.rs`, `crates/sony-ptpip/tests/discovery.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct DiscoveredCamera { pub model: String, pub friendly_name: String, pub ip: IpAddr, pub ptp_port: u16 /*15740*/, pub ptp_version: Option<String>, pub pairing_required: Option<bool>, pub ssh_supported: Option<bool>, pub source: DiscoverySource /* Ssdp | Gateway | Manual */ }
  pub async fn ssdp_search(timeout: Duration, target: Option<SocketAddr> /* None = 239.255.255.250:1900 */) -> Result<Vec<Url>, Error>; // M-SEARCH ST: ssdp:all, MX 2; collect LOCATION headers whose SERVER/USN mention "Sony" or whose description later parses as a Sony device
  pub async fn fetch_description(location: &Url) -> Result<DiscoveredCamera, Error>; // GET dd.xml; parse <friendlyName>, <modelName>, <manufacturer>; follow <av:X_ScalarWebAPI_...>? no: follow the DigitalImagingDesc link if present (X_PTP_Information: X_PTP_Versions, X_PTP_PairingNecessity, X_SSH_Support)
  pub fn parse_dd_xml(xml: &str) -> Result<(String /*model*/, String /*friendly*/, String /*manufacturer*/, Option<String> /*digital imaging desc path*/), Error>;
  pub fn parse_digital_imaging_desc(xml: &str) -> Result<(Option<String>, Option<bool>, Option<bool>), Error>;
  pub async fn probe_gateway(timeout: Duration) -> Result<Option<DiscoveredCamera>, Error>; // default gateway via `default-net` crate (or parse `route -n get default` on macOS and `route print` on Windows behind cfg) then TCP connect to :15740; on success return a Manual-source camera with model "Camera" (the handshake fills the real model later)
  pub async fn discover(timeout: Duration) -> Result<Vec<DiscoveredCamera>, Error>; // ssdp + gateway, deduplicated by ip
  ```
- The simulator's SSDP responder listens on a unicast UDP port given in `SimConfig.ssdp_port: Option<u16>` and answers any M-SEARCH with `LOCATION: http://127.0.0.1:<http_port>/dd.xml`, `SERVER: UPnP/1.0 Sony`. Its `dd.xml` names model "ZV-E10", manufacturer "Sony Corporation", and links `DigitalImagingDesc.xml` with `X_PTP_Versions` "3.00", `X_PTP_PairingNecessity` true, `X_SSH_Support` false.

- [ ] **Step 1: Write failing tests**: `parses_dd_xml_model_and_friendly_name`, `parses_digital_imaging_desc_flags`, `ssdp_finds_the_simulator` (search with `target = Some(sim ssdp addr)` → one camera, model "ZV-E10", pairing_required Some(true), ssh_supported Some(false)), `fetch_description_handles_missing_imaging_desc` (XML without the link → flags None).
- [ ] **Step 2: Run; confirm failure.** **Step 3: Implement** (HTTP GET with a minimal tokio implementation or `reqwest` with rustls; choose `reqwest` default-features off + rustls for simplicity). **Step 4: Pass, clippy clean.**
- [ ] **Step 5: Commit** `Find cameras on the network`.

---

### Task 11: The Camera facade: state, events, live view loop, capture loop, reconnect, GUID

**Files:**
- Create: `crates/sony-ptpip/src/camera.rs`, `crates/sony-ptpip/src/guid.rs`
- Test: `crates/sony-ptpip/tests/camera.rs`, unit tests in `guid.rs`

**Interfaces:**
- Produces:
  ```rust
  pub fn load_or_create_guid(path: &Path) -> Result<[u8; 16], Error>; // file holds 32 hex chars; created with uuid v4 if absent; same value on every later call
  pub struct CameraConfig { pub addr: SocketAddr, pub guid: [u8; 16], pub host_name: String /* "Rimix on <hostname>" */, pub destination: Destination, pub auto_receive: bool /* true */, pub reconnect: bool /* true */ }
  #[derive(Clone, serde::Serialize)] pub struct CameraState { pub connected: bool, pub model: String, pub firmware: String, pub battery: Option<u8>, pub recording: bool, pub focus_found: bool, pub properties: Vec<PropertySummary>, pub live_view_fps: f32, pub last_shot_at: Option<SystemTime> }
  #[derive(Clone, serde::Serialize)] pub struct PropertySummary { pub code: u16, pub label: String /* decode::label_for or hex */, pub short: String, pub raw: PtpValue, pub writable: bool, pub enabled: Enabled, pub choices: Vec<(PtpValue, String)> /* settable values with labels */ }
  #[derive(Clone, serde::Serialize)] pub enum CameraEvent { Connected, Disconnected { reason: String }, Reconnecting { attempt: u32 }, StateChanged(CameraState), Frame /* a new live view frame is available */, CaptureStarted { filename: String }, CaptureProgress { filename: String, received: u64, total: u64 }, CaptureSaved(Received), CaptureFailed { filename: String, error: String } }
  pub struct Camera { /* Arc<inner> */ }
  impl Camera {
      pub async fn connect(cfg: CameraConfig) -> Result<Camera, Error>;    // transport connect + GetDeviceInfo + sdio_connect + read_all_props; spawns the event loop, the live view loop and the capture loop
      pub fn subscribe(&self) -> broadcast::Receiver<CameraEvent>;
      pub fn state(&self) -> CameraState;
      pub fn latest_frame(&self) -> Option<Arc<LiveViewFrame>>;
      pub async fn set(&self, code: u16, value: PtpValue) -> Result<(), Error>;      // set_value with quirks, then read_changed_props and emit StateChanged
      pub async fn step(&self, code: u16, delta: i32) -> Result<(), Error>;          // move within choices by delta (clamped), then set
      pub async fn shoot(&self) -> Result<(), Error>;                                  // control::shoot(with_af = focus mode is not MF)
      pub async fn focus(&self) -> Result<(), Error>;
      pub async fn toggle_record(&self) -> Result<(), Error>;
      pub fn set_destination(&self, dest: Destination);
      pub async fn disconnect(&self);
  }
  ```
  Loops: event loop reads `conn.events()`; on 0xC203 it calls `read_changed_props` (coalesced to at most every 100 ms) and emits StateChanged. Live view loop: `liveview::start`, then `next_frame` continuously; `None` → sleep 15 ms; stores latest frame; computes fps over a 1 s window; emits `Frame`. Capture loop: every 250 ms `captured_ready`; while > 0 `receive_one` with progress events; skip a delivery whose (filename, size) matches one of the last 50 received. All loops share one `Mutex<PtpIpConnection>`; the live view loop yields between frames so commands are not starved (fairness: a tokio `Semaphore` or simply `tokio::task::yield_now`). Reconnect: when any loop sees `Error::Disconnected`, emit Disconnected, then retry connect with backoff 1 s, 2 s, 4 s, capped at 15 s, forever while `reconnect` is true, emitting Reconnecting{attempt}; on success emit Connected and restart loops.

- [ ] **Step 1: Write failing tests**: `guid_is_stable_across_calls`, `connect_populates_state_with_labels` (state has a property with code 0xD20D and short "1/125"), `set_iso_updates_state_and_emits_event`, `step_shutter_moves_to_neighbour` (from 1/125, step +1 → the next value in the fixture list), `live_view_frames_arrive` (within 2 s `latest_frame()` is Some and at least one `Frame` event seen), `shoot_results_in_capture_saved_event` (temp dir destination; event with filename `DSC00001.JPG`; file exists), `reconnects_after_drop` (SimCommand::DropConnection → Disconnected then Connected within 5 s; `state().connected` true; a subsequent `shoot` works), `duplicate_delivery_is_skipped` (sim re-queues the same object after reconnect; only one file saved).
- [ ] **Step 2: Run; confirm failure.** **Step 3: Implement.** **Step 4: Pass (allow these tests up to 20 s each), clippy clean.**
- [ ] **Step 5: Commit** `Keep the camera connected and in sync`.

---

### Task 12: Command-line tool

**Files:**
- Create: `crates/rimix-cli/Cargo.toml`, `crates/rimix-cli/src/main.rs`
- Create: `crates/sony-ptpip-sim/src/main.rs` (binary `sony-ptpip-sim --port 15740 --ssdp` for manual runs)
- Test: `crates/rimix-cli/tests/cli.rs` using `assert_cmd` against a spawned simulator

**Interfaces:**
- Subcommands (clap): `rimix discover [--timeout 3]`; `rimix connect <ip> [--name NAME]` (prints model, firmware, battery, then exits); `rimix props <ip> [--all]` (table: code, label, current, writable, choices count); `rimix set <ip> <code-hex> <value>` (value parsed as decimal or 0x-hex; checks the choice list first); `rimix step <ip> <code-hex> <+1|-1>`; `rimix shoot <ip> [--no-af] [--out DIR]` (shoots, waits for the capture, saves to DIR with FolderPerDate off); `rimix record <ip>` (toggle); `rimix liveview <ip> --frames N --out DIR` (saves `frame-0001.jpg` …); `rimix watch <ip> --out DIR` (stays connected, saves every new shot, prints events, Ctrl-C to quit). Global `--guid-file PATH` default `~/Library/Application Support/Rimix/guid` on macOS via `directories::ProjectDirs::from("", "", "Rimix")`.
- Exit codes: 0 ok, 2 usage, 3 connection failed, 4 camera refused (response error), 5 I/O.

- [ ] **Step 1: Write failing tests**: `discover_lists_simulator` (stdout contains "ZV-E10"), `props_shows_shutter` (contains "1/125"), `set_iso_then_props_shows_new_value`, `shoot_saves_a_file`, `liveview_saves_three_frames`, `connect_to_closed_port_exits_3`.
- [ ] **Step 2: Run; confirm failure.** **Step 3: Implement.** **Step 4: Pass, clippy clean.**
- [ ] **Step 5: Commit** `Add a command-line tool for the camera`.

---

### Task 13: Tauri app shell and commands

**Files:**
- Create: `apps/desktop/package.json`, `vite.config.ts`, `tsconfig.json`, `index.html`, `src/main.ts`, `src/ipc.ts`
- Create: `apps/desktop/src-tauri/Cargo.toml`, `tauri.conf.json`, `capabilities/default.json`, `build.rs`, `src/main.rs`, `src/lib.rs`, `src/commands.rs`, `src/settings.rs`, `src/state.rs`, `icons/` (generate with `cargo tauri icon` from a simple 1024px PNG drawn with the `image` crate or ImageMagick: a dark rounded square with a white ring)
- Modify: root `Cargo.toml` members to include `apps/desktop/src-tauri`
- Test: `apps/desktop/src-tauri/src/commands.rs` unit tests against the simulator; `apps/desktop/src/ipc.test.ts` with Vitest for message shapes

**Interfaces:**
- Tauri commands (all `#[tauri::command]`, async, managed state `AppState { camera: Mutex<Option<Camera>>, settings: Mutex<AppSettings>, guid: [u8;16] }`):
  ```rust
  discover(timeout_ms: u64) -> Result<Vec<DiscoveredCamera>, String>
  connect(ip: String) -> Result<CameraState, String>          // builds CameraConfig from settings; forwards CameraEvent to the window as Tauri events named "camera-event" (payload = CameraEvent JSON)
  disconnect() -> Result<(), String>
  get_state() -> Result<Option<CameraState>, String>
  set_property(code: u16, raw: serde_json::Value) -> Result<(), String>
  step_property(code: u16, delta: i32) -> Result<(), String>
  shoot() / focus() / toggle_record() -> Result<(), String>
  live_view_frame() -> Result<tauri::ipc::Response, String>   // raw JPEG bytes of the latest frame; empty body when none
  live_view_meta() -> Result<serde_json::Value, String>        // focus frames + level for the latest frame
  get_settings() -> Result<AppSettings, String>; save_settings(s: AppSettings) -> Result<(), String>
  pick_folder() -> Result<Option<String>, String>              // tauri-plugin-dialog
  ```
  ```rust
  #[derive(Serialize, Deserialize, Clone)] pub struct AppSettings { pub appearance: Appearance /* Dark|Light|System, default Dark */, pub layout: Layout /* Dock|Ring, default Dock */, pub destination_mode: DestinationMode /* default FolderPerDate */, pub destination_base: PathBuf /* default ~/Pictures/Rimix */, pub recent_cameras: Vec<RecentCamera { ip, model, last_used }> , pub last_ip: Option<String> }
  // stored as JSON at ProjectDirs "Rimix" config dir / settings.json; load() returns defaults if missing or unparsable (and logs)
  ```
- `tauri.conf.json`: productName "Rimix", identifier `com.rimix.desktop`, window 1440×900 min 1100×700, title "Rimix", `macOSPrivateApi` false, CSP allowing `img-src 'self' blob: data:`. Capabilities: core defaults + dialog + opener.
- Front end `ipc.ts`: typed wrappers `invoke<...>` for every command and `onCameraEvent(cb)` via `@tauri-apps/api/event`.

- [ ] **Step 1: Scaffold** with `pnpm create tauri-app` equivalents by hand (Vite vanilla-ts template) so the structure matches the file list; add `@tauri-apps/api`, `@tauri-apps/plugin-dialog`, `vitest`, `typescript`. Confirm `pnpm install` and `cargo tauri dev` launch a blank window (manual, once), then stop it.
- [ ] **Step 2: Write failing tests**: Rust `commands.rs` tests that construct `AppState` against the simulator and call `connect`, `set_property`, `shoot`, `live_view_frame` (non-empty, starts FF D8); `settings.rs` tests `defaults_when_missing`, `roundtrip_save_load`. Vitest `ipc.test.ts`: `cameraEventSchema_parses_state_changed_payload` (a fixture JSON of `CameraEvent::StateChanged` matches the TS type guard).
- [ ] **Step 3: Run; confirm failure.** **Step 4: Implement.** **Step 5: `cargo test --workspace` and `pnpm test` pass; `cargo tauri build --debug` produces an app bundle under 30 MB (record the size in the commit message body).**
- [ ] **Step 6: Commit** `Add the desktop app shell and camera commands`.

---

### Task 14: Main screen and connect screen (dock layout, dark theme)

**Files:**
- Create: `apps/desktop/src/app.ts`, `src/state.ts`, `src/components/topbar.ts`, `liveview.ts`, `dock.ts`, `settingsPopover.ts`, `sidePanels.ts`, `filmstrip.ts`, `connectScreen.ts`, `src/styles/tokens.css`, `glass.css`, `layout.css`
- Source of truth: `design/mockups/20-rimix-final.html` (port its markup, CSS tokens and glass material; keep class names where sensible)
- Test: Vitest `src/state.test.ts`, `src/components/dock.test.ts` (DOM via jsdom)

**Behaviour to implement (from spec 6.2 and 6.6):**
- Connect screen when no camera: "Connect a camera"; Nearby list from `discover` (auto-refresh every 5 s while visible, each row with a Connect button); "Enter the camera's address" with IP field prefilled `192.168.122.1` and Connect; Recently used from settings; QR panel present but showing "Coming in the next version" (QR is Phase 2); the pairing note text verbatim from the spec. Errors: connection refused → "Could not reach the camera at <ip>. Check that PC Remote is on and you are on the camera's Wi-Fi."; timeout during pairing → "The camera asked you to allow this computer. Press OK on the camera, then try again."
- Main screen: live view `<img>` updated from `live_view_frame()` on a `requestAnimationFrame` loop capped at 30 fps (object URLs revoked after swap); focus frames drawn in an overlay `<canvas>` from `live_view_meta()`; thirds grid toggle; level from meta; histogram computed from a 64×64 downscale of the frame every 250 ms; status capsule "Live view {fps} fps, last shot {n} s ago".
- Dock: ten chips bound to property codes `[0x500E, 0xD20D, 0x5007, 0xD21E, 0x5010, 0x5005, 0x500A, 0x5013, 0x5004, <picture profile: hidden until a code is known>]`, label from `PropertySummary.short`; disabled look when `enabled != On`; clicking opens the camera settings popover anchored to the chip; presets C1–C4 rendered inert with a tooltip "Presets arrive in the next version"; Record (red when `recording`), Shoot, Focus call the commands; Space/R/F keyboard shortcuts; Escape closes popovers.
- Camera settings popover: groups Exposure (0x500E, 0xD20D, 0x5007, 0xD21E, 0x5010), Image (0x5005 + 0xD20F when WB is colour temp, 0x5004), Focus and drive (0x500A, 0x5013); each row label, current `short`, minus/plus steppers calling `step_property(code, ∓1)` for enum or range values; segmented control for 0x500E when its choices are 4 or fewer; row disabled when not writable or not enabled.
- Side panels: Scopes and timer (toggles inert except Zebra which is CSS-only on the canvas: pixels above 95% luminance striped), Cameras list (connected camera + recent), Save to (path, three mode options, "Choose folder" via `pick_folder`).
- Filmstrip: last six `CaptureSaved` items with a thumbnail from the saved file (read via `convertFileSrc` or a `read_thumbnail` command returning bytes; choose the command), progress overlay from `CaptureProgress`.
- Top bar: Rimix, model, connection type ("Wi-Fi" until Phase 2 knows Direct vs router), battery, firmware, App settings button (opens the App settings popover from Task 15; in this task it opens a popover with only "Save to" defaults).

- [ ] **Step 1: Write failing Vitest tests**: `state_reducer_applies_state_changed`, `dock_renders_ten_chips_with_short_labels` (jsdom, fixture state), `stepper_calls_step_property_with_minus_one`, `connect_error_message_for_refused` (pure function mapping error strings to copy).
- [ ] **Step 2: Run; confirm failure.** **Step 3: Implement** components as small modules exporting `mount(root: HTMLElement, store)`; `state.ts` is a tiny observable store. **Step 4: `pnpm test` passes; `pnpm build` passes type-check; `cargo tauri dev` against `sony-ptpip-sim --port 15740` shows live view moving, chips with values, a shot landing in the filmstrip (manual check; take one screenshot with the OS screenshot tool and save it to `docs/screenshots/phase1-dock.png`).**
- [ ] **Step 5: Commit** `Build the shooting screen and connect screen`.

---

### Task 15: Themes, ring layout and App settings

**Files:**
- Create: `apps/desktop/src/components/appSettings.ts`, `src/components/ring.ts`, `src/styles/themes.css`
- Modify: `app.ts`, `tokens.css`, `settings.rs` already has fields

**Behaviour (spec 6.3, 6.4, 6.5):** App settings popover with Appearance (Dark, Light, System) and Layout (Dock, Ring) segmented rows plus Save to defaults; writes through `save_settings`; `data-theme` and `data-layout` on `<html>` exactly as in mockup 20; System follows `prefers-color-scheme` with a media query listener; Cmd/Ctrl+comma toggles the popover. Ring layout ports mockup 20's ring SVG; its segments are bound to the same ten property codes and open the same settings panel beside the ring with the selected row highlighted; Shoot in the hub, Record and Focus arcs.

- [ ] **Step 1: Write failing tests**: `applying_light_sets_html_data_theme`, `system_follows_media_query` (mock `matchMedia`), `ring_renders_ten_segments_bound_to_codes`, `settings_roundtrip_through_ipc_mock`.
- [ ] **Step 2: Run; confirm failure.** **Step 3: Implement.** **Step 4: Tests pass; manual check of all four states against the simulator; screenshots to `docs/screenshots/phase1-light.png` and `phase1-ring.png`.**
- [ ] **Step 5: Commit** `Add light theme, ring layout and app settings`.

---

### Task 16: Owner's camera checklist and documentation

**Files:**
- Create: `docs/camera-testing.md`
- Modify: `README.md` (how to build, run the simulator, run the app, run the CLI)

**Content of `docs/camera-testing.md`** (written for the owner, step by step, no jargon):
1. Camera setup: Menu, Network, PC Remote Function (or "Ctrl w/ Smartphone" off), PC Remote On, PC Remote Cnct Method Wi-Fi Direct, open Wi-Fi Direct Info, note the network name and password; also how to switch to Access Point mode for the router test.
2. The go/no-go QR test: Menu, Network, Send to Smartphone, show the QR; scan it with the phone's camera app and paste what it says into an issue or message; then, with the laptop joined to that network, run `rimix discover` and `nc -vz 192.168.122.1 15740`; record whether the port answers.
3. Pairing test: `rimix connect 192.168.122.1`; what the camera screen should show; press OK; expected output; what to send back if it fails (the full terminal output).
4. Dump: `rimix props 192.168.122.1 --all > zv-e10-props.txt` and share the file so the simulator fixture can be corrected.
5. Shoot test: `rimix shoot 192.168.122.1 --out ~/Desktop/rimix-test`; expected file.
6. Live view test: `rimix liveview 192.168.122.1 --frames 30 --out ~/Desktop/rimix-test`.
7. App test: launch the app, connect, shoot, change ISO, record; what to report.
8. Safety: never run anything not listed here against the camera; if the camera freezes, power-cycle it; pairing is permanent until a camera reset.

- [ ] **Step 1: Write both documents.** **Step 2: Verify every command in them exists in the CLI (`rimix --help` lists them).** **Step 3: Commit** `Explain how to test with a real camera`.

---

## Self-review notes

- Spec coverage: sections 5 (discovery, transport, session, control, keepalive/reconnect, quirks) → Tasks 3, 4, 5, 7, 10, 11; section 6.1–6.6 → Tasks 13–15; 6.7 destinations → Task 9; 6.8 presets → inert slots in Task 14 (Phase 2 feature, as the spec says); phase 1 "CLI proves each step" → Task 12; go/no-go and pairing tests → Task 16 (owner-run); Wi-Fi joining, QR, SSH, BLE are Phase 2+ and intentionally absent.
- Types: `PtpValue`, `PropertyDesc`, `Destination`, `CameraState`, `CameraEvent`, `DiscoveredCamera` are defined once (Tasks 2, 5, 9, 11, 10) and reused by name in Tasks 12–15.
- Review Focus mapping: 1 → Task 1 oversized/tiny frame tests; 2 → Task 8 `access_denied_frames_are_skipped_not_errors`; 3 → Task 5 unknown datatype test; 4 → Task 9 `unwritable_destination_is_io_error` and the UI error copy in Task 14; 5 → Task 9 `drop_mid_transfer_leaves_no_partial_file` and Task 11 `duplicate_delivery_is_skipped`.
- Known uncertainty carried deliberately: the ZV-E10 fixture values are plausible, not measured; the real dump from Task 16 step 4 replaces them.
