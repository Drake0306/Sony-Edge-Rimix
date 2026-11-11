# Rimix: hand-off notes for the next session

Written 2026-11-11 at Roy's request after the second real-camera test went
badly. Read this before touching anything. The user is Roy; the camera is a
Sony ZV-E10 (firmware 2.00) on a Mac running macOS 27. Roy's words at the
end of the last session: "it connected before, now it did not even connect
properly". That regression is open and is the first thing to fix.

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

## State of the code (main = 599220b, pushed)

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

**Caveat on fix 3, not yet proven on the camera:** the ZV-E10 DID answer
0x9209 with parameter [0] at connect (sessions came up). Whether it answers
the parameterless form is taken from libgphoto2. If it does not, connect
would now time out after 5 s and the log would show
`WARN ... operation timed out op="0x9209"`. The 10:00 local log has no such
line, so the regression below is not that, but keep it in mind.

## THE OPEN BUG (happening now, 2026-11-11 10:00 local, 04:29Z to 04:30Z)

With the 09:53 build, Roy pressed connect and got:

> Joined DIRECT-YKE1:ZV-E10 but no camera answered. On the camera, open
> Menu, Network, PC Remote Function, and stay on that screen.

Roy says this connection worked an hour earlier with the previous build
(session 3 above), camera 1 cm away.

### What the log proves (`rimix.log.2026-11-11`, 04:29:22Z to 04:30:41Z)

- The app started at 04:29:22Z. From the first second, SSDP discovery
  failed every round with `No route to host (os error 65)`, before any
  join was attempted. That error for a multicast send means the Mac was
  on an interface with no usable route: most likely still on the camera's
  Wi-Fi Direct network from the previous session, or on no network.
- Join at 04:29:52.084Z: `networksetup -setairportnetwork en0
  DIRECT-YKE1:ZV-E10 [password]` printed nothing (that is success for
  networksetup). 50 ms later `getairportnetwork` said "not associated"
  (macOS 27 always says that, the SSID is hidden) and `ipconfig getifaddr
  en0` printed `192.168.0.4`. The join code treats "any IPv4 address" as
  joined, so it returned success 60 ms after the join command. A DHCP
  lease from the camera cannot arrive in 60 ms, so 192.168.0.4 was already
  on en0 before the join.
- `192.168.0.4` is the address en0 had after EVERY join today (03:18,
  03:26, 03:47, 04:29, 04:30), including the one that worked. The camera is
  at 192.168.122.1. Those are different /24s. On this Mac's home network
  en0 is 192.168.31.179, so 192.168.0.4 is not the home address either.
  Unknown: whether the ZV-E10 hands out 192.168.0.x leases with a default
  route via 192.168.122.1, or whether 192.168.0.4 is a stale/self-assigned
  address.
- After the join the app does NOT connect to the stored address. It calls
  `waitForCamera(FIND_MS = 30 s)` (`apps/desktop/src/actions.ts`
  `connectWithWifi`), which runs SSDP and a probe of the default gateway
  (`crates/sony-ptpip/src/discovery/probe.rs`: `route -n get default`,
  then TCP to that gateway on 15740) in 1.5 s rounds. SSDP kept failing
  with `No route to host`. The gateway probe logged nothing, which means
  `route -n get default` returned no `gateway:` line or the TCP connect
  failed quietly: there was no default route through the camera. After
  30 s the UI showed "no camera answered". Roy retried twice (04:30:17
  and 04:30:31), same result. At 04:30:41 SSDP changed to `Network is
  unreachable (os error 51)`: en0 had lost its address entirely. The log
  ends there.
- No PTP/IP traffic was attempted at all in this session. The new polling
  code never ran. The failure is entirely in Wi-Fi join / routing /
  discovery, before the protocol layer.
- In session 3 (03:47Z) the same sequence worked: SSDP also failed with
  `No route to host` at 03:47:43 and 03:47:48, then the join at 03:47:50
  reported 192.168.0.4, and by 03:47:58 the camera was connected (first
  timeout line). So that time the gateway probe or SSDP succeeded within
  8 s, meaning the Mac did get a route to 192.168.122.1 shortly after the
  join. This time it never did in 50 s.

### Ranked hypotheses for the next session

1. **macOS never actually associated with the camera this time.** The
   camera may not have been on the PC Remote / Wi-Fi Direct screen, or it
   still held the previous session's link (the earlier app instance was
   killed without a PTP CloseSession; Sony bodies keep one Wi-Fi Direct
   client). `networksetup` prints nothing on failure to associate in some
   macOS versions, and our success rule (any IPv4 on en0) is satisfied by
   a stale address, so the app said "Joined" without evidence. Fix
   direction: treat a join as successful only when the address CHANGES or
   a default gateway appears, with a bound (say 20 s), and otherwise say
   "could not join" rather than "joined but no camera".
2. **The address rule is wrong even when association succeeds.** Waiting
   for "an IPv4 address" returns before DHCP finishes. Then `waitForCamera`
   has 30 s, which was enough at 03:47 and not at 04:29. Fix direction:
   after the join, wait for `route -n get default` to show a gateway on
   en0 (the camera is the gateway on Wi-Fi Direct) before discovery; and
   fall back to a direct TCP probe of the remembered address
   (192.168.122.1:15740 in `recentCameras`) during the find step, not only
   the gateway.
3. **Camera-side state.** After the 03:47 session's 30 forced reconnects,
   the ZV-E10 may have been left in a bad state until power-cycled. Roy
   should power the camera off and on before the next test. Not a code
   fix, but rules out noise.
4. Not the cause: the 0x9209 change (no PTP traffic happened), the
   handshake fixes, the QR/SSID composition (the SSID is right).

### Diagnostic recipe for the next session

Ask Roy to put the camera on Menu > Network > PC Remote Function (Wi-Fi
Direct Info screen), power-cycled, then run, in order, and send the output:

```
networksetup -setairportnetwork en0 "DIRECT-YKE1:ZV-E10" "<password>"   # Roy types the password himself; never paste it into the chat
sleep 8
ifconfig en0 | grep "inet "
route -n get default
ipconfig getsummary en0 | grep -iE "yiaddr|router|lease|SSID"
nc -z -w 3 192.168.122.1 15740 && echo PTP_PORT_OPEN
~/Downloads/Rimix-0.1.0/rimix discover
RUST_LOG=debug ~/Downloads/Rimix-0.1.0/rimix watch 192.168.122.1 --exit-after 20 2>&1 | tail -80
tail -60 ~/Library/Application\ Support/Rimix/logs/rimix.log.$(date +%F)
```

Those answer, in order: did association happen, what address and gateway
the camera gives, whether the PTP port is reachable, whether discovery
finds it, and whether the new parameterless 0x9209 polling works (look for
`operation timed out op=` lines). The `watch` output also settles the
caveat on fix 3.

If the camera is reachable from this Mac while a session is running, a
`tcpdump -i en0 -w` capture of port 15740 is the fastest way to compare
against libgphoto2's exchange.

### Other open items

- The desktop app only shows the camera's response on the connect screen;
  it does not yet surface "which step failed" (join vs find vs handshake)
  with the underlying detail. Adding the detail to the error text or a
  "show log" button would have saved an hour today.
- Never verified on hardware: card browsing in PC Remote mode, BLE
  handoff, SFTP push from the camera, SSH Access Authentication, Windows
  join. `docs/camera-testing.md` steps 9 to 11 collect these.
- `project-sony-edge-rimix-goal.md` in Claude's memory directory holds the
  same history in short form; update it when the bug is understood.
