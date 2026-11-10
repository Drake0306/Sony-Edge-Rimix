# Sony Edge Rimix

A desktop companion for Sony Alpha cameras that connects over Wi-Fi, the way Sony's phone app does, instead of needing a USB cable. Mac first, Windows next.

## Status

Phase 1 works against a simulated camera: the protocol library, a command line tool and a desktop app all run and pass their tests there. None of it has been tried on a real camera yet. To change that, follow [docs/camera-testing.md](docs/camera-testing.md).

## What's inside

- `crates/sony-ptpip` is the protocol library that talks to the camera over Wi-Fi (PTP/IP).
- `crates/sony-ptpip-sim` is a simulated camera (`sony-ptpip-sim`) for trying things without hardware.
- `crates/rimix-cli` is the command line tool (`rimix`): discover, connect, props, set, step, shoot, record, liveview, watch.
- `apps/desktop` is the desktop app, built with Tauri.
- `docs/` holds the design and the real-camera test guide.

## Build and run

You need:

- Rust 1.93 or newer, installed with [rustup](https://rustup.rs)
- Node 20 or newer, and pnpm
- Xcode command line tools (`xcode-select --install`) on a Mac
- The Tauri command line tool: `cargo install tauri-cli --version '^2'`

Build and test everything:

```
cargo build --release
cargo test --workspace
```

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
