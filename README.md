# Sony Edge Rimix

A desktop companion for Sony Alpha cameras that connects over Wi-Fi, the way Sony's phone app does, instead of needing a USB cable. Mac first, Windows next.

## Status

Phase 1 works against a simulated camera: the protocol library, a command line tool and a desktop app all run and pass their tests there. None of it has been tried on a real camera yet. To change that, follow [docs/camera-testing.md](docs/camera-testing.md).

## What's inside

- `crates/sony-ptpip` is the protocol library that talks to the camera over Wi-Fi (PTP/IP).
- `crates/sony-ptpip-sim` is a simulated camera (`sony-ptpip-sim`) for trying things without hardware.
- `crates/rimix-cli` is the command line tool (`rimix`): discover, connect, props, set, step, shoot, record, liveview, watch, wifi.
- `apps/desktop` is the desktop app, built with Tauri.
- `docs/` holds the design and the real-camera test guide.

## Build and run

You need:

- Rust 1.93 or newer, installed with [rustup](https://rustup.rs)
- Node 20 or newer, and pnpm
- Xcode command line tools (`xcode-select --install`) on a Mac, or the Visual Studio C++ build tools on Windows
- The Tauri command line tool: `cargo install tauri-cli --version '^2'`

Build and test everything:

```
cargo build --release
cargo test --workspace
```

The CI workflow runs these checks on every push: formatting, Clippy, the Rust tests and the front-end tests on macOS, and Clippy plus the Rust tests on Windows. It runs on two runners, `macos-latest` and `windows-latest`.

Run the simulated camera, then connect to it from the command line tool (in a second terminal):

```
cargo run -p sony-ptpip-sim -- --port 15740
cargo run -p rimix-cli -- connect 127.0.0.1
```

Run the desktop app, or build it:

```
cd apps/desktop
pnpm install
pnpm tauri dev
pnpm tauri build
```

## Install on a Mac

Download the latest `Rimix_..._universal.dmg` from the [Releases](../../releases) page, open it and drag Rimix into Applications. Rimix needs macOS 14 or newer.

The download is unsigned until the project owner adds Apple signing secrets (see "Release a version"), so macOS will warn about it the first time. Either right-click Rimix in Applications and choose Open, or run this once in Terminal:

```
xattr -d com.apple.quarantine /Applications/Rimix.app
```

Rimix asks for camera permission on first launch. It uses the webcam only to scan the QR code on your camera's screen. To join the camera's Wi-Fi network, Rimix uses macOS's own `networksetup` command, which needs no admin rights.

## Install on Windows

Download the latest `Rimix_<version>_x64-setup.exe` from the [Releases](../../releases) page and run it. It installs Rimix for your user account only, so it needs no admin rights.

The installer is unsigned, so Windows SmartScreen warns about it the first time. Choose "More info", then "Run anyway".

To join the camera's Wi-Fi network, Rimix uses Windows's own `netsh wlan` command, which needs no admin rights. Rimix adds a network profile for the camera the first time; the profile stays in Windows, so later reconnects need no password. You can remove it in Settings, Network and internet, Wi-Fi, Manage known networks.

## Connect your camera

The connect screen has three ways in:

1. **Scan the QR code.** On the camera choose Menu, Network, Send to Smartphone Func., Send to Smartphone, then hold the screen up to the Mac's webcam. If the webcam can't read it, scan the code with your phone and use "Paste the code's text" under the QR card.
2. **Nearby.** If the camera and the Mac are on the same router, the camera shows up in the list. Pick it.
3. **Type the Wi-Fi name and password.** On the camera choose Menu, Network, PC Remote Function, Wi-Fi Direct Info, then type the name (it starts with DIRECT-) and the password. The Mac joins the camera's network by itself.

"Advanced: connect by address" holds the old IP address field. Rimix remembers cameras you have used. In App settings, the Connection section lets you choose whether Wi-Fi joins happen automatically or ask first, pick the webcam, and forget remembered cameras.

If the camera's Wi-Fi drops while you work, Rimix tries to rejoin it by itself from the second reconnect attempt. If that does not work, join the camera's network again from the Wi-Fi menu, or press Disconnect and connect again.

## Features

- Live view, shooting, recording and the main exposure settings from the desktop.
- Wi-Fi joining by QR code, by typed name and password, or from the nearby list. If the network is not visible yet, Rimix says "This Mac can't see <name> yet" and tells you to open Wi-Fi Direct Info on the camera and try again.
- Reconnecting: from the second attempt Rimix rejoins the camera's Wi-Fi by itself. From attempt 5 the top bar says "Reconnecting… if the camera's Wi-Fi dropped, rejoin it from the Wi-Fi menu".
- Per-model settings: Rimix handles older and newer camera generations differently. Hover the firmware in the top bar to see the generation.
- Interval timer: in the Scopes and timer panel, open the Interval timer row. Set Every (seconds, at least 2) and Shots (leave empty for no limit), then press Start. The dock shows, for example, "Interval 3 of 20, next in 4 s". Shots skipped because the camera was busy are counted as missed. A dropped link does not stop the run; Disconnect and Stop do.

## Logs

Rimix writes a log to `~/Library/Application Support/Rimix/logs/`, one file per day (`rimix.log.YYYY-MM-DD`), and keeps the last 7. In App settings, the Logs row has a Show log folder button. Wi-Fi passwords are kept out of the log. Set `RIMIX_LOG` to change the level, for example `RIMIX_LOG=debug`. Attach the newest log file to bug reports.

## Command line Wi-Fi

```
rimix wifi status
rimix wifi join "DIRECT-xxxx"
printf '%s\n' 'the password' | rimix wifi join "DIRECT-xxxx" --password-stdin
```

`status` prints the Wi-Fi device, the network it is on and its address. `join` exits with 3 when the network is not visible and 4 for any other failure.

## Access Authentication

Bodies from 2022 on (a7 IV, a7R V, a6700, a7C II, ZV-E1, FX3, FX30) can require a username and password before they accept a connection. The ZV-E10 does not. For these cameras, open "This camera uses Access Authentication" on the connect screen and enter the Username and Password shown on the camera. The first time, Rimix shows "This camera's fingerprint is <sha256>. Check it against Access Authen. Info on the camera." Compare it with the camera's screen and press "It matches, connect". A lock in the top bar shows the connection is authenticated.

Rimix stores the username and the fingerprint with the remembered camera, never the password. If the camera refuses the login, Rimix stops reconnecting, because a camera may refuse all connections after repeated bad attempts. Fix the details and connect again.

On the command line:

```
printf '%s\n' 'the password' | rimix connect 192.168.122.1 --ssh-user USER --ssh-password-stdin --fingerprint FINGERPRINT
```

Without `--fingerprint`, the tool prints the fingerprint and asks you to confirm it on the camera and pass it.

## Release a version

The first time, try the workflow with a throwaway tag: push `v0.0.0-test`, check the draft release it makes, then delete that draft release on GitHub and the tag (`git push origin :refs/tags/v0.0.0-test` and `git tag -d v0.0.0-test`).

Tag the commit and push the tag:

```
git tag v0.2.0
git push origin v0.2.0
```

The Release workflow builds a DMG (universal Mac app) and an NSIS installer for Windows into one draft release. In detail: it creates the draft with the DMG attached, then builds the Windows installer (`Rimix_<version>_x64-setup.exe`) and adds it to the same draft. Review the draft, then publish it.

For a signed and notarized build, add these repository secrets (Settings, Secrets and variables, Actions): `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`. Without them the workflow still runs and the DMG is unsigned.

### Building a release on your own Mac

On recent macOS (seen on macOS 27), run `cargo build --release -p rimix-desktop --features tauri/custom-protocol` before `pnpm -C apps/desktop exec tauri build`. Without it the build can fail with "can't find crate for serde_derive", because the linker produces proc-macro libraries that macOS refuses to load when the deployment target is 14.0. Also run the build with `CI=true` to skip the DMG window-layout step, which needs Finder automation permission.

## Develop without a camera

Environment variables let you run the whole app against the simulated camera. `RIMIX_WIFI=fake` makes Wi-Fi joins succeed instantly, and `RIMIX_CAMERA_ADDR=127.0.0.1:15740` says where the "camera" is after joining. To try Access Authentication, start the simulator with `--ssh cam:12345678` and set `RIMIX_SSH_PORT` to the SSH port the simulator prints.

```
cargo run -p sony-ptpip-sim -- --port 15740
cd apps/desktop
RIMIX_WIFI=fake RIMIX_CAMERA_ADDR=127.0.0.1:15740 pnpm tauri dev
```

With Access Authentication:

```
cargo run -p sony-ptpip-sim -- --port 15740 --ssh cam:12345678
RIMIX_WIFI=fake RIMIX_CAMERA_ADDR=127.0.0.1:15740 RIMIX_SSH_PORT=<port> pnpm tauri dev
```

On the command line tool, the matching flag is `--ssh-port`.

## Testing with a real camera

[docs/camera-testing.md](docs/camera-testing.md) is a step-by-step checklist for a Sony ZV-E10, written for the camera's owner. It takes about 30 minutes and says what to send back after each step.

## Research

Before any code, the project started as research into how Sony's own apps connect:

- `reports/Sony camera wireless desktop control.html` is the readable report. Open it in a browser.
- `reports/Sony camera wireless desktop control.md` is the same report as plain text.
- `research_notes/` holds the detailed notes behind the report, with sources.

## Design directions

`design/index.html` is a gallery of design directions for the app, each built as a full mockup in `design/mockups/`. Round two keeps the Viewfinder layout and the Remote Console settings list and explores six glass variants of them. `design/inspiration.md` collects how existing camera apps look, with links. Open the gallery in a browser to compare.

## Licence

Apache-2.0. See `LICENSE`.
