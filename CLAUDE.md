# Rimix: hand-off notes for the next session

Written 2026-11-11 at Roy's request after the second real-camera test went
badly; updated the same day once the "did not even connect" regression was
understood and fixed (see "The 10:00 connect failure" below). Read this
before touching anything. The user is Roy; the camera is a Sony ZV-E10
(firmware 2.00) on a Mac running macOS 27. Roy retested at 05:20Z: it
connected, stayed connected and shot. He then asked for a list of changes,
all built the same day (see "Roy's feedback after the first good
session"); none of those is yet tested on the camera.

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

## State of the code (main = merge of real-values)

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

Seen in the report and not used yet: 0xD278 is an HTTP live view stream
URL on the camera (`http://192.168.122.1:60152/liveviewstream?...`),
possibly a faster live view than the PTP frames; 0xD26A (1 or 2,
writable) may be the live view quality; drive mode has 36 values and is
still a stepper.

## Next real-camera session

Roy opens the new build and tries: the Mode chip (picker, names), Drive
(names), then shoot. The question that matters most is still whether a
picture reaches the Save to folder. Read the log for `the camera is
sending new pictures to this computer` (the save destination took),
`the camera offers a picture` and `picture saved`; or the warning that the
camera keeps saving to its card only, in which case Roy sets Still Img.
Save Dest. to PC+Camera on the camera once and tries again. Then record
and stop (`recording started` / `movie saved` / the card refused), and
"Use as a webcam". Likely follow-ups: mode values missing from the table; the card
refused in PC Remote (then say so in the README and drop the two walks);
the save destination write refused (the log has a warning naming it).

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
