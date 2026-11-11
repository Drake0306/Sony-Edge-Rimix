# Rimix: hand-off notes for the next session

Roy stopped work on 2026-11-11 with the words "Let's release a build, and
we'll continue the work later after it is released", and asked for
everything to be written here so it can be picked up at any time. Read
"Where we stopped" and the rules under it first; everything after that is
the history behind it, oldest first, and is still true unless the recap
says otherwise. The user is Roy; the camera is a Sony ZV-E10 (firmware
2.01, with a power zoom lens) on a Mac running macOS 27.

## Where we stopped (2026-11-11, evening): read this first

Ten real-camera sessions happened on 2026-11-11, each followed by a build.
Nothing is half done: the working tree is clean and `main` is pushed.

**Version 0.1.0 is built and sits on GitHub as a DRAFT release. It is NOT
published yet.** Tag `v0.1.0` is on `8494772` ("Merge release-0.1.0").
The draft "Rimix 0.1.0" holds `Rimix_0.1.0_universal.dmg` and
`Rimix_0.1.0_x64-setup.exe`, with notes that say what was and was not
tried on a camera. Claude's permission system refused the publish command,
so the last step is Roy's: the "Publish release" button on
`github.com/Drake0306/Sony-Edge-Rimix/releases`, or
`gh release edit v0.1.0 --repo Drake0306/Sony-Edge-Rimix --draft=false --latest`.
FIRST THING next session: `gh release list --repo Drake0306/Sony-Edge-Rimix`
says whether it is still "Draft". See "CI and releasing".

**The build Roy has** is `~/Downloads/Rimix-0.1.0/` (`Rimix.app`, the DMG
and the CLI `rimix`; built 17:17 local from `43ad154`, Apple silicon,
signed ad hoc as a bundle, three `ok` from `check-mac-bundle.sh`). The
commits after `43ad154` change no behaviour (two lint fixes, README, this
file), so it was not rebuilt. The HTML reports of every round are in the
same folder; the last is `Files-zoom-and-fixes-report.html`.

**Where each thing stands on the ZV-E10.** The session headings below have
the detail and the evidence.

- *Proven on the camera:* connecting over its own Wi-Fi with the signed
  build; the parameterless 0x9209 polling (no timeouts); live view; taking
  pictures; writes of mode 0x500E (reported about 3 s late), aperture
  0x5007, ISO 0xD21E with Auto, focus area 0xD22C; the Auto buttons; the
  live view metadata layout (focus frames, level).
- *Built, never tried on the camera:* Browse files over Send to
  Smartphone (the most important one; it has met no real camera at all);
  zoom (W and T, Zoom setting, Switch to JPEG); tap to focus with the spot
  the camera lists (session 9's fix; where a point lands is unseen); the
  Macro-in-Auto display fix (an unproven reading); the webcam feed with
  OBS (the feed ran for 22 s once, OBS is not installed).
- *Probably fine, but no log line proves it:* writes of shutter speed,
  drive, focus mode and white balance (writes are logged since session 8
  and only the four codes above appear), and starting a recording from
  the app (no `recording started` line in any log).
- *Decided, do not retry:* the ZV-E10 never offers a picture over Wi-Fi
  after a shot (the copy feature is hidden, default off); it has no card
  listing over PTP; no remote SteadyShot, Face/Eye AF or Remote Touch.
- *Never run on any hardware:* Windows, Bluetooth, SFTP from the camera,
  Access Authentication, several cameras at once.

**When Roy comes back,** he will most likely have tried the list under
"Next real-camera session". Start like this:

1. `git status`, `git log --oneline -5`, and
   `gh run list --repo Drake0306/Sony-Edge-Rimix --limit 3`.
2. Read the newest app log and `logs/camera-ZV-E10.json` in
   `~/Library/Application Support/Rimix/logs/` (the JSON is the camera's
   last state of the last session). The app writes `[password]` in place
   of the Wi-Fi password on every join line (checked on 2026-11-11: 20 of
   20); still never print a join line unfiltered, and never print
   `settings.json` whole.
3. Look for the log lines named under "Next real-camera session"; each
   open question there has the line that answers it.
4. Before using any property, control or operation code, look it up (see
   "What Sony's own reference says"). Roy, session 10: "Do not
   hallucinate. Check it properly, thoroughly, then implement it."

**How a round goes** (it worked ten times): Roy says what he saw; find the
cause in the log and the camera report before changing anything; write
the failing test against the simulator, then the fix; `cargo test
--workspace`, `pnpm -C apps/desktop test`, clippy, fmt, tsc; look at the
interface in `preview.html` with headless Chrome; release build into
`~/Downloads/Rimix-0.1.0/`, `check-mac-bundle.sh`, open it once and check
for Local Network blocks; branch, merge to `main`, push, wait for CI;
an HTML report with screenshots in the same folder; update this file and
the memory note; `cargo clean`.

**Ideas noted and not started:** the camera's HTTP live view (0xD278) as a
faster picture and 0xD26A as its quality; what event 0xC206 could give
for pictures after a shot; a picker for drive mode (36 values, still a
stepper); thumbnails in the PTP card browser (bodies with storage
commands only); a stable self-signed or Developer ID certificate so macOS
stops asking for webcam permission on each build; a real camera extension
for the webcam if Roy gets an Apple Developer account; version numbers
(everything is 0.1.0; the next release needs them raised in
`apps/desktop/src-tauri/tauri.conf.json`, `apps/desktop/package.json` and
every `Cargo.toml`).

## Rules that bind every session

- Never store, log, print, or paste the camera's Wi-Fi password, an SSH
  password, or a Bluetooth-read password. Roy once pasted his real QR text
  including the password; it must not be repeated anywhere. Test vectors use
  placeholders. The only secret allowed in `settings.json` is the SFTP
  receiver's own generated password, and that must not be printed either.
- Git: push to `origin` (`git@github-tb-vms:Drake0306/Sony-Edge-Rimix.git`,
  SSH alias `github-tb-vms`, author Drake0306). Plain commit messages under
  60 characters. No Claude credits, no Co-Authored-By lines.
- Disk is tight (about 18 GB free after `cargo clean`). Run `cargo clean`
  at the end of a build-heavy session. A release rebuild takes a few minutes.
- Roy prefers HTML reports over markdown for deliverables he reads; this
  file is an exception he asked for.
- Roy's standing instruction: do not wait for him, implement and test, then
  hand him a build in `~/Downloads/Rimix-0.1.0/` (`Rimix.app`,
  `Rimix_0.1.0_aarch64.dmg`, release CLI `rimix`).

## What the project is

Open-source Tauri + Rust desktop app that controls Sony Alpha cameras over
Wi-Fi the way Sony's phone app does. Spec:
`docs/superpowers/specs/2026-11-10-rimix-design.md`. Plans for Phases 2 to 4
are in `docs/superpowers/plans/`. Real-camera test guide:
`docs/camera-testing.md`.

Layout: `crates/sony-ptpip` (protocol: `ptpip/transport.rs`,
`sony/handshake.rs`, `sony/property.rs`, `sony/capture.rs`,
`sony/liveview.rs`, `camera.rs` with `camera/loops.rs` and
`camera/state.rs`, `discovery/`), `crates/sony-ptpip-sim` (simulator used
by all tests, knobs in `SimConfig`), `crates/rimix-wifi` (macOS
`networksetup` join, Windows `netsh`), `crates/rimix-sftp`,
`crates/rimix-ble`, `crates/rimix-cli`, `apps/desktop` (TypeScript UI in
`src/`, Tauri commands in `src-tauri/src/`).

Dev flags: `RIMIX_WIFI=fake`, `RIMIX_CAMERA_ADDR=127.0.0.1:15740`,
`RIMIX_WEBCAM=off`, `RIMIX_BLE=fake|off`, `RUST_LOG=debug`. App log:
`~/Library/Application Support/Rimix/logs/rimix.log.YYYY-MM-DD`.

Release build on this Mac:
`cargo build --release -p rimix-desktop --features tauri/custom-protocol`
then `CI=true pnpm -C apps/desktop exec tauri build`, plus
`cargo build --release -p rimix-cli`; copy into `~/Downloads/Rimix-0.1.0/`.
Then run `scripts/check-mac-bundle.sh ~/Downloads/Rimix-0.1.0/Rimix.app`:
it must print three `ok` lines. A bundle that fails it cannot reach any
camera (see "The 10:00 connect failure").

## State of the code (the history, session by session)

All four spec phases are built and merged. Everything was developed against
the simulator; only three real-camera sessions have happened, all on
2026-11-11. Each found a protocol difference, each was fixed and shipped:

1. 03:18Z: `expected CmdResponse, got StartData { tid: 3, total_len: 8 }`.
   The ZV-E10 sends an 8-byte data phase before each SDIO_Connect (0x9201)
   response. Fixed: 0x9201 read through the data-in path; the transport now
   tolerates an unexpected data phase (commit e47b1cb).
2. 03:26Z: `ExtDeviceInfo truncated`. 0x9202 arrays use u32 counts, not
   u16 (954bf66). Also the QR short name `YKE1` could not be joined; the
   real SSID is `DIRECT-<code>:<model>`, so `DIRECT-YKE1:ZV-E10` (321c8bb).
   QR grammar confirmed: `W01:S:<code>;P:<pw>;C:<model>;M:<mac>;`.
3. 03:47Z to 03:51Z: connected and showed live view, but the log shows
   `camera connection lost reason=operation timed out` every 6.3 s for the
   whole session (5 s command timeout poisons the link, supervisor
   reconnects in about 1.3 s). Roy saw lag, "disconnected" flashes, and
   ISO/shutter changes that stalled and came back unchanged. Diagnosis:
   Rimix sent two requests libgphoto2 never sends to Sony bodies: 0x9209
   GetAllExtDevicePropInfo WITH a parameter ([1] "changed only" every
   100 ms, [0] at connect) and 0x9251 single-property read of 0xD215 every
   250 ms. Fix merged as 599220b (commits 60fa8df, 7227e8c, 671314c,
   af99535): 0x9209 is sent with NO parameters (libgphoto2's
   `ptp_sony_getalldevicepropdesc`), about 4 times a second, diffed locally;
   0xD215 is taken from that table; 0x9251 is no longer sent in a session;
   every timeout now logs `operation timed out op=0x.... tid=N` and every
   transaction is logged at debug. The simulator defaults now refuse the
   old forms (`props_param_hangs`, `single_prop_read_hangs`), so the old
   code fails 49 tests with Timeout and the new code passes. Full suites
   green (413 Rust, 303 TS, clippy, fmt). Shipped to Downloads at 09:53
   local.

**Fix 3 is proven on the camera.** Session 4 (05:20:39Z to 05:25:55Z, the
signed build) ran over five minutes and 7,468 transactions on the
parameterless 0x9209 polling with no timeout and no reconnect; the single
`operation timed out op="0x9209"` at 05:25:55 is the session ending. Roy:
"worked perfectly, connected properly, clicked photos properly".

## The 10:00 connect failure (2026-11-11, 04:29Z): solved, was macOS

Symptom: with the 09:53 build Roy got "Joined DIRECT-YKE1:ZV-E10 but no
camera answered", having connected an hour earlier with the previous build.

**Cause.** macOS Local Network privacy was dropping every packet Rimix.app
sent to the local network (SSDP and TCP to port 15740 alike) with
`EHOSTUNREACH`, "No route to host". Nothing was wrong with the join, the
camera, DHCP, routing or the PTP code. The builds were only linker-signed:
code-signing identifier `rimix_desktop-c38e19902c842888`, Info.plist not
bound. macOS stores the Local Network permission for `com.rimix.desktop`;
a process whose signing identifier is something else is not reliably
matched to it once the app is rebuilt and replaced (most likely it only
matches through a per-binary UUID that goes stale; that part is inferred,
the experiment below is not). macOS then blocks the app while System
Settings (and `nehelper`) still say "allowed", and shows no prompt.

**Evidence** (`/usr/bin/log show`, around 09:59 to 10:00 local):
- kernel: `tcp drop outgoing [...:15740] ... process: rimix-desktop ...
  reason: NECP`, `error: 65`.
- UserEventAgent: `Got local network blocked notification: pid: 16101`,
  38 times in the session; nehelper each time: `Local network allowed by
  preference for Rimix (com.rimix.desktop)`.
- The block was there from the app's first second, while the Mac was
  still on the home network. It needs no camera to reproduce.
- Controlled test on the home network, same binary each time: linker
  signature = blocked 3 of 3 searches; throwaway identifier with a new
  UUID = blocked 3 of 3; signed as a bundle (identifier
  `com.rimix.desktop`) = allowed, also with a never-seen UUID. The CLI,
  run from a terminal, was never blocked (it runs under the terminal's
  permission).

**Why it worked at 03:47Z.** That build was blocked too for its first 26 s
(same kernel drops). It was let through from the Wi-Fi switch at 09:17:49
local, which coincided with `nesessionmanager` restarting (presumably
reinstalling the kernel's rules). At 09:59 no such restart happened.
Luck, not code.

**Things the earlier notes got wrong** (kept so nobody re-derives them):
- The join was real. The camera's DHCP gives this Mac `192.168.0.4`,
  netmask `255.255.0.0`, router and DHCP server `192.168.122.1`. The /16
  puts the camera on-link. The address appears about 40 ms after link-up
  because macOS reuses its remembered lease (INIT-REBOOT), so "any IPv4
  address means joined" was not the bug.
- The Mac was on the home network, not stuck on the camera's, when the
  app started. SSDP `No route to host` (65) there was the block.
  `Network is unreachable` (51) is the genuine "no address yet" error.

**Fix (branch `fix-local-network`, merged):**
- `tauri.conf.json`: `bundle.macOS.signingIdentity` is `-`, so the bundler
  signs the app ad hoc as a bundle (identifier `com.rimix.desktop`,
  hardened runtime, the entitlements in `Entitlements.plist`). In CI the
  `APPLE_SIGNING_IDENTITY` secret still overrides it.
- `Info.plist`: `NSLocalNetworkUsageDescription`.
- `scripts/check-mac-bundle.sh`: fails on a bundle signed the old way.
- The app now says so when it is blocked instead of blaming the camera:
  `sony_ptpip::discovery::discover_report` flags a search the system
  refused (SSDP send fails with 65 while a default route exists); the
  `discover` and `wait_for_camera` commands return
  `LOCAL_NETWORK_BLOCKED`; the connect screen's Nearby list and the
  connect error then point at System Settings, Privacy & Security, Local
  Network. The log line is `the system refused the SSDP search`.

**Verified on this Mac, 10:39 local:** the new build, copied over the old
one in `~/Downloads/Rimix-0.1.0/` (the exact step that failed before), was
not blocked on its first search: 0 block notifications, gateway SYN sent,
SSDP devices fetched. A deliberately mis-signed copy of it logged the new
warning. 419 Rust and 306 TS tests, clippy, fmt and tsc pass.

**Confirmed by Roy at 05:20Z:** the signed build connected first time. If
the app is ever blocked again, the manual cure is System Settings, Privacy &
Security, Local Network: turn Rimix off and on, then reopen it.

To check for this block in future, run while the app is open:

```
/usr/bin/log show --last 2m --info --debug --predicate 'eventMessage CONTAINS "local network blocked notification" OR (process == "kernel" AND eventMessage CONTAINS "rimix-desktop" AND eventMessage CONTAINS "reason: NECP")'
```

Note `log` alone is a zsh builtin; use `/usr/bin/log`.

Side effects of ad-hoc signing to know about: macOS asks for webcam
permission again on each new build (its record is tied to the exact
binary; this was already so). A stable self-signed or Developer ID
certificate would end that; not done, it needs Roy's keychain.

## Roy's feedback after the first good session (2026-11-11) and what was built

Branch `session-feedback`, merged. Everything below is tested against the
simulator and in the browser preview; nothing yet on the camera.

1. **Photos never reached the laptop.** The receive code was fine; the
   camera arrives with Still Img. Save Dest. = Camera Only (0xD222 = 0x10)
   and then never raises 0xD215. `capture::ensure_host_delivery` now runs
   in `connect_once` when `auto_receive` is on: it sets 0xD222 to the
   listed value with both bits (0x11, PC+Camera; the card copy is never
   given up) and 0xD268 (size sent to the computer) to 1, original. It logs
   `changed the camera's still image save destination ...` at info, or a
   warning if the camera refuses. `CameraState.shots_to_computer` drives a
   line in the Save to panel when the camera still keeps shots to itself.
   The simulator now models this (`still_destination`, `host_image_size`).
   Roy said "raw DNG"; Sony raw is ARW and whatever the camera shoots is
   what is copied.
2. **Videos.** `movie_loop` (camera/loops.rs): lists the card when a
   recording starts and again 1.5 s after it stops, and downloads the new
   `CardKind::Video` files through the card code, emitting the capture
   events so the filmstrip shows progress. UNKNOWN ON THE ZV-E10: whether
   it lets the card be read in a PC Remote session at all. If it refuses,
   `MovieTransferUnavailable` is emitted once and the UI says "Videos stay
   on the card". Two card walks per clip is slow on a full card; an
   ObjectAdded-event path would be better if the camera sends one.
3. **Transfer progress.** The filmstrip (right) has one summary bar above
   the per-file tiles.
4. **Mode "numbers".** `decode.rs` only knew eight modes; unknown values
   showed as `0x8000`. Tables now cover the Sony SDK / libgphoto2 lists for
   mode, white balance, drive (brackets decoded) and format; unknowns read
   `Unknown (0x....)`. Two changes are from references, not the camera:
   focus 0x8005 is now AF-A and 0x8006 DMF (they were swapped), and
   colour temperature is 0x8012 as well as the fixture's 0x8007.
5. **ISO.** A camera takes only its listed stops. `iso_step_target`: each
   press aims 50 away and lands on the nearest listed stop, always moving
   at least one (100, 160, 200, 250, 320 ...). The popover's Custom row
   shows min and max and sets the nearest stop to what is typed.
6. **Dock overlap.** The dock row was three flex items; the preset strip
   overflowed under Record. Now a grid `1fr auto 1fr`; only the preset in
   use shows its name.
7. **Webcam.** macOS needs a Developer ID camera extension to add a system
   camera, so Rimix cannot be one by itself (0 signing identities here).
   `src-tauri/src/webcam.rs` serves the live view as MJPEG on
   `127.0.0.1:8717` (`/`, `/stream.mjpg`, `/frame.jpg`), with Full, Medium
   and Low quality (scaled with the `image` crate). "Use as a webcam" in
   the Cameras panel swaps the live view for a panel with the address, the
   quality choice and the OBS steps. OBS is NOT installed on Roy's Mac;
   he has to install it, or get an Apple Developer account, in which case
   a real camera extension becomes possible and would reuse this mode.

Also new: on every connect the app writes `logs/camera-<model>.json`, the
whole property table with allowed values. READ IT FIRST next session: it
settles the mode values the ZV-E10 really uses, the focus and white
balance codes above, the ISO list, what 0xD222 offered, and whether there
is a live view quality property worth using for the webcam quality.

Dev tools added: `http://localhost:1420/preview.html` under `pnpm dev`
(the real UI with a pretend camera, `src/preview.ts`; screenshot it with
headless Chrome to check layout), and
`cargo run -p rimix-desktop --example webcam_feed`.

## Session 5 on the camera (06:18Z) and the real property table

Roy tested the feedback build. `docs/camera-reports/zv-e10-fw2.01.json` is
the report the app wrote at that connect: all 93 properties of the real
ZV-E10 (firmware 2.01) with their allowed values. It is the ground truth
for this body; check it before believing any table.

What it showed, and what was fixed on branch `real-values` (merged):

- **Mode and drive showed bare numbers except M.** The ZV-E10 sends
  0x500E and 0x5013 as U32: the classic 16-bit code in the lower half, a
  family number in the upper (P is `0x0001_0002`, Movie P `0x0007_8050`,
  Continuous Lo `0x0001_8012`). `label_for` took only u16, so every value
  but M (`0x0000_0001`) fell to the raw-number fallback. Now `low_word`
  names both forms. The values match libgphoto2's
  `sony_exposure_program_32_modes` and `sony3_capturemode`
  (camlibs/ptp2/config.c) exactly; the names follow Sony's Help Guide
  ("Program Auto", "Intelligent Auto", "Hand-held Twilight"). Tests list
  all 25 modes and 36 drive values the camera reported.
- **Confirmed from the report:** focus is 0x8005 AF-A, 0x8004 AF-C,
  0x8006 DMF (the camera lists them in menu order AF-S, AF-A, AF-C, DMF,
  MF); colour temperature is 0x8012 (0x8007 was only ever the fixture);
  ISO runs 100 to 32000; the save destination offers 1, 0x11, 0x10.
- **Photos still did not arrive.** The log says the write of 0xD222 was
  answered OK at connect, and the report taken straight after still says
  0x10 (card only). Either the camera applies it late or it ignores a
  write made that early (drive mode was also reported "off, 0" at that
  moment, so the body was not settled). `delivery_loop` now watches 0xD222
  for the whole session and asks again every 1.5 s, six times, then warns
  with the camera's own menu path. The capture and movie pipelines log at
  info now ("the camera offers a picture", "picture saved", "recording
  started", "movie saved"), so the next log shows how far a shot got.
  NOT YET KNOWN whether the retry is what the ZV-E10 needs.
- **Mode picker.** Roy asked for a list with an icon on the left and the
  mode's letter on the right instead of a stepper. `src/modes.ts` sorts
  the modes into Photo, Movie, S&Q and Scene; the popover's Mode row
  unfolds into tabs and a list (pictogram, Sony's name, dial cap). The
  pictograms are symbols `m-*` in `index.html`.
- Chips need short forms: `Choice.short` and `drive_short` / `wb_short`.

- **A switch for copying** (branch `save-switch`, merged; Roy: "there is
  no toggle to stop it from saving and turning it on saves new
  pictures"). Setting `saveToComputer` (default on), shown as "Copy to
  this computer" at the top of the Save to panel. `Camera::set_receiving`
  changes it while connected: the capture, movie and delivery loops always
  run and look at `Receive` (Leave for the CLI's `auto_receive: false`,
  Here, CardOnly). Off writes 0xD222 back to 0x10 so the camera does not
  hold shots for a computer that will not fetch; flipping the switch
  restarts the six tries of the delivery loop.

- **Session 6 (07:16Z) and branch `controls-and-capture` (merged).** The
  second report is `docs/camera-reports/zv-e10-fw2.01-session6.json`.
  - *Photos still not received, and now it is narrowed down.* The log
    shows 0xD222 = 0x11 at connect (`the camera is sending new pictures to
    this computer`), the switch flipping it both ways correctly, and NOT
    ONE `the camera offers a picture` line, while the card's remaining
    shots dropped by one. So the camera offered and Rimix failed before
    logging. Two faults found by reading the code against libgphoto2's
    `camera_sony_capture`: `parse_object_info` required the date strings
    after the file name, and `check_filename` rejected an empty name
    (libgphoto2 names such files itself); and every such failure was
    swallowed without a word. Now the dataset may end after the numbers,
    a nameless picture is saved as `Rimix-<date>.<EXT>`, 0xD215 changes
    and ObjectAdded (0xC201) events are logged at info, ObjectAdded on
    0xFFFFC001 also triggers a fetch, and every failed fetch warns with
    its error. WHICH of these was the cause on the ZV-E10 is not known:
    the next log will say (`a picture the camera offered could not be
    received` plus the error, or `picture saved`).
  - *Mode jumped to Macro in Intelligent Auto.* Reading of Roy's report:
    the camera puts the scene it recognises into the mode value, the Auto
    family number over the scene's code (`0x0004_8015`), and the lower
    half alone read as Macro. `Snapshot::mode_shown` now shows the listed
    mode of that family (last seen, else its automatic one) and logs each
    unlisted value once (`the camera reports a shooting mode it does not
    list`): that line confirms or refutes the reading.
  - *Format was blank:* the ZV-E10 has no 0x5004; file format is 0xD253
    (1 RAW, 2 RAW+JPEG, 3 JPEG), JPEG quality 0xD252, what is sent to the
    computer 0xD269 (it was 3, RAW only). `findProp(camera, PROP.format)`
    finds either.
  - *Shutter and aperture list no values* (form none): plus and minus did
    nothing. `Camera::step` now nudges with 0x9207 +1/-1 and `Camera::set`
    walks there (`control::nudge_to`), as libgphoto2's
    `_put_sony_value_u16`. Aperture was "display only" at both connects;
    such nudge-only settings are treated as usable unless "off". Whether
    the ZV-E10 obeys the nudge for aperture is untested.
  - *ISO* is back to one camera stop per press (Roy asked for steps of
    50, then of 10; neither exists on a camera, and the 50 rule skipped
    stops). *Colour temperature* has a kelvin scale; `PropertySummary`
    carries `range`. *Drive, Focus, White balance, Format* use the shared
    picker (`src/pickers.ts`); *Aperture* a grid of f-stops.

- **Session 7 (08:08Z) and branch `focus-and-connect` (merged).** Third
  report: `docs/camera-reports/zv-e10-fw2.01-session7.json`.
  - *Photos: the fault is before the pickup.* With the 13:21 build, which
    logs every change of 0xD215 and every ObjectAdded event, the session
    logged NEITHER, with 0xD222 = 0x11 confirmed at connect. So either no
    still was taken in that session (the report cannot say: it is written
    at connect; shots left went 863 to 861 over the session before), or
    the ZV-E10 over Wi-Fi does not raise 0xD215 for Rimix at all. The
    handshake and shutter sequence match libgphoto2's. The capture loop
    now watches shots left (0xD249): any picture taken logs `the camera
    took a picture`, and if nothing is offered within 6 s a warning gives
    0xD215, 0xD222, 0xD269 and 0xD253 as they stand. Every non-property
    PTP event is logged (`camera event code=...`). THE NEXT LOG DECIDES:
    took-a-picture followed by an offer means the earlier fixes work;
    followed by the warning means the camera never offers over Wi-Fi and
    the transfer has to be approached another way (the camera's HTTP
    side, 0xD278, is the lead).
  - *Aperture "still a toggle":* it was the new row, but with plus and
    minus beside it it read as the old control. The row is now a list to
    open like Drive; single steps are inside its panel.
  - *Tap to focus:* `Camera::focus_at` sets Flexible Spot: M (0xD22C =
    0x0102) when the area ignores a point, sends 0xD2DC (x << 16 | y on a
    640 by 480 grid, as libgphoto2's `_put_Sony_SpotFocusArea`) and half
    presses. Whether the ZV-E10 accepts 0xD2DC is untested; the report
    now lists the camera's control codes (`controls`), which says.
  - *Face and tracking frames:* live view metadata is read as up to three
    blocks (focus, face, tracking) of the same shape. Roy saw no frames
    at all on the camera, so the real layout differs from what Phase 1
    assumed; the log now carries `live view metadata sample` lines (hex,
    up to four a session) to decode it from. Rimix has no switch for the
    camera's Face/Eye AF or human/animal choice: no known property code.
  - *Two "save" settings:* "Receive from camera" (SFTP) is now "FTP
    transfer from the camera", with a line saying it is separate from
    "Copy to this computer".
  - *Connecting:* a cover over the connect screen with the step and a
    Cancel (`cancel_connect`; `AppState::cancellable` wraps join, find
    and connect).

- **Session 8 (08:48Z) and branch `frames-and-aperture` (merged).** Fourth
  report: `docs/camera-reports/zv-e10-fw2.01-session8.json`. Roy: tap to
  focus works but lands somewhere else; Auto modes (photo and movie) still
  show Macro; the Aperture list is not there, only the old stepper.
  - *THE TABLE AT CONNECT IS NOT SETTLED.* The report is written at
    connect, and there the ZV-E10 says drive off, focus Manual, focus area
    Wide, aperture "display only" with no values. Roy then autofocused and
    saw a stepper for aperture, which the popover only draws when the
    camera LISTS f-stops. So do not reason about a session from the
    connect-time report. `ReportKeeper` (state.rs) now rewrites
    `logs/camera-<model>.json` whenever the settings change, so the file
    is the camera's last state of the session.
  - *Aperture.* Cause (inferred from the above, reproduced in a test): with
    listed f-stops the row was a plain stepper and the chip greyed out
    ("display only"). The row is now always the f-stop list, with the
    camera's own stops when it lists them, and aperture and shutter speed
    count as usable unless "off", in the interface and in `Camera::set`.
  - *Live view metadata, decoded from the log's samples* (tests in
    `sony/liveview.rs` hold the real bytes): version 0x65; level at 0x10
    (state 2 = valid, roll at 0x18 in WHOLE DEGREES wrapping at 360, so
    359 is one degree left; it was read as thousandths); frame blocks
    start at 0x38, not 0x28, which is why no frame and no level ever
    showed. Each block: grid 640x1024 by 480x1024, count, 24 bytes per
    frame (kind, state, centre x and y, height, width). The camera sends
    FOUR blocks; the first is focus frames (seen: kind 6, state 1). Which
    of the others is faces and which tracking is still from Sony's SDK,
    not seen filled. The loop now logs a sample for each new shape of the
    data (up to 16 a session), so a face in front of the camera gives the
    bytes to check.
  - *Tap to focus.* The camera took every point (0xD2DC answered OK, a
    0xC20C event after each) and nothing asked for a change of focus area,
    so the area was a spot already or was fixed by the mode: unknown
    which. `Camera::focus_at` now goes by the area: a spot in use is moved
    (0xD2DC); wide, zone and centre are made Flexible Spot: M and the
    point waits until the camera reports the change; an area the camera
    fixes (automatic modes) gets a remote touch (0xD2E4, which the ZV-E10
    lists; value assumed to be the same x << 16 | y, UNTESTED) or, with no
    such control, an error saying the camera chooses. Each tap logs
    `focus point ... focus_area=... focus_mode=... how=...` and then twice
    `the camera's frames after a focus point` with where the camera's own
    frames are. THAT PAIR OF LINES SAYS whether a point lands where it was
    asked (compare asked_x/asked_y with the frame's centre). If they
    differ by a fixed scale or offset, the grid is not 640 by 480 over the
    whole picture (3:2 and 16:9 letterboxing is the suspect).
  - *Macro in Auto modes.* The log has no `does not list` line in any
    session, so the earlier reading (Auto's family over the scene's code)
    never happened. New reading: in an automatic mode the camera reports
    the scene it recognises as that scene's own listed mode
    (`0x0005_8015`). `Snapshot` now remembers the automatic mode
    (`auto_mode`) and shows it while scene modes follow it, unless the
    scene was asked for through the app (`mode_chosen`). Every change of
    the reported mode is logged (`the camera's shooting mode reported=...
    shown_as=... was=...`), and every write (`setting written to the
    camera`, and `the camera took a setting and reports another value
    straight after`). If the reading is wrong the log shows what the
    camera does instead: a write of Movie P that the camera ignores (it
    may need its own Still/Movie button) is the other suspect.
  - *Photos:* no still was taken in session 8 (no `the camera took a
    picture` line), so the question from session 7 is still open.
  - Dev tool: `preview.html?report=<file of docs/camera-reports>` puts a
    real report behind the interface.

- **Session 9 (10:02Z) and branch `purpose-and-auto` (merged).** Fifth
  report, and the first SETTLED one (the file is now rewritten through the
  session): `docs/camera-reports/zv-e10-fw2.01-session9.json`. Prefer it
  to the four before it.
  - *PHOTOS: DECIDED. The ZV-E10 does not offer its pictures over Wi-Fi.*
    At 10:05:04 the log has `the camera took a picture shots_left=858`,
    events 0xC20C and 0xC206 at that moment, and six seconds later `has
    not offered it`, with 0xD215 = 0 and 0xD222 = 0x11 (card and
    computer). No ObjectAdded, no 0xD215. So the libgphoto2 route does
    not work on this body over Wi-Fi; more retries are pointless. Leads
    not followed: what 0xC206 means (it came with the shot), the camera's
    HTTP side (0xD278). Roy asked for the feature to be put away:
    setting `copyFeature` (default OFF; "Show copying to this computer"
    in App settings) hides the Save to panel and Last captures, and
    `AppSettings::copies()` (feature AND its switch) is what connect and
    `save_settings` go by, so off also puts the camera on card only.
  - *Aperture: confirmed.* Settled, 0x5007 is enabled with 17 listed
    f-stops (f/3.5 to f/22); direct writes are taken. Shutter speed is a
    list of 53 too, ISO of 32 with Auto (0xFFFFFF) first.
  - *Mode writes work, three seconds late.* 0x500E written at 10:04:08.5,
    reported at 10:04:11.3, photo to movie included. No Macro was seen
    this session, so the recognised-scene reading is still unproven.
  - *Tap to focus failed: "The camera did not take the focus point".*
    Settled, 0xD22C lists only [1 Wide, 2 Zone, 0x103 Flexible Spot: L]
    (it listed fourteen at connect). Rimix asked for 0x0102, not listed,
    so it fell to the remote touch (0xD2E4 with x << 16 | y), which the
    camera refused 19 times of 19. That also explains session 8: the area
    stayed Wide, where 0xD2DC is accepted and ignored. Now `focus_at`
    takes the first spot the camera lists (M, S, L, expanded, then the
    tracking forms), waits up to 3.2 s for the change, and sends no
    touch. STILL NOT KNOWN: whether the point lands where asked; the
    `the camera's frames after a focus point` lines will say (none were
    written this session, every tap failed first).
  - *Frames, from this session's samples* (decoded by hand from the log):
    at the shot, ten focus frames of kind 2, state 2 (the small focused
    points of Wide), so the first block is read right on real data; at
    10:03:05 ONE frame in the SECOND block (kind 2, state 1, the field
    after them 0x800), centre 0.50, 0.58, 0.528 wide and 0.79 high. On a
    3:2 picture that is a square (0.528 x 3 = 0.79 x 2), as a face frame
    is, and the focus points are squares the same way. So the second
    block is faces, and the 640 by 480 grid is STRETCHED over the whole
    picture, not letterboxed: fractions of the grid are fractions of the
    picture, which is how the parser and `focus_at` already count. The
    letterbox suspicion noted under session 8 is very likely wrong.
  - The focus area has names (`decode::focus_area_names`) and a row in
    the settings popover, because a tap leaves the camera on a spot.
  - *Auto for shutter speed, aperture, ISO* (Roy: "add an auto mode ...
    even if you are in manual mode"). A camera has no auto value for
    shutter or aperture: it is the mode (A chooses the shutter, S the
    aperture, P both). `src/auto.ts`: the Auto button on those rows
    writes the mode that makes it so, within photo, movie or S&Q; ISO's
    writes 0xFFFFFF and goes back to the value set by hand.
  - *What the camera is for, chosen before connecting* (`openAs`: Remote,
    Webcam, Files; three cards on the connect screen). After a connect,
    `openAsChosen` turns the webcam feed on, or opens the card browser.
    The card browser says "Import" and has its own destination line.
    Card reading in a PC Remote session is STILL UNTESTED on the ZV-E10;
    "Browse files" is the first thing that will try it for Roy. No
    thumbnails yet (no GetThumb in the protocol crate).
  - Fixed on the way: a change to a movie mode read as "took a picture"
    (shots left falls to 0); the check now takes drops of 1 to 12 only.

- **Session 10 (11:09Z) and branch `files-and-zoom` (merged).** Report:
  `docs/camera-reports/zv-e10-fw2.01-session10.json`. Roy: the three
  purpose buttons look wrong on hover, the settings on the right cannot be
  scrolled, "Browse files" did nothing, and he wants stabilisation and zoom.
  Working on the camera in that session (from the log and report): the
  Auto buttons (ISO Auto 0xFFFFFF and mode M to A were written and taken),
  the Focus area row (Wide to Zone and back). Tap to focus was not tried.
  - *Browse files did nothing* because it opened the PTP card browser,
    and this body has no storage commands at all (see "What Sony's own
    reference says"). The log has not one card line. REBUILT on Send to
    Smartphone: `crates/sony-ptpip/src/sendto.rs` (`SendTo::open`, `list`,
    `picture`, `download`, `close`), a pretend camera in
    `sony-ptpip-sim/src/sendto.rs`, commands `files_open`, `files_list`,
    `files_picture`, `files_import`, `files_close` (event
    `files-progress`), dev override `RIMIX_FILES_ADDR`. With "Browse files"
    chosen, a connect joins the Wi-Fi and calls `files_open` (the router's
    address, port 64321): NO PTP session, no live view; the store's screen
    is `files` and `components/filesScreen.ts` shows folders, thumbnails,
    a large preview and Import. "Done" sends X_TransferEnd (the camera
    leaves the mode and its Wi-Fi). NOTHING OF THIS HAS MET A REAL CAMERA.
    What to look for in the log: `Send to Smartphone opened ... root=`,
    `listed a folder of the camera`, `imported a file from the camera`,
    or `no camera in Send to Smartphone answered` with the error. Unknown
    on the ZV-E10: the port and description path (64321, DmsDescPush.xml
    are what other bodies use), whether RAW and movies are offered, and
    whether its network name and password are the same in this mode.
  - "Browse card" in the Cameras panel is hidden on generation 3 bodies.
    The PTP card code and the movie copy stay for bodies that have the
    storage commands; none has been tried.
  - *Zoom* (Lens section of the settings popover, only when the camera
    reports 0xD25B): W and T buttons that zoom while held
    (`Camera::zoom`, 0xD2DD with 1, -1, then 0 on release; refused unless
    0xD25B is 1), the zoom scale beside them, and a Zoom setting list
    (0xD25F). In RAW the camera marks that list display only; the panel
    says why (Sony's Help Guide: no zoom but the lens's own in RAW or
    RAW & JPEG) and offers "Switch to JPEG". The session-10 report shows
    0xD25B = 1 with Roy's power zoom lens, scale 1000, setting 1 of
    [1, 3, 4]. Untested on the camera.
  - *Stabilisation:* not possible on this body, per Sony's table. Rimix
    shows a SteadyShot row on bodies that report 0xD0D9 / 0xD0DA (1 off,
    2 on; movie: 1 off, 2 standard, 3 active, 4 hybrid).
  - Fixed: the hover tint of the purpose buttons (a tint over the glass,
    not instead of it), and the App settings popover now scrolls (it had
    no height limit and had outgrown a laptop screen).

## What Sony's own reference says (found 2026-11-11, session 10)

READ THIS BEFORE GUESSING AT A CODE. Sony's "Camera Control PTP 3
Reference" (and the PTP 2 one) is transcribed page by page in
`github.com/olkham/pysonycam` under `docs/sdk/` (750 pages; it is Sony's
copyright, so it is not copied here: download the repo tarball and grep
it). Every operation, property, control and event has a compatibility
table with a ZV-E10 column. For the ZV-E10 (which does both PTP 2 and 3;
Rimix sends InitiatorVersion 0x012C, PTP 3):

- Operations supported: 0x1001 to 0x1003, GetObjectInfo 0x1008, GetObject
  0x1009, GetPartialObject 0x101B, 0x9201, 0x9202, 0x9205, 0x9207, 0x9209.
  NOT supported: GetStorageIDs, GetObjectHandles, GetThumb,
  SetContentsTransferMode 0x9212, GetContentInfoList. So THE CARD CANNOT
  BE LISTED OVER PTP ON THIS BODY, in any mode (libgphoto2 agrees: its
  "ZV-E10 (Control)" has no file listing, files are "ZV-E10 (MTP mode)"
  over USB). The card browser and the movie copy built on those
  operations can never have worked on it.
- NOT supported: SteadyShot (0xD0D9, movie 0xD0DA), Face/Eye and subject
  recognition (0xD060, 0xD157, 0xD158, 0xD1B8), Remote Touch Operation
  0xD2E4 (the body lists the control and refuses it; its enable status
  0xD284 read 0), Zoom Type Status, Zoom Speed Range.
- Supported zoom: Zoom Operation Enable Status 0xD25B (1 = enabled), Zoom
  Scale 0xD25C (x 0.001; min, max and step vary), Zoom Bar Information
  0xD25D (bits 31-24 boxes in all, 23-16 the box in use, 15-0 the place
  in it), Zoom Setting 0xD25F (1 optical only, 2 smart, 3 Clear Image
  Zoom, 4 digital zoom), and the control Zoom Operation 0xD2DD (INT8: 1
  tele, -1 wide, 0 stop; only while 0xD25B is 1). The ZVZoom project
  confirms 0xD2DD on a ZV-1.
- AF Area Position 0xD2DC: "x in the upper two bytes, y in the lower;
  X 0 to 639, Y 0 to 479". As Rimix sends it.
- Names for codes the notes above call by libgphoto2's: 0xD215 is
  "Shooting File Info" (over 0x8000: fetch 0xFFFFC001 until it is 0),
  0xD214 "Predicted Maximum File Size", 0xD283 "Function of Touch
  Operation", 0xD267 "Live View Area (x, y)". Event 0xC206 is
  "Captured" (libgphoto2), 0xC20C "FTP display name list changed".

**Files over Wi-Fi are a different camera mode and a different protocol:
"Send to Smartphone"** (Menu, Network, Send to Smartphone Func., Send to
Smartphone; on the ZV-E10 the pictures are chosen on the camera: This
Image, All with this date, Multiple Images). The camera then runs a UPnP
media server at `http://192.168.122.1:64321` on its own Wi-Fi (same
network name and password). Reference: `schorschii/ImagingEdge4Linux`
(`imaging-edge.py`, read in full), agreeing with `WWNNL/SonyCameraTransfer`
and `okru1/sony-photo-transfer`:
`GET /DmsDescPush.xml`; SOAP to `/upnp/control/XPushList`
(`urn:schemas-sony-com:service:XPushList:1`) `X_TransferStart`, then
`/upnp/control/ContentDirectory` `Browse` of `PushRoot` (what was chosen
on the camera) or, failing that, `PhotoRoot` (everything, on bodies that
have "Select on Smartphone"), `BrowseDirectChildren`, paged; the Result is
DIDL-Lite XML inside the XML; each item has several `res` links, told
apart by `protocolInfo` (`_TN` thumbnail, `_SM`, `_LRG` previews) and the
original with `size` and `resolution`; then `X_TransferEnd` with
`<ErrCode>0</ErrCode>`, after which the camera leaves the mode and closes
its Wi-Fi. On older bodies a RAW is offered only as JPEG previews (HTTP
406 for the original); the ZV-E10 has a "RAW+J Trans. Trgt." menu, so it
may send RAW: not yet seen. NOT TESTED ON THE ZV-E10 by anyone listed.

Seen in the report and not used yet: 0xD278 is an HTTP live view stream
URL on the camera (`http://192.168.122.1:60152/liveviewstream?...`),
possibly a faster live view than the PTP frames; 0xD26A (1 or 2,
writable) may be the live view quality; drive mode has 36 values and is
still a stepper.

## CI and releasing (2026-11-11)

**CI had been red for a day and nobody looked.** `.github/workflows/ci.yml`
installs the newest stable Rust (1.99.0 on that day); this Mac has 1.93.1.
Newer clippy rejects code the local one accepts, the first failing lint
stops the job, and the tests after it never run. Every push from
2026-11-10 22:10Z to the release failed that way (one `sort_by` in
`rimix-cli`), through all ten camera rounds, so nothing built that day had
been tested on Windows or by CI at all until the release was prepared.
Fixed then: `sort_by_key` with `Reverse` in `crates/rimix-cli/src/format.rs`,
and a compare-exchange loop in `webcam.rs` in place of
`AtomicUsize::fetch_update` (deprecated in 1.99; its replacement
`try_update` does not exist in 1.93, so neither is used).

- RULE: after every push, look at
  `gh run list --repo Drake0306/Sony-Edge-Rimix --limit 3` and wait for
  green; a red run is read with
  `gh api repos/Drake0306/Sony-Edge-Rimix/actions/jobs/<job id>/logs`.
- To hear what CI's Rust says without a round trip:
  `rustup toolchain install 1.99.0 --profile minimal -c clippy --no-self-update`,
  `cargo +1.99.0 clippy --workspace --all-targets -- -D warnings`, then
  `rustup toolchain uninstall 1.99.0` (it is about 1 GB). Do NOT
  `rustup update` the default toolchain: the release recipe above is
  known to work on 1.93.1 with macOS 27.
- Windows-only code (`crates/rimix-wifi` netsh, `discovery/probe.rs`)
  cannot be linted or tested on this Mac; only CI's `windows` job does.

**Releasing** is in the README under "Release a version": pushing a tag
`vX.Y.Z` starts `.github/workflows/release.yml`, which builds a universal
Mac DMG, then the Windows NSIS installer, into ONE DRAFT release on
GitHub; the draft is reviewed and then published by hand. Without Apple
signing secrets (none are set) the Mac app is signed ad hoc as a bundle,
which is what `check-mac-bundle.sh` needs. The repository is public.

**Release 0.1.0, what was done and checked (2026-11-11, 15:52Z to 16:15Z).**
CI was made green on branch `release-0.1.0` (Mac and Windows; the first
time that day's code ran its tests on Windows), merged to `main`, and
`v0.1.0` pushed. The Release run (43052768322) passed both jobs. The DMG
was downloaded from the draft and checked on this Mac: `x86_64 arm64`,
identifier `com.rimix.desktop`, ad hoc with hardened runtime,
`check-mac-bundle.sh` three `ok`, minimum macOS 14.0; started from a
scratch copy it logged `Rimix started version="0.1.0"` and drew no Local
Network block in 8 s. The Windows installer was built and NOT run by
anyone. The draft also carries `Rimix_universal.app.tar.gz`, which the
build action adds by itself.

Things the permission system refused in that session, so do not be
surprised by them: a throwaway `v0.0.0-test` tag (the README suggests one
for a first release; the real tag was pushed instead and made only a
draft), deleting the merged branch `release-0.1.0` on origin (it is still
there and can go), and publishing the draft.

The README was brought up to date for the release: its Status section is
now a table of what was tried on the ZV-E10, and Features describes the
purpose choice, Browse files, zoom, the Auto buttons and the hidden copy
feature. Keep it true when a camera session settles something.

## Next real-camera session

Roy tries, most important first: (1) Browse files: on the camera, play
back a picture, Menu, Network, Send to Smartphone Func., Send to
Smartphone, choose pictures; in Rimix choose Browse files and connect
with the name and password the camera shows. Read the log for the
`Send to Smartphone` lines above; if nothing answered, find out from the
camera's own screen and `nc -z 192.168.122.1 64321` whether the port is
right. (2) Zoom: hold W and T in Settings, Lens. (3) A tap on the live
view in P or M (`focus point ... how=`, `the camera's frames after a
focus point`). (4) Intelligent Auto on a close subject (`the camera's
shooting mode` lines).

If the connect screen says macOS is keeping Rimix off the local network,
do the Local Network toggle above: it would mean the signing fix is not
sufficient on its own.

Diagnostic commands if the connection itself fails (Roy types the password
himself; never paste it into the chat):

```
networksetup -setairportnetwork en0 "DIRECT-YKE1:ZV-E10" "<password>"
sleep 8
ifconfig en0 | grep "inet "
route -n get default
nc -z -w 3 192.168.122.1 15740 && echo PTP_PORT_OPEN
~/Downloads/Rimix-0.1.0/rimix discover
RUST_LOG=debug ~/Downloads/Rimix-0.1.0/rimix watch 192.168.122.1 --exit-after 20 2>&1 | tail -80
tail -60 ~/Library/Application\ Support/Rimix/logs/rimix.log.$(date +%F)
```

The CLI is not subject to the app's Local Network block, so if the CLI
connects and the app does not, suspect the block first.

### Other open items

- A movie clip that ends before the card has been listed (the listing
  starts when the recording does) cannot be told from older clips and is
  not copied; `movie_loop` now says so in the log (`the recording ended
  before the card had been listed`). On a full card that listing takes
  many seconds, so short clips will hit this. A listing kept from connect
  would fix it at the price of a card walk on every connect; not done,
  and whether the ZV-E10 lets the card be read at all is still unknown.

- The join code still treats "any IPv4 address" as joined. It was not the
  cause here and the camera's lease does arrive that fast, but a join to
  a network whose DHCP is slow would report success early. Low priority.
- Other connect failures still do not say which step failed (join vs find
  vs handshake) with the underlying detail; only the Local Network case
  is now named.
- Never verified on hardware: card browsing in PC Remote mode, BLE
  handoff, SFTP push from the camera, SSH Access Authentication, Windows
  join. `docs/camera-testing.md` steps 9 to 11 collect these.
- `project-sony-edge-rimix-goal.md` in Claude's memory directory holds the
  same history in short form.
