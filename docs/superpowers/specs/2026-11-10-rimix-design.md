# Rimix design document

Date: 2026-11-10. Status: draft for owner approval.

Rimix is an open-source desktop app, macOS first and Windows second, that connects to Sony Alpha cameras over Wi-Fi and gives the photographer live view, remote shooting, camera settings and image transfer from a laptop, the way Sony's phone app does on a phone. The owner's own camera is a Sony ZV-E10 (first generation, firmware 2.00), and the first working version must work with it.

## 1. Intent and why

Sony's desktop software is USB-first. The phone app pairs wirelessly in seconds; the desktop app makes you read a Wi-Fi name and password off the camera screen, join the network by hand, and type credentials. The research (reports/Sony camera wireless desktop control.md) established that the camera-side protocol is the same for both: every 2019-and-later Alpha runs a PTP/IP server on TCP port 15740 that any computer on the same Wi-Fi can talk to. What Sony withheld from the desktop is the onboarding, not the control. Rimix's job is to make the first ten seconds (find the camera, join it, trust it, see live view) as easy as the phone, and then to be a better shooting companion than Imaging Edge Desktop.

## 2. Goals, non-goals, anti-goals

Goals, in priority order:
1. Connect to the owner's ZV-E10 over Wi-Fi (both Wi-Fi Direct and a shared router) without a cable, show live view, change exposure settings, take a photo, start and stop video, and receive the captured files on the laptop.
2. Onboard without typing: scan the camera's QR code with the laptop webcam, or pick the camera from a list of nearby cameras, or as a fallback type an IP address. Remember the camera so the next connection is one click.
3. Save captures where the photographer wants them: a custom folder, a Lightroom watched folder, or a new folder per date.
4. Work on more Sony bodies than just the ZV-E10 over time, including the 2022-and-later "Access Authentication" bodies (a7 IV, a7R V, a6700, a7C II, ZV-E1, FX3 and others).
5. Later phases: multi-camera, video scopes (waveform, false colour, zebra), interval timer, Bluetooth onboarding and wake.

Non-goals (not planned):
- Cloud upload to Sony's Creators' Cloud, livestreaming setup, firmware updates. These stay in Sony's apps.
- USB tethering. Wi-Fi is the point. USB may come later only if trivial.
- Cameras older than 2019 (a7 III, a6400, a6600, a6000 and the legacy JSON API). They have no Wi-Fi remote-control server for computers.
- Mac App Store distribution. Rimix ships as a direct download (DMG, Windows installer) from GitHub and a website.
- Using Sony's Camera Remote SDK. It does not support the ZV-E10 at all, its licence forbids redistribution (which conflicts with open source), and it would force a C++ sidecar. Rimix speaks the protocol itself.

Anti-goals (would make it a failure even if it worked):
- Needing a cable, a driver, or admin rights.
- Losing the connection on every sleep/wake or Wi-Fi hiccup without recovering by itself.
- Hiding camera settings behind menus. The chosen design shows every exposure value on screen at all times.
- Being heavy. No Electron. Target under 30 MB installed on macOS.

## 3. Constraints

- Tech stack: Tauri v2 with a Rust core and a web front end. The owner wants web-stack UI, not native Swift/WinUI, and not Electron.
- Open source under a permissive licence. Code may be studied from Apache-2.0 and MIT projects (alpha2go, remote-camera-control-rs, alphawire, SonyCamViewer's documentation); nothing may be copied from GPL projects (alpharemote, ImagingEdge4Linux, camera-wifi-transfer).
- Platforms: macOS 14 and later on Apple silicon and Intel first; Windows 11 second; Linux is not a target but nothing should rule it out.
- Signing: macOS Developer ID with notarization and hardened runtime (the owner needs an Apple Developer account before the first public DMG). Windows signing is deferred until there is a Windows build.
- The protocol is reverse-engineered and Sony has said its older "PTP 2" command set may stop working on some models from 2027. The code must isolate per-model behaviour so a quirk in one body does not break others.

## 4. What the owner's camera can do (ZV-E10, firmware 2.00)

- Imaging Edge Desktop lists it for USB, Wi-Fi Direct and Wi-Fi access point mode, so the camera has the PTP/IP remote server.
- It is a "generation 3" body: wireless connections are authorised by GUID pairing (the camera shows the computer's name and asks the user to press OK once; it remembers the GUID), not by SSH username/password. Pairing cannot be undone without a camera reset, so the app must generate one stable GUID per installation and keep it.
- Its smartphone QR code (Menu, Network, Send to Smartphone, show QR) encodes the camera's Wi-Fi Direct name (DIRECT-xxxx) and password. This is what the webcam scan reads.
- In Wi-Fi Direct the camera is at 192.168.122.1 and is also the default gateway. On a router it gets a DHCP address and is found via SSDP (UDP 1900) and its UPnP description at http://camera:64321/dd.xml.
- Bluetooth: the camera supports Sony's BLE remote protocol (shutter, focus, zoom) and advertises with Sony company ID 0x012D. Credential handoff over BLE is documented for newer bodies; for the ZV-E10 it must be tested.
- Not supported by Sony's SDK, and Creators' App refuses it; Imaging Edge Mobile is Sony's phone app for it.

## 5. Decided: the connection and protocol layer

Rimix implements Sony's PTP/IP dialect in Rust inside the Tauri core, as a standalone crate (working name `sony-ptpip`) with no dependency on the UI so it can be tested from a CLI.

Connection flow:
1. Discovery. Send an SSDP M-SEARCH and listen for Sony cameras; fetch dd.xml and DigitalImagingDesc.xml to learn model name, PTP version, whether pairing is required and whether SSH (Access Authentication) is supported. Also probe the default gateway on port 15740 (covers Wi-Fi Direct instantly) and allow a manual IP.
2. Transport. Plain TCP to port 15740 for pairing-generation bodies. For Access Authentication bodies, open an SSH connection to the camera with the username and password from the camera screen, verify the host-key fingerprint against what the user confirms on screen (and cache it), and forward a local port to the camera's 15740.
3. PTP/IP session. Init_Command_Request with Rimix's stable GUID and a friendly host name, Init_Event_Request on a second TCP connection, OpenSession, then Sony's SDIO handshake (operations 0x9201 phases 1 to 3, 0x9202, 0x9209) to get the full property set.
4. Control. Read and write device properties (0x9205), press controls (0x9207), poll live view JPEG frames with GetObject(0xFFFFC002), receive new-object events and download captures (full-resolution RAW and JPEG per the camera's format setting) to the chosen folder.
5. Keepalive and reconnect. Detect dropped sockets within a few seconds, reconnect automatically with backoff, re-pair silently (same GUID), and resume live view without user action. Survive laptop sleep/wake.

Wi-Fi joining. The app tries to join the camera's Wi-Fi Direct network programmatically (macOS CoreWLAN from the main process, with Location permission; Windows WiFiAdapter). If the OS refuses, it shows the network name and password it decoded and asks the user to join manually, then continues automatically once the camera becomes reachable. Shared-router mode needs no joining.

Onboarding go/no-go gate (first experiment, before any UI work). The ZV-E10 shows its QR code under Send to Smartphone, while PC Remote has its own Wi-Fi Direct Info screen. It is not yet known whether the network the QR describes is the same one on which the camera serves PTP/IP on port 15740. The first task is a fifteen-minute test with the camera: show the QR, decode it, join that network, probe 15740. If it works, QR onboarding proceeds as designed. If not, QR onboarding is replaced for this body by a guided screen ("On the camera: Menu, Network, PC Remote Function, Wi-Fi Direct Info" plus two fields for the name and password shown there), and the Bluetooth credential handoff moves from Phase 4 to Phase 3 as the zero-typing path.

Pairing fallback. Pairing is the normal path and the app, the CLI and any test tool share one stored GUID so the camera sees a single host. If the ZV-E10 refuses GUID pairing from a non-Sony host, the crate must support the "Connect without pairing" variant of the handshake documented in alpha-fairy's reverse-engineering notes; design the handshake so both are selectable from the start.

Per-model quirks live in a small table keyed by model name and PTP version (for example, generation-3 bodies step exposure values rather than accept direct writes; some bodies reject certain operations). The ZV-E10 row is filled first from testing.

Bluetooth (phase 4, optional). Scan for Sony advertisements to show "ZV-E10 nearby, pairing enabled"; attempt the credential handoff (service 8000CC00, characteristics CC06 SSID and CC07 password) if the ZV-E10 supports it; BLE shutter as a bonus. BLE wake is only in scope if the power-on command can be found; the research did not.

What the camera layer explicitly does not do: Sony's SDK, the legacy JSON API, cloud, firmware.

## 6. Decided: the app and its screens

### 6.1 Design reference

The approved visual design is the interactive mockup design/mockups/20-rimix-final.html, built from three approved explorations: 13 Glass Dock (the base), 16 Glass Daylight (the light theme) and 15 Glass Ring (the alternative layout). The brief for the content and the glass material is design/mockups/brief.md. The builder should treat the mockup as the layout and styling specification and port its CSS tokens.

Design language in one paragraph: the live view fills the whole window and every control floats over it in Apple-style frosted glass (backdrop blur with saturation, a 1px specular highlight on the top edge, a soft lift shadow, generous concentric rounding). Hierarchy comes from opacity, not colour; the only accents are recording red and focus-lock green. Typeface is Inter with system-font fallbacks and tabular numerals for values. Text over footage carries a subtle shadow for legibility.

### 6.2 Main screen (dock layout, the default)

- Live view edge to edge with overlays: focus frame (green when locked), optional thirds grid, level indicator, small histogram, status capsule ("Live view 24 fps, last shot 0.8 s ago").
- Top bar: app name, camera name, connection type and signal, battery, card slot and shots left, firmware, and a gear button for App settings.
- One floating glass dock bottom-centre: ten exposure chips in a row (Mode, Shutter, Aperture, ISO, Exposure comp, White balance, Focus, Drive, Format, Picture profile); below them presets C1 to C4 as a segmented control, Record, a large round Shoot button, Focus. Clicking a chip opens the camera settings popover anchored to it.
- Camera settings popover: the stacked-list pattern from mockup 06 (label left, value right, inline minus/plus stepper for numeric values, chevrons or segmented control for choices, hairline separators, groups Exposure, Image, Focus and drive). Changing a value sends it to the camera immediately and the chip updates when the camera confirms.
- Left side, quieter glass panels: Scopes and timer (Waveform, False colour, Zebra toggles, Interval timer), Cameras (connected and offline), Save to (path plus the three destination options).
- Right side: vertical filmstrip of the last captures with transfer progress on the newest.
- Keyboard: Space shoots, R toggles record, F focuses, arrows adjust the selected chip, Escape closes popovers, Cmd/Ctrl+comma opens App settings.

### 6.3 Ring layout (alternative)

Same panels, but the dock is replaced by a lens-style control ring bottom-right: Shoot in the hub, Record and Focus as inner arcs, the ten exposure values around the rim; selecting a value opens the same camera settings panel beside the ring with that row highlighted. Users choose it in App settings.

### 6.4 Themes

Dark (default, mockup 13's look), Light (mockup 16's frosted-white look with dark text), and System (follow the OS). All colours are CSS custom properties on the root element; both themes must be complete, and record red and focus-lock green are shared.

### 6.5 App settings versus camera settings

These are two different things and must not be mixed in the UI. Camera settings are the camera's own properties (exposure, white balance, drive) and live in the chip popover or ring panel. App settings are Rimix's preferences and live behind the gear button: Appearance (Dark, Light, System), Layout (Dock, Ring), Save to defaults, webcam used for QR scanning, Wi-Fi join behaviour (automatic or manual), and the list of remembered cameras. App settings persist on disk.

### 6.6 Connect screen

Shown when no camera is connected, in the same glass language. Headline "Connect a camera". Three equal ways: Scan the QR code (webcam preview, with the on-camera menu path as a hint), Nearby (list of discovered cameras over SSDP and, later, Bluetooth, each with a Connect button), Enter the camera's address (IP field prefilled 192.168.122.1). A note that first-time pairing requires pressing OK on the camera. A "Recently used" list for one-click reconnect. Errors say what happened and what to do ("The camera asked you to allow this computer. Press OK on the camera, then try again.").

### 6.7 Captures and destinations

Each capture is downloaded as the camera's output (RAW+JPEG when set) into the destination. Three modes, selectable on the main screen and defaulted in App settings: Custom folder (user picks a folder once), Lightroom watched folder (user points at Lightroom's auto-import folder; files are written atomically with a temporary name then renamed so Lightroom never sees a half file), New folder per date (a YYYY-MM-DD folder under the chosen base). Filenames keep the camera's names (DSC01234.ARW). Transfer progress shows on the filmstrip tile; failed transfers are retried and listed.

Stills only in phases 1 to 3. The camera pushes new stills to the computer during remote shooting, but it does not push movie files; getting an MP4 off the card requires the camera's separate content-transfer mode, in which it cannot be controlled. Video recording (start and stop) is in Phase 1; video file retrieval is a Phase 4 feature ("Browse card": switch mode, list clips, resumable download of multi-gigabyte files, switch back) and is out of scope until then.

### 6.8 Presets

A preset is an app-side bundle of camera property values (for example Mode M, 1/125, f/4.0, ISO 800, white balance 5200K, AF-C, RAW+JPEG) with a user-given name such as "Talking head". Rimix stores presets in App settings, per camera model. Selecting one writes each value to the camera in order, using step commands where the body does not accept direct writes, and shows progress on the preset control until the camera confirms. Four slots C1 to C4 are shown on the dock; "Save current settings as preset" and "Rename" live in the preset control's menu. Presets are a Phase 2 feature; in Phase 1 the slots are visible but inert. They are not the camera's own memory-recall registers.

## 7. Phases

Phase 1, connect and shoot (the owner's ZV-E10 only): the onboarding go/no-go test and the pairing test first, then the Rust crate with discovery, pairing, session, properties, live view, capture download; a CLI that proves each step; Tauri app with the dock layout in dark theme, manual IP and SSDP connect, camera settings popover, Save to with the three modes, filmstrip, auto-reconnect. Done when the owner can shoot a RAW+JPEG wirelessly from the laptop and find it in a dated folder.

Phase 2, onboarding and polish: webcam QR scan (or the guided fallback), Wi-Fi Direct auto-join with manual fallback, remembered cameras, presets, light and system themes, ring layout, App settings, keyboard shortcuts, DMG with notarization, GitHub release.

Phase 3, more cameras and more shooting tools: Access Authentication (SSH) for 2022-and-later bodies, per-model quirk table, interval timer, Windows build and installer.

Phase 4, pro features: multi-camera (connect several, switch the live view, shoot all), video scopes drawn client-side from the live view frames (waveform, false colour, zebra), Bluetooth nearby/handoff/shutter (earlier if the QR gate fails), card browsing with video file download, SFTP receiver so the camera can push files when control is not needed.

## 8. Left to the builder

- Rust crate choices (async runtime, SSH library, SSDP, BLE), front-end framework inside Tauri (plain TypeScript, Svelte or React are all acceptable), state management, and how live view frames cross the Tauri boundary (binary channel, not JSON).
- Test strategy, including a fake camera for CI (SonyCamViewer and Tether both test against simulated cameras).
- Exact packaging, update mechanism, crash reporting (none is acceptable for v1).
- Repository layout and CI.

## 9. Calls made in this document that the owner should check

- Licence: Apache-2.0 (my call). It matches the Rust and Go projects we will study and allows others to build on Rimix commercially.
- Default destination mode is New folder per date (my call; the owner listed it among the options).
- Keyboard shortcuts in 6.2 (my call).
- Phase order, in particular putting QR scan in phase 2 rather than phase 1, so the first milestone is purely "does the protocol work" (my call).
- Not supporting USB at all in v1 (follows from the owner's goal; confirm).
- Video files are not transferred until Phase 4; only stills are received automatically (my call, see 6.7).
- Presets are app-side named bundles, not the camera's memory registers, and arrive in Phase 2 (my call, see 6.8).
- If the QR code's network turns out not to carry the remote-control server, QR is replaced by a guided screen for this body and Bluetooth handoff moves up a phase (my call, see section 5).

## 10. Open questions to settle by experiment with the camera in hand

- What the ZV-E10's QR code literally encodes, and whether the network it describes serves PTP/IP on port 15740 (the go/no-go gate in section 5).
- Whether the ZV-E10 accepts GUID pairing from a non-Sony host without "Connect without pairing" tricks, and what the camera shows during pairing.
- Which exposure properties the ZV-E10 accepts as direct writes versus step commands.
- Whether CoreWLAN can join the camera's Wi-Fi Direct network from a notarized Tauri app on the owner's macOS version, and which permission prompts appear.
- Whether the ZV-E10 exposes the BLE credential-handoff characteristics.
