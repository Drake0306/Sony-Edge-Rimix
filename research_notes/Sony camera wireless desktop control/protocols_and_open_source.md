# Sony Alpha wireless control from a desktop: protocols, reverse engineering, and open-source landscape (as of November 2026)

Scope note: this covers every known non-SDK way to talk to a Sony Alpha/ZV/FX/RX body over a network or BLE, with wire-level specifics where they are publicly documented. Items that only apply to pre-2020 ("Camera Remote API"/PlayMemories-era) bodies are flagged as **[LEGACY, pre-2020]**. Items confirmed on 2021+ bodies (a1, a7 IV, a7R V, a6700, FX30, a7C II, etc.) are flagged **[MODERN]**.

---

## Q1. PTP/IP (ISO 15740 over TCP 15740) and Sony's vendor extension: handshake, "Access Authen.", and what is on the wire during Imaging Edge Desktop Remote over Wi-Fi

### Takeaway
Modern Sony bodies expose a standard PTP/IP responder on TCP 15740 plus the "SDIO" vendor extension (opcodes 0x92xx, properties 0xD2xx) that is the same protocol used over USB; the only wireless-specific additions are (a) the PTP/IP Init_Command_Request pairing step (host GUID + friendly name shown on the camera, user presses OK, GUID is remembered) and (b) on firmware with "Access Authen." = On, an SSH server on the camera through which PTP/IP port 15740 is tunnelled using the username/password shown in the camera's "Access Authen. Info" menu. Open-source code (Python, Rust, ESP32/C++, libgphoto2) exists that completes the handshake on a7 III / a6700 / FX30 / a1 / a6600 class bodies, so Sony's authentication does not block third parties; it is "just" SSH with keyboard-interactive password auth.

### Cited Findings

**Standard PTP/IP layer**
- PTP/IP default port is 15740; the control/data connection is initiator (computer) driven, the event connection is responder driven; packets are `[u32 length LE][u32 type LE][payload]`; type 1 = Init_Command_Request carrying a 16-byte GUID followed by a null-terminated UTF-16 computer name; type 2 = Init_Command_Ack returning a 4-byte connection number, the camera's 16-byte GUID and its name; type 3/4 = Init_Event_Request/Ack; type 6 = Cmd_Request (`u32 data-phase flag, u16 opcode, u32 transaction id, u32 params...`); 7 = Cmd_Response; 8 = Event; 9/10/12 = Start_Data/Data/End_Data; 13 = Ping/Pong — [gphoto PTP/IP documentation](https://gphoto.sourceforge.io/doc/ptpip.php)
- libgphoto2's `ptpip.c` uses port 15740 for both data and event sockets (`eventport = port = 15740`), builds Init_Command_Request from a GUID stored in `~/.gphoto/settings` under `ptp2_ip/guid` (randomly generated if absent) plus `gethostname()`; alternative port syntax is `ptpip:IP:dataport:eventport` — [libgphoto2 camlibs/ptp2/ptpip.c](https://github.com/gphoto/libgphoto2/blob/master/camlibs/ptp2/ptpip.c); [libgphoto2 NEWS](https://github.com/gphoto/libgphoto2/blob/master/NEWS)
- The Rust `rocc-ptp` crate builds the same Init_Command_Request as `[16-byte GUID][UTF-16LE name + 0x0000][u32 0x00000001 (protocol version 1.0)]`, and parses Init_Command_Ack as `[u32 connection number][16-byte responder GUID][UTF-16 responder friendly name]` — [Streetblock/remote-camera-control-rs crates/rocc-ptp/src/lib.rs](https://github.com/Streetblock/remote-camera-control-rs/blob/main/crates/rocc-ptp/src/lib.rs)

**Sony vendor extension ("SDIO") opcodes/property codes as named by libgphoto2**
- `PTP_VENDOR_SONY = 0x00000011`; operations: `SDIO_Connect 0x9201`, `SDIO_GetExtDeviceInfo 0x9202`, `GetDevicePropdesc 0x9203`, `GetDevicePropertyValue 0x9204`, `SDIO_SetExtDevicePropValue 0x9205`, `GetControlDeviceDesc 0x9206`, `SDIO_ControlDevice 0x9207`, `SDIO_GetAllExtDevicePropInfo 0x9209` ("gets a 4126 byte blob of device props"), `SDIO_OpenSession 0x9210`, `SDIO_GetPartialLargeObject 0x9211`, `SDIO_SetContentsTransferMode 0x9212`, `SDIO_GetDisplayStringList 0x9215`, `SDIO_GetLensInformation 0x9223`, `SDIO_OperationsResultsSupported 0x922f`; QX-series variants `0x96F2..0x96FE`; events `Sony_ObjectAdded 0xC201`, `ObjectRemoved 0xC202`, `DevicePropChanged 0xC203`, `CapturedEvent 0xC206`, `CWBCaptureResult 0xC208`, `CameraSettingReadResult 0xC209`, `FTPSettingReadResult 0xC20A`, `MediaFormatResult 0xC20B`, `FTPDisplayNameListChanged 0xC20C` — [libgphoto2 camlibs/ptp2/ptp.h](https://github.com/gphoto/libgphoto2/blob/master/camlibs/ptp2/ptp.h)
- Representative Sony property codes from the same header: `ShutterSpeed 0xD20D`, `ISO 0xD21E`, `ExposureCompensation 0xD224`, `FocusFound 0xD213`, `Zoom 0xD214`, `ObjectInMemory 0xD215` ("used to signal when to retrieve new object"), `BatteryLevel 0xD218`, `MovieRecordingState 0xD21D`, `LiveViewStatus 0xD221`, `StillImageStoreDestination 0xD222`, `FocusArea 0xD22C`, `ShutterType 0xD0DF`, `SilentMode 0xD0DB` — [libgphoto2 ptp.h](https://github.com/gphoto/libgphoto2/blob/master/camlibs/ptp2/ptp.h)
- libgphoto2's Sony init sequence (USB and PTP/IP alike): if `0x9202` is supported, call `SDIO_Connect(1,0,0)`, `SDIO_Connect(2,0,0)`, fetch vendor prop codes via `0x9202`, then `SDIO_Connect(3,0,0)`; QX cameras use `QX_Connect(1/2/3, 0xda01, 0xda01)` — [libgphoto2 camlibs/ptp2/library.c](https://github.com/gphoto/libgphoto2/blob/master/camlibs/ptp2/library.c)
- The Rust port performs the identical three-phase handshake after `OpenSession`/`GetDeviceInfo`: `SDIO_Connect [1,0,0]`, `SDIO_Connect [2,0,0]`, `SDIO_GetExtDeviceInfo [0x0000012C]` (data phase returns protocol version + supported property list), `SDIO_Connect [3,0,0]`, then `0x9209 GetAllExtDevicePropInfo` for the full property blob — [rocc-ptp lib.rs](https://github.com/Streetblock/remote-camera-control-rs/blob/main/crates/rocc-ptp/src/lib.rs)
- The Python reference (`jakkuh/sony-alpha-python`) uses the same `SDIO_CONNECT [1,0,0] -> [2,0,0] -> [3,0,0]` sequence and documents `SDIO_GET_EXT_DEVICE_PROP 0x9251` ("not supported on cinema cameras"); it names special object handles `LIVEVIEW_HANDLE 0xFFFFC002` and `STILL_IMAGE_HANDLE 0xFFFFC001`; live view frames are fetched with `GetObject(0xFFFFC002)` after one `GetObjectInfo(0xFFFFC002)`; additional modern property codes listed: `ZOOM_STEP 0xD2DD`, `ZOOM_SETTING 0xD25F`, `ZOOM_BAR_INFO 0xD25D`, `MOVIE_RECORDING 0xD2C8`, `AF_AREA_POSITION 0xD2DC` (u32: hi16 = X 0-639, lo16 = Y 0-479), `REMOTE_TOUCH 0xD2E4`, `SHUTTER_SPEED 0xD20D` (hi16 numerator/lo16 denominator; 0 = BULB), `ISO 0xD21E` (lower 24 bits value, upper byte mode), `MEDIA_SLOT1_REC_TIME 0xD24A`, `BATTERY_REMAINING 0xD218` — [jakkuh/sony-alpha-python sony_camera/protocol.py](https://github.com/jakkuh/sony-alpha-python/blob/main/sony_camera/protocol.py); [sony_camera/client.py](https://github.com/jakkuh/sony-alpha-python/blob/main/sony_camera/client.py)
- Alpha Fairy's author (ESP32 Wi-Fi remote) confirms the handshake has two parts: the documented PTP/IP init, then "a series of operation requests using Sony's own operation codes"; "the act of pairing is simply a delay after the host sends the initialization command request which contains the host's friendly name, the name is shown on the camera's screen and the user has to press OK ... The host's GUID is memorized by the camera so it won't be prompted for any future handshakes"; there is no way to unpair short of a factory reset, so the author recommends the camera's "Connect Without Pairing" option; property-description blobs changed format between the a6600 (PIMA 15740-conformant) and the a1 (new format with multiple enum lists per property) — [alpha-fairy doc/Camera-Reverse-Engineering.md](https://github.com/frank26080115/alpha-fairy/blob/main/doc/Camera-Reverse-Engineering.md)
- Sony's own public documentation now exists for this protocol: libgphoto2 2.5.31 notes say "Sony: Now officially documented by Sony. Changes imported from the documentation" — [libgphoto2 NEWS](https://github.com/gphoto/libgphoto2/blob/master/NEWS); Sony's "Camera Remote Command" toolkit (a "proprietary extension of the ISO standard PTP") ships a Command Reference and sample code, added "control over IP connection (PTP-IP)" in Ver. 2024.1.0 (2024-Oct-16), reached Ver. 2.02.00 on 2026-Jun-10, is free, but "only corporate customers can use it; personal users cannot download it", and warns that "Camera Control PTP 2 commands may become unusable on some models from 2027" — [Sony Camera Remote Command](https://support.d-imaging.sony.co.jp/app/cameraremotecommand/en/index.html)
- A copy of Sony's "Camera Control PTP 3 Reference.pdf" (3.1 MB) is checked into the `docs/` folder of the jakkuh/sony-alpha-python repository (redistribution status unclear) — [jakkuh/sony-alpha-python tree](https://github.com/jakkuh/sony-alpha-python)

**"Access Authen." = SSH**
- Sony's a7 IV help guide: "Access Authen. Settings" (firmware 2.00+) at MENU > Network > Network Option; when Off, "communication is performed without SSH connection authentication or encryption"; user name up to 16 chars, password 8-16 chars, both auto-set at purchase, with "Generate Password"; "If unauthorized access is detected, the camera may stop accepting communication" — [Sony Help Guide ILCE-7M4, Access Authen. Settings](https://helpguide.sony.net/ilc/2110/v1/en/contents/TP1000954812.html)
- Credentials (user, password, fingerprint) are shown on the camera under MENU > Network > [Network Option] > [Access Authen. Info] and are typed into Imaging Edge Desktop (Remote); remote control is not possible while the camera is connected to a smartphone; Sony recommends WPA2/WPA3 APs — [Sony Help Guide ILCE-7RM6, Remote Shoot Function](https://helpguide.sony.net/ilc/2620/v1/en/contents/0903_pc_remote_function.html)
- When Access Authen. is Off and the connection goes through a Wi-Fi access point, the camera and computer must be paired ("Pairing" item in the Remote Shoot Function menu, completed from Imaging Edge Desktop); pairing data is erased by camera initialisation; Sony recommends Access Authen. On — [Sony Help Guide ILCE-7RM5, Remote Shoot Function](https://helpguide.sony.net/ilc/2420/v1/en/contents/0903_pc_remote_function.html)
- Open-source confirmation of what is on the wire: `jakkuh/sony-alpha-python` connects with `sshpass -p <password> ssh -c aes128-ctr -N -L <localport>:localhost:15740 -o PreferredAuthentications=keyboard-interactive -o KbdInteractiveAuthentication=yes -o PubkeyAuthentication=no <user>@<camera-ip>` and then opens both PTP/IP command and event sockets to the forwarded local port; without credentials it connects directly to `camera:15740` — [sony_camera/connection.py](https://github.com/jakkuh/sony-alpha-python/blob/main/sony_camera/connection.py); README states it "uses the SSH authentication method, not the pairing method" and that SSH "was far more reliable in my testing", requires a "PTP 3"-compatible camera (FX30, A6700 given as examples), "Remote Shoot Function" enabled, same network — [jakkuh/sony-alpha-python README](https://github.com/jakkuh/sony-alpha-python)
- Capability advertisement: the camera's UPnP `DigitalImagingDesc.xml` contains `<X_PTP_Information>` with `X_PTP_Versions` (e.g. `3.00`), `X_PTP_PairingNecessity` (`Necessary`/`Unnecessary`), `X_PTP_MediaServerSupport`, `X_PTP_ContentsTransferSupport`, `X_PTP_RemoteControlSupport`, and `<X_ConnectionInfo><X_SSH_Support>` — [rocc-discovery xml.rs](https://github.com/Streetblock/remote-camera-control-rs/blob/main/crates/rocc-discovery/src/xml.rs)
- A third-party tool using Sony's official SDK ("sonshell") exposes a `--username` option "for cameras with Access Auth enabled", confirming the SDK path also consumes the same credentials — [goudvuur/sonshell](https://github.com/goudvuur/sonshell)

**libgphoto2 status on Sony PTP/IP**
- 2.5.9 (2015): "PTP/IP: fixes to support Ricoh Theta and Sony Alpha over WLAN better"; 2.5.26: "Added very experimental Wifi support" and "Modern Sony cameras seem to require a 3 second wait on startup (ZV-1, A7 R4, RX100 M7)"; 2.5.34 (released 2026-05-30): "Sony generation 2 vs 3 handling enhanced", Sony ILCE-1M2 ID added, multiple CVEs fixed in `ptp_unpack_Sony_DPD()` — [libgphoto2 NEWS](https://github.com/gphoto/libgphoto2/blob/master/NEWS); [libgphoto2 releases](https://github.com/gphoto/libgphoto2/releases)
- `ptpip_models[]` contains generic entries only ("PTP/IP Camera", Ricoh Theta, Nikon DSLR/1, Canon EOS, Fuji X (WLAN)); there is no Sony-specific PTP/IP model entry, so Sony Wi-Fi is used via the generic "PTP/IP Camera" model — [libgphoto2 library.c](https://github.com/gphoto/libgphoto2/blob/master/camlibs/ptp2/library.c)
- Issue #1219 (2026-03-14, DSC-RX100M6): "newer Sony cameras expect a vendor-specific connection initialization in PTP/IP mode to confirm the connection before any filesystem operations are allowed", otherwise the camera shows "Connecting..." and `GetObjectHandles` times out; Issue #1218: Sony expects a shutdown command at session end or the camera refuses further connections ("Shuts down automatically after save"); PR #1220 "Sony PTP/IP connection vendor-specific Initialization and De-initialization" was merged 2026-03-15 — [gphoto/libgphoto2#1219](https://github.com/gphoto/libgphoto2/issues/1219); [#1218](https://github.com/gphoto/libgphoto2/issues/1218); [PR #1220](https://github.com/gphoto/libgphoto2/pull/1220)
- Issue #1091 (open, 2025-02): ILCE-7M3 in "Send to Computer" mode over a phone hotspot fails with "bad type returned 5" (= PTPIP_INIT_FAIL) from an Android/iOS libgphoto2 build — [gphoto/libgphoto2#1091](https://github.com/gphoto/libgphoto2/issues/1091)
- Issue #946 (closed 2024-01): ILCE-7C described as "mostly non-functional"; #985/#1126/#1230 (2024-2026): ILCE-7M4 over USB — manual focus not implemented, capture trigger errors, Still Capture Mode property 0x5013 limits — [libgphoto2 issues search](https://github.com/gphoto/libgphoto2/issues?q=sony+ptpip)

**Legacy "Send to Computer" GUID authorization [LEGACY, pre-2020 PlayMemories Home flow]**
- In "Send to Computer" mode the camera validates only the GUID in Init_Command_Request, not the host name; PlayMemories Home sets the authorised host over USB (SCSI opcode 0x7A bulk-out, host name UTF-16 + MAC list); the GUID is `00..00 FF FF <6-byte MAC>`; later finding (2015-09-27): the camera accepts any GUID whose last six bytes are zero; each "Send to Computer" allows one MTP session — [airnef developer notes](https://horshack-dpreview.github.io/airnef/developer)
- `sony-pm-alt`: camera broadcasts SSDP to 239.255.255.250:1900 and runs PTP/IP on TCP 15740 in "Send to Computer" mode; GUID in `~/.gphoto/settings` must be `00:00:00:00:00:00:00:00:ff:ff:aa:bb:cc:dd:ee:ff` (MAC) or the value set by the bundled `sony-guid-setter` (libusb, `-g 054c:0994`); 23 non-standard packets (4 start/3 end) make the camera show "Sending..." and power off; last push 2021-12-01, 320 stars, no license file — [falk0069/sony-pm-alt](https://github.com/falk0069/sony-pm-alt)

### Inferences
- Imaging Edge Desktop Remote over Wi-Fi is therefore: SSDP/UPnP discovery (or user-entered IP) -> if `X_SSH_Support`/Access Authen. On: SSH (password, keyboard-interactive) with a local forward to 127.0.0.1:15740 on the camera -> standard PTP/IP Init_Command_Request (GUID + "friendly name") -> OpenSession -> `SDIO_Connect 1,2` -> `SDIO_GetExtDeviceInfo` -> `SDIO_Connect 3` -> `0x9209` property blob -> live view via `GetObject(0xFFFFC002)`, events on the second TCP connection. No source gives a byte-level capture of Imaging Edge itself, but three independent implementations (libgphoto2, rocc-ptp, sony-alpha-python) and Alpha Fairy's Wireshark-derived notes converge on this sequence.
- Because the SSH server is a stock-looking OpenSSH-style endpoint, any desktop language with an SSH client library (libssh2, Paramiko, russh, golang.org/x/crypto/ssh) can replicate Imaging Edge's Wi-Fi path without Sony's SDK. The SSH host key fingerprint displayed in "Access Authen. Info" is what a client should pin.
- "PTP 2" vs "PTP 3" in Sony's nomenclature maps onto libgphoto2's `sony_mode_ver == 2/3` (property-blob format change observed between a6600 and a1), and Sony says PTP 2 commands may stop working from 2027 — new code should target PTP 3 bodies.

### Gaps
- No public document describes the exact SSH server configuration on the camera (port is presumably 22 but no source states it; key exchange/cipher list unknown beyond the `aes128-ctr` the Python author forces).
- No published Wireshark capture of Imaging Edge Desktop's Wi-Fi session was found; the "pairing" (Access Authen. Off) variant's byte-level behaviour (beyond "GUID remembered after user presses OK") is only described anecdotally.
- Sony's official Camera Remote Command PDF is corporate-gated; the copy in the jakkuh repo could not be read in this research.

---

## Q2. Legacy Sony "Camera Remote API" (ScalarWebAPI JSON-RPC over HTTP) and which modern cameras lack it [LEGACY, pre-2020]

### Takeaway
The 2013-2017 Camera Remote API (SSDP discovery of `urn:schemas-sony-com:service:ScalarWebAPI:1`, device description at `http://<cam>:64321/dd.xml`, JSON-RPC at `http://<cam>:10000/sony/<service>`, MJPEG-ish live view at port 60152 or `/liveview`) is archived by Sony and absent from 2018+ bodies (a7 III/a7R III/a9 onward); those bodies only expose PTP/IP ("DigitalImaging" UPnP description) for control.

### Cited Findings
- SSDP: `M-SEARCH * HTTP/1.1` to 239.255.255.250:1900 with `ST: urn:schemas-sony-com:service:ScalarWebAPI:1`, response `LOCATION: http://192.168.122.1:64321/dd.xml`; the dd.xml carries `av:X_ScalarWebAPI_DeviceInfo` with `X_ScalarWebAPI_ServiceList` entries whose `X_ScalarWebAPI_ActionList_URL` is `http://<ip>:10000/sony` and a `X_ScalarWebAPI_LiveView_URL` such as `http://192.168.122.1:60152/liveviewstream?%211234%21%2a%3a%2a%3aimage%2fjpeg%3a%2a%21%21%21%21%21`; the same dd.xml lists `<SCPDURL>/DigitalImagingDesc.xml</SCPDURL>` for the PTP side — [rocc-discovery ssdp.rs](https://github.com/Streetblock/remote-camera-control-rs/blob/main/crates/rocc-discovery/src/ssdp.rs); [rocc-discovery xml.rs](https://github.com/Streetblock/remote-camera-control-rs/blob/main/crates/rocc-discovery/src/xml.rs)
- Community MCP server for the Camera Remote API defaults to camera IP 192.168.122.1, API port 10000, dd.xml port 64321, requires "Control with Smartphone" mode, tested on α6100 — [@neirth/sony-camera-mcp README](https://app.unpkg.com/@neirth/sony-camera-mcp@1.9.0/files/README.md)
- Sony archived the Camera Remote API page: "updates and active support for Camera Remote API at Developer World are no longer available", though "the API still functions as expected on its supported devices"; cameras supported by the Camera Remote API "are not included in compatible cameras' line-up for Camera Remote SDK" — [ProVideo Coalition on Camera Remote SDK](https://www.provideocoalition.com/camera-remote-sdk-for-sony-alpha-7r-iv-and-alpha-9-ii/); [Stack Overflow mirror: "Sony camera remote api archived"](https://kiwix.lema.org/stackoverflow.com_en_all_2023-11/questions/52010256/sony-camera-remote-api-archived)
- Alpha Fairy: "older cameras use a deprecated API and SDK ... totally proprietary protocol ... missing manual focusing (so focus stacking is impossible) ... plain JSON and fairly well documented"; its PTP path supports A1/A6600/A6000/RX100M4/RX0M2 and the HTTP path "supports more cameras but offers fewer features" — [alpha-fairy Camera-Reverse-Engineering.md](https://github.com/frank26080115/alpha-fairy/blob/main/doc/Camera-Reverse-Engineering.md); [alpha-fairy README](https://github.com/frank26080115/alpha-fairy)
- Cascable 3.4 (2018) added α7R III/α7 III/α9 but "Sony has removed several remote control features from these cameras, which affects the features Cascable is able to provide" — [sonyalpharumors: Cascable 3.4](https://www.sonyalpharumors.com/cascable-3-4-released-including-support-for-the-%ce%b17r-iii-%ce%b17-iii-%ce%b19-and-more/)
- The Rust rocc port routes a7 III via ScalarWebAPI and a6700 via DigitalImaging/PTP/IP, i.e. the a7 III still advertises ScalarWebAPI while the a6700 does not — [remote-camera-control-rs README](https://github.com/Streetblock/remote-camera-control-rs)
- pysony (`Bloodevil/sony_camera_api`, MIT, 288 stars) is the canonical Python client for the Camera Remote API; last push 2022-11-06 — [Bloodevil/sony_camera_api](https://github.com/Bloodevil/sony_camera_api); GitHub API metadata via `api.github.com/repos/Bloodevil/sony_camera_api`
- qDslrDashboard's Sony support is via the camera-side "Smart Remote" PlayMemories app (a6000/NEX era); users report a7 IV, a7S III and a9 cannot connect — [LRTimelapse forum thread](https://forum.lrtimelapse.com/Thread-can-qdslrdashboard-work-with-sony-a6300); [dslrdashboard.info Sony post](https://dslrdashboard.info/?p=347)

### Inferences
- "Port 8080" in the task prompt does not match any source; all primary material points to 10000 (JSON-RPC), 64321 (UPnP dd.xml), 60152 (live view stream) on legacy bodies. Treat 8080 as unverified.
- Every body in the "modern" list (a7 IV, a7R V, a1 II, a7C II, a6700, FX3, ZV-E1, a9 III) post-dates Sony's removal of ScalarWebAPI; for them only PTP/IP (or the Creators' App's own UPnP/SOAP transfer service, see Q5) is available over Wi-Fi.

### Gaps
- Sony's archived supported-camera list for the Camera Remote API could not be fetched (developer.sony.com and web.archive.org blocked); the a7 III's ScalarWebAPI exposure is confirmed only via rocc's validation, not Sony documentation.

---

## Q3. Bluetooth LE: how Creators' App / Imaging Edge Mobile pair, GATT services/characteristics, write formats

### Takeaway
Sony cameras advertise manufacturer data with company ID 0x012D and expose four 128-bit services in the `8000xx00-xx00-FFFF-FFFF-FFFFFFFFFFFF` family: `CC00` Camera Control (Wi-Fi handoff: SSID in CC06, password in CC07, Wi-Fi status notify CC05, FTP settings), `DD00` Location (95-byte GPS/time payload to DD11), `EE00` Pairing, and `FF00` Remote Control (16-bit button codes written to FF01, state notifications on FF02). The remote and location services are fully reverse engineered and shipped in several open-source projects; the pairing/Wi-Fi-handoff service is only partially documented.

### Cited Findings

**Advertising / discovery**
- Advertisement example `2D 01 03 00 64 00 45 31 22 EF 00`: `2D 01` = Sony company ID 0x012D; `03 00` = device type camera; `64` = protocol version; `45 31` = model code ("E1" e-mount, "A1" a-mount); `22 EF 00` = tag 0x22 status byte, where 0xEF = open for pairing, 0xAF = not pairing — [coral/freemote README](https://github.com/coral/freemote); [Hypoxic: Sony Camera BLE Control Protocol](https://gethypoxic.com/blogs/technical/sony-camera-ble-control-protocol-di-remote-control)
- Tag 0x22 bit mask: 0x80 PairingSupported, 0x40 PairingEnabled, 0x20 LocationFunctionSupported, 0x10 LocationFunctionEnabled, 0x08/0x04 unknown function supported/enabled, 0x02 RemoteFunctionEnabled; Tag 0x21: 0x80 WirelessPowerOnEnabled, 0x40 CameraOn, 0x20 WifiHandoverSupported, 0x10 WifiHandoverEnabled; a per-camera 128-bit UUID is advertised on the a7C (e.g. `8E3F7E15-AF80-1567-D946-A9190C2CF98A`) but not on the a7 IV — [Hypoxic](https://gethypoxic.com/blogs/technical/sony-camera-ble-control-protocol-di-remote-control)
- alphaloc filters on manufacturer company ID 0x012D with product ID 0x0003 and camera name prefix `SonyA7` — [finfinack/alphaloc](https://github.com/finfinack/alphaloc)

**Services and characteristics (Hypoxic, 2021-01-07, author Mark Kirschenbaum)**
- Camera Control `8000CC00-CC00-FFFF-FFFF-FFFFFFFFFFFF`: CC01 notify, CC02, CC05 Wi-Fi status notify, CC06 SSID, CC07 password, CC21 FTP status notify (a7C), CC22-CC2C FTP/Wi-Fi server writes (a7C), CC40 FTP server names; "the Sony app uses the Camera Control service for BLE-to-Wi-Fi handoff"; service is disabled while "Bluetooth Rmt Ctrl" is On
- Camera Location `8000DD00-DD00-FFFF-FFFF-FFFFFFFFFFFF`: DD01 notify, DD11 info (write), DD21 feature
- Pairing `8000EE00-EE00-FFFF-FFFF-FFFFFFFFFFFF`: EE01; untested commands `06 08 01 00 00 00` (pair), `06 08 02 00 00 00` (pair with disconnect), `03 08 13` (power off)
- Remote Control (DIRC) `8000FF00-FF00-FFFF-FFFF-FFFFFFFFFFFF`: FF01 RemoteCommand (write), FF02 RemoteNotify; protocol errors return GATT status 0x0185 — [Hypoxic](https://gethypoxic.com/blogs/technical/sony-camera-ble-control-protocol-di-remote-control)

**Remote shutter command set (write to FF01; format `[len 0x01|0x02][code][optional step]`)**
- Shutter half: 0x0106 up / 0x0107 down; shutter full: 0x0108 up / 0x0109 down; record: 0x010E up / 0x010F down; AF-On: 0x0114 up / 0x0115 down; C1: 0x0120 up / 0x0121 down; zoom tele release `0x0244[00-0F]` press `0x0245[10-8F]`; zoom wide release `0x0246` / press `0x0247`; focus in release `0x026A[00-0F]` / press `0x026B[00-7F]`; focus out `0x026C` / `0x026D`; shutter must be sequenced half-down, full-down, half-up, full-up or the camera can get stuck; scanning 0x00-0xFF found no hidden codes; notifications on FF02: focus `02 3F 00` lost / `02 3F 20` acquired, shutter `02 A0 00` ready / `02 A0 20` active, record `02 D5 00` stopped / `02 D5 20` started; tested on A7 III, nRF52840, Apache-2.0, 178 stars, pushed 2026-09-19 — [coral/freemote](https://github.com/coral/freemote)
- Original write-up (Greg Leeds, 2021-12-09, a6100, nRF sniffer + Wireshark): service `8000FF00-...`, write char 0xFF01; camera "does not respond to unpaired devices and disconnects immediately"; reliable photo sequence `0x0107, 0x0109, 0x0108, 0x0106`; the author moved to BLE because the camera's Wi-Fi API only worked in AP mode and "Send to Computer" (station mode) disables shooting — [gregleeds.com: Reverse Engineering Sony Camera Bluetooth](https://gregleeds.com/reverse-engineering-sony-camera-bluetooth/)

**Location (GPS) write format**
- Write to DD11 a 95-byte payload: `[0:1]` length 0x005D; `[2:4]` `08 02 FC`; `[5]` 0x03 to include TZ/DST else 0x00; `[6:10]` `00 00 10 10 10`; `[11:14]` latitude x 1e7 big-endian int32; `[15:18]` longitude x 1e7; `[19:20]` UTC year u16; `[21..25]` month, day, hour, minute, second; `[26:90]` zeros; `[91:92]` UTC offset minutes; `[93:94]` DST offset minutes; read DD21 flags first; DD30 = lock location (0/1), DD31 = enable location updates; enable FF02 notifications; camera must have "Location Info. Link Set." enabled; ESP32-C6/S3, Apache-2.0, pushed 2026-10-10 — [finfinack/alphaloc](https://github.com/finfinack/alphaloc)

**Pairing behaviour / platform notes**
- Creators' App pairing must be started from the app, not phone Bluetooth settings; iPhone shows a six-digit code to match on the camera; "The camera and Creators' App are automatically connected to Wi-Fi via the Bluetooth function"; up to 10 cameras, one active; VPN can block the connection — [Sony Creators' App Help: Connecting the camera (first-time)](https://helpguide.sony.net/di-app/ca/v1/en/contents/CA_connect_camera.html)
- "Cnct. during Power OFF" / "Cnct. while Power OFF": lets a paired smartphone power the camera on over Bluetooth from Imaging Edge Mobile / Creators' App; drains battery; reset to Off if pairing is cancelled — [Sony Help Guide ILCE-1, Cnct. during Power OFF](https://helpguide.sony.net/ilc/2040/v1/en/contents/TP1000425295.html)
- α-Remote (Android, GPL-3.0, F-Droid/Play, v1.4 released 2026-09-28) uses Android Companion Device pairing; confirmed on a6100/a6400/a6600/a6700/a7 III/a7C II/a7 IV/a7R III/a7R IV/a7R V/a9/ZV-E10; status feedback limited to focus/shutter/recording; "Bluetooth Rmt Ctrl" must be On and camera in pairing mode; a6400 cannot do BLE remote + BLE geotag simultaneously — [Staacks/alpharemote](https://github.com/Staacks/alpharemote); release metadata via GitHub API
- furble (ESP32/M5Stack, MIT, v3.9.1 2026-08-10, 258 stars) supports Sony shutter/focus/GPS, protocol derived from Android HCI snoop logs in Wireshark, tested on ZV-1F — [gkoh/furble](https://github.com/gkoh/furble)

### Inferences
- The "power on" command is not publicly known; only the untested `03 08 13` power-off and the tag-0x21 `WirelessPowerOnEnabled` bit are documented. A desktop could at best detect power-on capability, not exercise it, without further sniffing.
- Wi-Fi credential handoff via CC06/CC07 (SSID/password characteristics) is how the phone learns the camera's Wi-Fi Direct credentials without a QR code; a desktop with a BLE radio (macOS CoreBluetooth, Windows WinRT, Linux BlueZ) could in principle read them after bonding, but no open project has implemented CC00 and the bonding/encryption requirements are undocumented.
- BLE on desktop OSes is feasible (Linux BlueZ, macOS CoreBluetooth) but all current implementations are embedded or Android; bonding persistence and the 6-digit confirmation flow would need to be re-engineered on desktop.

### Gaps
- No source documents the EE00 pairing exchange that Creators' App performs (six-digit code), nor whether LE Secure Connections/passkey is required for the CC00 service.
- Byte format of CC05/CC06/CC07 reads/writes is not published; Hypoxic marks the FTP characteristics "unconfirmed".
- No open-source desktop (Linux/macOS/Windows) Sony BLE client was found.

---

## Q4. What the camera's smartphone-pairing QR code encodes

### Takeaway
Sony's documentation only states that scanning the QR code registers the camera's Wi-Fi Direct SSID (`DIRECT-xxxx`) and password on the phone; no public source (Sony or reverse-engineering) documenting the literal payload grammar (e.g. a `W01:S:<ssid>;P:<password>;C:<model>;` string) was found in this research.

### Cited Findings
- Sony Help Guide (ILCE-1, Android): "Once the QR Code has been read, this product's SSID (DIRECT-xxxx) and password are registered to the smartphone", so later connections can be made by selecting that SSID; if the SSID/password are reset the previously scanned code stops working — [Sony Help Guide ILCE-1, Connecting using the QR Code](https://helpguide.sony.net/ilc/2040/v1/en/contents/TP1000416941.html); [Sony Help Guide ZV-1A](https://helpguide.sony.net/dc/2410/v1/en/contents/241H_qrcode_android.html)
- Sony SSID length is up to 32 characters and password 8-64 characters for manual entry — [Sony FTP Help Guide: Connecting via wireless LAN](https://helpguide.sony.net/di/ftp_2420/v1/en/contents/WiFi_connection.html)
- A separate QR code exposes only the camera's MAC address, read by the Transfer & Tagging app — [Sony FTP Help Guide (ftp_2110)](https://helpguide.sony.net/di/ftp_2110/v1/en/contents/TP1000657891.html)
- Alpha Fairy's docs include a photo of a camera "Wi-Fi Info" screen and its own Wi-Fi-config QR, implying the author connected the ESP32 by reading the camera's SSID/password manually rather than parsing Sony's QR — [alpha-fairy README](https://github.com/frank26080115/alpha-fairy)

### Inferences
- Since the phone-side result is only SSID + WPA2 password, a desktop app that decodes the QR via webcam only needs to extract those two fields and join the camera's AP (NetworkManager / CoreWLAN / WlanConnect), after which discovery proceeds exactly as for a phone (Q5). Whether the payload is Sony-proprietary or a standard `WIFI:S:...;P:...;;` string is unverified.

### Gaps
- No source confirming the `W01:S:...;P:...;C:...;` grammar from the prompt. Searches (`"W01:S:"`, `"W01"` + Sony QR, GitHub code search via grep.app) returned nothing. A decode of a real camera's QR with a generic reader is the only way to settle this.

---

## Q5. Wi-Fi modes (camera AP / infrastructure), 5 GHz, and how the phone finds the camera's IP

### Takeaway
In Wi-Fi Direct (camera-as-AP) mode the camera is 192.168.122.1 (older bodies 10.0.0.1 or 192.168.0.1) and advertises itself via SSDP/UPnP on 239.255.255.250:1900 with a device description at `http://<ip>:64321/dd.xml` (plus `DigitalImagingDesc.xml` / `DmsDescPush.xml`); in infrastructure mode the phone/desktop must discover it the same way (SSDP) or know its DHCP address. Many 2020+ bodies support a 5 GHz Wi-Fi Direct band; Creators' App's Bluetooth handoff is what makes the phone flow "automatic".

### Cited Findings
- SSDP responses from Sony cameras: `LOCATION: http://192.168.122.1:64321/dd.xml` (also `192.168.0.1` in test fixtures); the `rocc-discovery` crate searches for both `urn:schemas-sony-com:service:ScalarWebAPI:1` and a DigitalImaging URN — [rocc-discovery ssdp.rs](https://github.com/Streetblock/remote-camera-control-rs/blob/main/crates/rocc-discovery/src/ssdp.rs); [rocc-discovery lib.rs](https://github.com/Streetblock/remote-camera-control-rs/blob/main/crates/rocc-discovery/src/lib.rs)
- "Send to Smartphone" transfer path used by Imaging Edge Mobile is SOAP/UPnP: device description `http://192.168.122.1:64321/DmsDescPush.xml` -> `XPlsDesc.xml` listing transfer-control actions; `TransferStart`/`TransferEnd` work, `TransferProgress` is a no-op; no PTP GUID needed; RAW cannot be fetched (JPEG only); tested ILCA-77M2, ILCE-7S, ILCE-6000/6400/6600, HX90V/HX99, RX100 III/VI/VII, WX300/WX350 (WX350 at 10.0.0.1), RX10 IV; GPL-3.0; created 2024-04-30, pushed 2026-10-18 — [schorschii/ImagingEdge4Linux](https://github.com/schorschii/ImagingEdge4Linux)
- A NEX-6 hobbyist note shows UPnP ContentDirectory `Browse` via SOAP POST on port 64321 at 10.0.0.1 — [scrapbox: NEX-6 UDP Client](https://scrapbox.io/p4ken/NEX-6_UDP_Client)
- Camera in "Send to Computer" (station) mode broadcasts SSDP NOTIFY to 239.255.255.250:1900; find its DHCP IP from the router or `tcpdump udp port 1900` — [falk0069/sony-pm-alt](https://github.com/falk0069/sony-pm-alt)
- PC Remote / Remote Shoot connection methods on current bodies: USB, Wi-Fi Direct (camera shows SSID/password under [Wi-Fi Direct Info.]), Wi-Fi Access Point (camera and PC on same AP; pairing or Access Authen. required), and on a9 II/a1-class bodies Wired LAN — [Sony Help Guide ILCE-7RM6](https://helpguide.sony.net/ilc/2620/v1/en/contents/0903_pc_remote_function.html); [Sony Help Guide ILCE-7RM5](https://helpguide.sony.net/ilc/2420/v1/en/contents/0903_pc_remote_function.html)
- [Wi-Fi Frequency Band] (2.4 GHz / 5 GHz) applies to Wi-Fi Direct connections with a smartphone or computer on models that support 5 GHz; MENU > Network > Wi-Fi > Wi-Fi Frequency Band — [Sony Help Guide ILCE-7M4, Wi-Fi Frequency Band](https://helpguide.sony.net/ilc/2110/v1/en/contents/TP1000656751.html)
- Sony's Monitor & Control support page recommends 5 GHz where supported (less interference, faster), notes ILME-FX6T lacks 5 GHz, and says some models need Access Authentication turned off for Monitor & Control use — [Sony Creators' Cloud support CCMC99008](https://www.sony.si/electronics/support/e-mount-body-zv-e-series/zv-e1/articles/CCMC99008)
- Alpha Fairy (ESP32) supports only 2.4 GHz; the camera does not re-establish a dropped connection — [alpha-fairy README](https://github.com/frank26080115/alpha-fairy)
- Greg Leeds: the Wi-Fi control API worked only with the camera in AP mode; in "Send to Computer" (station) mode the camera joined his home network but shooting was disabled (a6100, 2021) — [gregleeds.com](https://gregleeds.com/reverse-engineering-sony-camera-bluetooth/)
- Bluetooth and Wi-Fi share 2.4 GHz; Sony suggests disabling phone Bluetooth if Wi-Fi is unstable — [Sony Creators' App Help](https://helpguide.sony.net/di-app/ca/v1/en/contents/CA_connect_camera.html)

### Inferences
- A desktop app should: (1) join the camera's `DIRECT-xxxx` AP or share its infrastructure network, (2) send an SSDP M-SEARCH for `urn:schemas-sony-com:service:ScalarWebAPI:1` and the DigitalImaging URN (or listen for NOTIFY), (3) GET `dd.xml` and `DigitalImagingDesc.xml` to read `X_PTP_Versions`, `X_PTP_PairingNecessity`, `X_PTP_RemoteControlSupport`, `X_SSH_Support`, (4) choose SSH-tunnelled or direct PTP/IP. Falling back to 192.168.122.1 when SSDP fails is reasonable for Wi-Fi Direct.
- No evidence of mDNS/Bonjour advertisement by Sony cameras was found; discovery is UPnP/SSDP only.

### Gaps
- The exact DigitalImaging SSDP search target string was not captured (the Rust code references a `UpnpServiceType::DigitalImaging.urn()` constant whose literal value was not in the grepped lines).
- No source quantifies 5 GHz Wi-Fi Direct throughput or lists per-model 5 GHz support.

---

## Q6. FTP push transfer from camera

### Takeaway
Current Alpha/FX bodies can push stills/proxies to up to nine registered FTP servers (FTP, FTPES, or SFTP; passive mode only), automatically during shooting or in batches; this is the simplest "camera -> desktop" image path because the desktop only needs to run an FTP/SFTP server on the same network.

### Cited Findings
- Up to nine FTP servers; host name max 64 chars; Sony recommends [Secure Protocol] = [On: FTPES] or [On: SFTP]; with SFTP the [Root Certificate Error] and [Passive Mode] settings are unavailable; the camera is not compatible with active-mode FTP — [Sony FTP Help Guide (ILCE-1 v2+), Registering an FTP server](https://helpguide.sony.net/di/ftp_2420/v1/en/contents/FTP_server_registration.html)
- FTP guide topics (ILCE-1 Ver.2.00+): wired LAN, wireless LAN, smartphone USB tethering, batch transfer, automatic transfer, single-image transfer from playback, which still/proxy files to transfer, protecting transferred files, FTP Power Save, root certificate update, save/load FTP settings — [Sony FTP Help Guide index](https://helpguide.sony.net/di/ftp_2420/v1/en/index.html)
- Root-certificate errors arise when a certificate from a different model or pre-update firmware is reused; forcing "Connect" is at the user's risk — [Sony support article 00336871](https://www.sony.ca/en/electronics/support/e-mount-body-ilce-1-series/ilce-1m2/articles/00336871)
- BLE Camera Control service exposes FTP-related characteristics (CC21 status, CC22-CC2C server writes, CC40 names) on ILCE-7C, and libgphoto2 lists PTP events `FTPSettingReadResult 0xC20A` / `FTPDisplayNameListChanged 0xC20C`, indicating FTP settings are also programmable over PTP and BLE — [Hypoxic](https://gethypoxic.com/blogs/technical/sony-camera-ble-control-protocol-di-remote-control); [libgphoto2 ptp.h](https://github.com/gphoto/libgphoto2/blob/master/camlibs/ptp2/ptp.h)
- Sony Camera Remote Command 2.00.00 (2025-06-23) added "background file transfer" and remote deletion over PTP, i.e. a non-FTP pull path also exists in the official PTP extension — [Sony Camera Remote Command](https://support.d-imaging.sony.co.jp/app/cameraremotecommand/en/index.html)

### Inferences
- FTP push is one-directional (no control) but works on every 2019+ body with the FTP function and requires zero reverse engineering; a desktop app can embed an SFTP server and configure the camera once (manually or via the phone "Transfer & Tagging" flow).

### Gaps
- Exact PTP opcodes/property codes for programming FTP server entries are not publicly enumerated.

---

## Q7. Open-source projects (status, scope, license, last activity)

### Takeaway
Three code bases implement the modern Sony PTP/IP control handshake outside Sony's SDK (libgphoto2 in C/LGPL, `remote-camera-control-rs` in Rust/Apache-2.0+MIT, `sony-alpha-python`), one implements SSH-tunnelled Access Authen. (`sony-alpha-python`), one runs PTP/IP on an ESP32 (`alpha-fairy`), several implement BLE remote/GPS (freemote, furble, alpharemote, alphaloc), and two implement the legacy "Send to Smartphone"/"Send to Computer" transfer paths (ImagingEdge4Linux, sony-pm-alt). Most PTP/IP projects are small (1-8 stars) and recent (2025-2026).

### Cited Findings (repo metadata from the GitHub REST API on 2026-11-10 unless noted)
| Project | Language / scope | Modern-body status | License | Last push |
|---|---|---|---|---|
| [gphoto/libgphoto2](https://github.com/gphoto/libgphoto2) | C; USB + PTP/IP; `--port ptpip:IP`; Sony SDIO handshake; v2.5.34 (2026-05-30) | Sony IDs through ILCE-1M2/ILCE-9M3/A7R V; PTP/IP vendor init/deinit fix merged 2026-03-15 (PR #1220); no SSH/Access Authen. support in code grepped | LGPL-2.1 | 2026-11-09 |
| [Streetblock/remote-camera-control-rs](https://github.com/Streetblock/remote-camera-control-rs) | Rust; SSDP + dd.xml/DigitalImagingDesc.xml; PTP/IP + SDIO; ScalarWebAPI; live view | Validated a6700 (PTP/IP: ISO/aperture/shutter/EV/WB writes) and a7 III (ScalarWebAPI); port of Swift `rocc` | Apache-2.0 / MIT | 2026-04-12 (created 2026-04-11) |
| [jakkuh/sony-alpha-python](https://github.com/jakkuh/sony-alpha-python) | Python; PTP/IP via SSH tunnel; zoom, record, live view, remote touch | FX30, A6700 ("PTP 3") | none stated | 2026-02-09 |
| [frank26080115/alpha-fairy](https://github.com/frank26080115/alpha-fairy) | ESP32 (M5StickC-Plus) C++; PTP/IP (Wireshark-derived) + legacy HTTP JSON + IR; 223 stars | A1, A6600, A6000, RX100M4, RX0M2 tested; A7SM3/A9M2/A7RM4/A7C/A7M4/ZV-E10 untested; 2.4 GHz only; uses "Connect Without Pairing" | MIT | 2026-11-07 |
| [watsonmw/alphawire](https://github.com/watsonmw/alphawire) | C + Python; USB (WIA/libusbk/IOKit/libusb) and TCP/IP PTP; live view; ImGui debugger; "PTPIP backend does not support SSL" | "pre-2020 and post-2020" Alphas, no model list | MIT | 2026-11-08 |
| [simonmitchell/rocc](https://github.com/simonmitchell/rocc) | Swift; Sony Camera Remote API + PTP/IP ("a9ii"); powers the "Camrote" iOS app; 148 stars | a7ii (API) and a9ii (PTP/IP) named | MIT | 2025-03-23 |
| [falk0069/sony-pm-alt](https://github.com/falk0069/sony-pm-alt) | Python + gphoto2; "Send to Computer" SSDP listener + GUID setter; 320 stars | ILCE-5000 era; GUID authorisation **[LEGACY]** | none | 2021-12-01 |
| [schorschii/ImagingEdge4Linux](https://github.com/schorschii/ImagingEdge4Linux) | Python; "Send to Smartphone" SOAP/UPnP at 192.168.122.1:64321 | a6400/a6600/RX100 VII tested; JPEG only | GPL-3.0 | 2026-10-18 |
| [Bloodevil/sony_camera_api](https://github.com/Bloodevil/sony_camera_api) (pysony) | Python; Camera Remote API JSON-RPC; 288 stars | **[LEGACY]** | MIT | 2022-11-06 |
| [Parrot-Developers/sequoia-ptpy](https://github.com/Parrot-Developers/sequoia-ptpy) | Python PTP with `ptpy/extensions/sony.py` (cited by alpha-fairy as a code source) | — | BSD-3-Clause | 2019-06-13 |
| [petabyt/libpict](https://github.com/petabyt/libpict) | C PTP client library (USB + IP) | Sony coverage not verified here | Apache-2.0 | 2026-11-05 |
| [malc0mn/ptp-ip](https://github.com/malc0mn/ptp-ip) | Go PTP/IP (Fuji X-T1 focus) | no Sony | MIT | 2025-04-28 |
| [coral/freemote](https://github.com/coral/freemote) | nRF52840 BLE remote; 178 stars | A7 III tested | Apache-2.0 | 2026-09-19 |
| [gkoh/furble](https://github.com/gkoh/furble) | ESP32/M5Stack BLE remote multi-brand; v3.9.1 2026-08-10; 258 stars | ZV-1F tested | MIT | 2026-09-14 |
| [Staacks/alpharemote](https://github.com/Staacks/alpharemote) | Android BLE remote; v1.4 2026-09-28; 150 stars | a6700/a7 IV/a7R V/a7C II confirmed | GPL-3.0 | 2026-11-01 |
| [finfinack/alphaloc](https://github.com/finfinack/alphaloc) | ESP32 BLE GPS pusher | "Sony Alpha" (A7 prefix) | Apache-2.0 | 2026-10-10 |
| [goudvuur/sonshell](https://github.com/goudvuur/sonshell) | Linux shell on top of Sony's official Camera Remote SDK; `--username` for Access Auth | SDK-supported bodies | see repo | — |

Additional findings:
- Wireshark ships a C PTP/IP dissector `epan/dissectors/packet-ptpip.c` (2013) — [Wireshark source mirror](https://gitea.osmocom.org/osmocom/wireshark/src/branch/master/epan/dissectors/packet-ptpip.c); `tmandys/mtp-tools` is a Lua PTP/MTP/PTP-IP/Fuji dissector whose vendor JSON descriptions are generated from libgphoto2's `ptp.h` (so Sony codes are available via that path), BSD/GPL mixed — [tmandys/mtp-tools](https://github.com/tmandys/mtp-tools)
- Alpha Fairy mirrors the public PTP spec PDFs (`pima15740-2000.pdf`, CIPA `DC-X005.pdf` PTP/IP draft) in `ext_docs/` — [alpha-fairy tree](https://github.com/frank26080115/alpha-fairy)
- Sony's official Camera Remote SDK v2.02.00 (2026-06-10) lists ILCE-1M2, ILCE-1, ILCE-9M3, ILCE-9M2, ILCE-7RM6, ILCE-7RM5, ILCE-7RM4(A), ILCE-7CR, ILCE-7SM3, ILCE-7M5, ILCE-7M4, ILCE-7CM2, ILCE-7C, ILCE-6700, FX3/FX3A/FX30/FX2/FX6, ZV-E1, ZV-E10M2, etc., supports USB, wired LAN and Wi-Fi (Wi-Fi added for ILCE-1/9M3/7SM3/7M4 in 2024), Windows 11 x86 only (no ARM), macOS 14.1+, Linux x86-64/ARMv7/ARMv8, free, commercial apps allowed — [Sony Camera Remote SDK](https://support.d-imaging.sony.co.jp/app/sdk/en/index.html)

### Inferences
- "sony-remote-ptp", "SonyAlphaControl", "pysony" (= Bloodevil/sony_camera_api) and "Shutter" from the prompt: only pysony was located; Simon Mitchell's open-source Sony stack is `rocc` and its app is "Camrote", not "Shutter". No project named "sony-remote-ptp" or "SonyAlphaControl" surfaced.
- The Rust and Python projects are the best starting points for a desktop implementation because they already combine SSDP discovery, the SDIO handshake, and (Python) the SSH tunnel; libgphoto2 remains the most battle-tested property decoder (`ptp_unpack_Sony_DPD`, generation 2 vs 3 handling).

### Gaps
- No Node.js or Go PTP/IP library with Sony SDIO support was found.
- Sony coverage of `petabyt/libpict` and `tmandys/mtp-tools` not verified beyond README.

---

## Q8. Commercial third-party apps with wireless Sony control and how they achieve it

### Takeaway
Capture One (since 22 / 15.3.0, 2022) and Cascable Studio offer Wi-Fi control of modern Sony bodies; Capture One's setup guide mirrors Imaging Edge's PC Remote/Access Authen. flow (strongly implying the official Camera Remote SDK or an equivalent PTP/IP+SSH implementation), Cascable historically used the Camera Remote API and now lists 51 Sony models including a7 IV/a7R V/a6700/a1 II/a9 III as compatible, while Smart Shooter and CamRanger achieve "wireless" via USB plus a Wi-Fi bridge box (Air Direct / CamRanger hardware).

### Cited Findings
- Capture One wireless tethering for Sony: supported A7R IV/IVa, A1, A7 IV, A7S III, A7C, FX3, ZV-E10, A9 II; connection via Wi-Fi Direct (join camera SSID), Wi-Fi Access Point, or wired LAN (A9 II/A1); "workflow is identical to USB"; on firmware 2.0 one guide says to disable the access authentication setting; Sony Remote app must not run concurrently; a7 IV users report "camera busy/rejected the connection" — [Capture One: Sony wireless tethering setup guide](https://support.captureone.com/hc/en-us/articles/5473534946461-Sony-wireless-tethering-setup-guide); [Capture One community: a7 IV busy/rejected](https://support.captureone.com/hc/de/community/posts/12745185658781-How-to-fix-the-problem-The-camera-could-not-be-attached-The-Sony-ILCE-7M4-is-currently-busy-rejected-the-connection-with-wireless-tethering-in-Capture-One-23); [Imaging Resource on Capture One update](https://www.imaging-resource.com/news/capture-one-announces-major-update/)
- Cascable compatibility (2026): 51 Sony models compatible with Cascable Studio including α1, α1 II, α9 III, α7 IV, α7 V, α7R V, α7C II, α7C R, α6700, FX3/FX3A/FX30, ILX-LR1, ZV-E10 II; Pro Webcam excludes α1 II, α9 III, α7 V, FX3/FX3A/FX30, ILX-LR1, ZV-E10 II; "features vary greatly between models"; cameras not listed may work over USB via fallback import — [compatibility.cascable.se/sony](https://compatibility.cascable.se/sony/)
- Cascable App Store listing: "Uses the Camera Remote API by Sony" (legacy path) and Wi-Fi/USB support for Alpha, HX, RX, NEX ranges — [Cascable Studio on the App Store](https://apps.apple.com/us/app/cascable-studio-tether-more/id974193500); 2018 note that Sony removed remote features from α7 III/α7R III/α9 — [sonyalpharumors](https://www.sonyalpharumors.com/cascable-3-4-released-including-support-for-the-%ce%b17r-iii-%ce%b17-iii-%ce%b19-and-more/)
- Smart Shooter: Sony support (shoot, AF, download, settings, live view, power zoom) over USB; "wireless" via Tether Tools Air Direct hardware (camera USB -> AIR_DIRECT Wi-Fi -> Air Direct Utility on PC) — [Tether Tools: Smart Shooter Sony updates](https://tethertools.com/blog/smart-shooter-4-updates-sony-cameras-compatibility-and-more/); [Tether Tools: Enhance your tethered workflow](https://tethertools.com/blog/enhance-your-tethered-workflow-with-smart-shooter-4/)
- qDslrDashboard: Sony only via camera-side "Smart Remote" app (PlayMemories Camera Apps era); a7 IV/a7S III/a9 reported not connecting — [LRTimelapse forum](https://forum.lrtimelapse.com/Thread-can-qdslrdashboard-work-with-sony-a6300); [dslrdashboard.info](https://dslrdashboard.info/?p=347)
- Sony's own Monitor & Control app (pro Wi-Fi monitoring) requires Access Authentication Off on some models — [Sony CCMC99008](https://www.sony.si/electronics/support/e-mount-body-zv-e-series/zv-e1/articles/CCMC99008)

### Inferences
- Capture One's guide instructing users to enter the camera's Access Authen. credentials and to disable Access Authen. on fw 2.0 matches the SSH-tunnelled PTP/IP path and the Camera Remote SDK's credential requirement; Capture One does not publicly state whether it uses Sony's SDK.
- Cascable's ability to list a1 II/a9 III/a7 V (bodies with no Camera Remote API) implies it implemented PTP/IP (or licensed Sony's SDK) for modern bodies; it does not disclose which.

### Gaps
- No vendor (Capture One, Cascable, Smart Shooter, Lumariver, digiCamControl, CamRanger) publicly states its Sony Wi-Fi transport; Lumariver and digiCamControl Sony Wi-Fi support were not found in searches.

---

## Key Question A. Is wireless PTP/IP to current bodies feasible with open source today, or does Sony's authentication block it?

### Takeaway
Feasible. On current bodies the gating mechanism is either (a) one-time GUID pairing (user presses OK on camera, or "Connect Without Pairing") or (b) SSH password auth whose credentials the camera displays; both are satisfied by open-source code (SSH tunnel + standard PTP/IP + SDIO handshake). Validated by open source on a6700 and FX30 (SSH path), a1/a6600 (ESP32 pairing path), and by libgphoto2 maintainers' 2026 fixes for RX100M6-class PTP/IP.

### Cited Findings
- SSH tunnel implementation and FX30/A6700 validation — [jakkuh/sony-alpha-python connection.py](https://github.com/jakkuh/sony-alpha-python/blob/main/sony_camera/connection.py)
- a6700 PTP/IP reads + ISO/aperture/shutter/EV/WB writes — [Streetblock/remote-camera-control-rs](https://github.com/Streetblock/remote-camera-control-rs)
- A1 PTP/IP via ESP32 with "Connect Without Pairing" — [alpha-fairy](https://github.com/frank26080115/alpha-fairy)
- Sony: Off = "without SSH connection authentication or encryption"; "if unauthorized access is detected, the camera may stop accepting communication" (lockout, cleared by reconnecting from the start) — [Sony Help Guide ILCE-7M4](https://helpguide.sony.net/ilc/2110/v1/en/contents/TP1000954812.html)
- libgphoto2 PR #1220 merged 2026-03-15 adding Sony PTP/IP vendor init/deinit — [PR #1220](https://github.com/gphoto/libgphoto2/pull/1220)

### Inferences
- Remaining practical risks: Sony lockout on repeated auth failures, inability to unpair GUIDs without factory reset, property-blob format differences between generations (need `sony_mode_ver`-style detection), 2.4 GHz interference with BLE, and Sony's stated intent to retire PTP 2 commands on some models from 2027.

### Gaps
- No open-source project reports success on a1 II, a9 III, a7R V, ZV-E1 specifically over PTP/IP (only BLE for a7R V via alpharemote); these are SDK-supported so the protocol is presumably the same, but unverified.

## Key Question B. Can a desktop replicate the phone's QR flow?
See Q4: the QR yields SSID+password; a desktop can scan it with a webcam and join the AP, then run SSDP discovery. Payload grammar unverified (Gap).

## Key Question C. BLE GATT UUIDs and write formats
See Q3: remote `8000FF00-FF00-FFFF-FFFF-FFFFFFFFFFFF` / FF01 / FF02 with 16-bit codes; location `8000DD00-...` / DD11 95-byte payload; pairing `8000EE00-...` / EE01 (untested commands); camera-control `8000CC00-...` CC05/CC06/CC07 for Wi-Fi handoff (formats undocumented).

## Key Question D. What is achievable on desktop without the SDK vs. what needs it

### Takeaway
Without the SDK: discovery, SSH/PTP/IP connection, full property read/write (exposure, focus mode, WB, zoom on PZ lenses), shutter/record/AF triggers, live view JPEG stream, image download, FTP push, and BLE shutter/GPS. With the SDK (or Sony's corporate-only Camera Remote Command docs): guaranteed coverage of 300+ newer commands (OSD emulation, background transfer, remote firmware update, pan/tilt), per-model quirks, and vendor support; some newest-model behaviours (e.g. ILCE-7M4 capture-mode limits seen in libgphoto2) may be unimplemented in open source.

### Cited Findings
- Open-source feature coverage: property get/set, live view, capture, download (libgphoto2; alphawire; rocc-ptp; sony-alpha-python) — [libgphoto2 NEWS](https://github.com/gphoto/libgphoto2/blob/master/NEWS); [watsonmw/alphawire](https://github.com/watsonmw/alphawire); [remote-camera-control-rs](https://github.com/Streetblock/remote-camera-control-rs); [jakkuh/sony-alpha-python](https://github.com/jakkuh/sony-alpha-python)
- SDK/Command toolkit exclusives: 2.00.00 added "over 300 command types", OSD emulation, background file transfer, remote firmware update, deletion, pan/tilt (BRC-AM7/FR7) — [Sony Camera Remote Command](https://support.d-imaging.sony.co.jp/app/cameraremotecommand/en/index.html)
- Open-source pain points: shutter-speed changes "unreliable" over Wi-Fi, no reconnection by camera, card-buffer stall unless "Still Image Save Destination = Camera Only", APS-C mode disables many PTP commands, manual-focus mode can lock out physical controls — [alpha-fairy Camera-Reverse-Engineering.md](https://github.com/frank26080115/alpha-fairy/blob/main/doc/Camera-Reverse-Engineering.md)
- libgphoto2 open issues on ILCE-7M4: manual focus not implemented (#985), capture trigger failures (#1126), Still Capture Mode 0x5013 only Single Shot (#1230) — [libgphoto2 issues](https://github.com/gphoto/libgphoto2/issues?q=sony+ptpip)
- SDK platform limits: Windows 11 x86/AMD only (no ARM), macOS 14.1+, Linux x86-64/ARMv7/ARMv8 — [Sony Camera Remote SDK](https://support.d-imaging.sony.co.jp/app/sdk/en/index.html)

### Inferences
- A pragmatic desktop architecture: use the SDK where available (official Wi-Fi support exists for a1/a9 III/a7S III/a7 IV and later) and a reverse-engineered SSH+PTP/IP path for platforms the SDK does not cover (Windows on ARM, older bodies, embedded) or for features like BLE power-on/GPS that the SDK does not expose.

### Gaps
- No side-by-side feature matrix of SDK vs. open-source for each 2021+ body exists; the SDK's per-model interface table lives in its gated README/API reference.
