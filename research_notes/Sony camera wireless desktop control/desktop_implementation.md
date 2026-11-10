# Desktop implementation options for a cross-platform Sony Alpha wireless control app (macOS first, Windows second)

Research date: 2026-11-10. Scope: how to build the desktop app *around* Sony's C++ Camera Remote SDK (CrSDK) and/or PTP/IP. Camera-protocol details themselves are out of scope (covered by other researchers).

Version snapshot as of 2026-11-10 (from package registries / endoflife.date, fetched directly):
- Electron 45.2.3 (latest line 45, released 2026-10-20; 44.x and 43.x still supported) — [endoflife.date/electron](https://endoflife.date/api/electron.json)
- Tauri crate 2.12.5 (2026-11-06), `@tauri-apps/cli` 2.12.4, with a `next` tag of 3.0.0-alpha.6 — [crates.io/tauri](https://crates.io/api/v1/crates/tauri), [npm @tauri-apps/cli](https://registry.npmjs.org/@tauri-apps/cli)
- Qt 6.12.0 LTS (2026-09-30); Qt 6.11.3 — [endoflife.date/qt](https://endoflife.date/api/qt.json)
- .NET 10.0.13 LTS; .NET 8 and .NET 9 both reach EOL on 2026-11-10 — [endoflife.date/dotnet](https://endoflife.date/api/dotnet.json)
- Avalonia 12.1.6 (NuGet) — [NuGet flatcontainer](https://api.nuget.org/v3-flatcontainer/avalonia/index.json)
- Flutter stable 3.47.9 (2026-10-29, Dart 3.13.7) — [flutter releases_macos.json](https://storage.googleapis.com/flutter_infra_release/releases/releases_macos.json)
- koffi 3.3.7 (2026-11-05), node-addon-api 8.9.2, electron-builder 26.18.0, @electron-forge/cli 8.0.1 — [npm registry](https://registry.npmjs.org/koffi)
- btleplug 0.13.5 (2026-10-10), mdns-sd 0.21.7, zxing-cpp Rust crate 0.5.3, cxx 1.0.205 — [crates.io](https://crates.io/api/v1/crates/btleplug)
- zxing-wasm 3.1.6, bonjour-service 1.4.4, @abandonware/noble 1.9.2-26 (last publish 2025-02-09), jsqr 1.4.0 (last publish 2021) — [npm registry](https://registry.npmjs.org/zxing-wasm)
- pub.dev: camera_desktop 2.0.0, ffigen 23.0.0, flutter_blue_plus 2.3.15, universal_ble 3.2.0, bonsoir 7.1.6, nsd 5.0.1 — [pub.dev API](https://pub.dev/api/packages/camera_desktop)

---

## Key Question 1: Framework choice for wrapping the native C++ CrSDK (Electron, Tauri v2, Qt 6, Flutter, .NET/Avalonia/MAUI, native Swift + WinUI)

### Takeaway
Every framework can call the CrSDK, but the hard parts are identical everywhere: (a) marshalling SDK callbacks that fire on SDK-owned threads back onto the UI thread, (b) moving 15-30 JPEG frames/s to the UI without JSON-serialising them, and (c) getting Sony's dylibs/DLLs and the `CrAdapter/` plugin folder into a signed, notarised bundle. Electron and Tauri have the best-documented tooling for a web-tech developer; Qt and native Swift have the least friction for the C++ SDK itself; Flutter and .NET sit in the middle. The two real-world CrSDK integrations found in the wild (eclipseClick, camera-remote-web-api) both chose to isolate the SDK in a separate native process rather than load it in-process (see Q2).

### Cited Findings

**Electron**
- Electron requires code signing then notarization on macOS; Forge uses `@electron/packager`, `@electron/osx-sign`, `@electron/notarize`; `safeStorage`, `autoUpdater` (Squirrel.Mac) etc. misbehave with unsigned/ad-hoc builds — [Electron code signing docs](https://www.electronjs.org/docs/latest/tutorial/code-signing)
- Windows: since June 2023 Microsoft requires an EV certificate to avoid SmartScreen warnings; EV keys must live on FIPS 140 L2 hardware; Azure Artifact Signing (formerly Trusted Signing) is the cheapest route and removes SmartScreen warnings but is country-limited; `@electron/windows-sign` powers `windowsSign` across Forge/packager/Squirrel/WiX; MSIX/Store is a separate guide — [Electron code signing docs](https://www.electronjs.org/docs/latest/tutorial/code-signing)
- Electron's default session denies `getUserMedia`; camera access must be granted in the main process via the session permission handler; on macOS `NSCameraUsageDescription` must be added via electron-builder `mac.extendInfo` — [Dynamsoft Electron barcode tutorial](https://www.dynamsoft.com/codepool/electron-barcode-mrz-document-scanner.html)
- Koffi (FFI) callbacks: "transient" callbacks are only valid during the C call; "registered" callbacks (`koffi.register`) can be called any time, max 8192 live registrations; JS callbacks always run on the main thread — Koffi queues calls made from other threads and runs them when the event loop is free, so blocking the main thread waiting on a secondary thread deadlocks — [koffi.dev/callbacks](https://koffi.dev/callbacks)
- node-addon-api: native threads must not call functions needing `Napi::Env`; use `Napi::ThreadSafeFunction` / `TypedThreadSafeFunction`, whose calls are queued and executed on the main loop thread; no other threads may use the TSF after the finalize callback — [node-addon-api threadsafe.md (vendored in koffi)](https://unpkg.com/koffi@2.12.3/vendor/node-addon-api/doc/threadsafe.md)
- A 2017 Electron issue states SharedArrayBuffer only works between threads, not between processes — [electron/electron#10409](https://github.com/electron/electron/issues/10409)
- Electron's experimental `sharedTexture` API imports platform shared-texture handles, converts them to `VideoFrame`, and can be sent across processes (`importSharedTexture` in main, `setSharedTextureReceiver` in renderer, `sendSharedTexture` with a 1000 ms timeout); all methods marked Experimental and may be removed — [Electron sharedTexture docs](https://www.electronjs.org/docs/latest/api/shared-texture)
- Electron bundle size: one self-published signed+notarized macOS test measured 182 MB .dmg / 410 MB unpacked for Electron vs 8.1 MB / 22 MB for Tauri; another measured 244 MB vs 8.6 MB `.app`; universal (x64+arm64) builds roughly double Electron artefacts to ~250-300 MB — [dev.to gethopp comparison](https://dev.to/gethopp/tauri-vs-electron-performance-bundle-size-and-the-real-trade-offs-1el4); [OpenReplay comparison](https://blog.openreplay.com/comparing-electron-tauri-desktop-applications/)
- Canon precedent: `@brick-a-brack/napi-canon-cameras` wraps EDSDK for Node (N-API); live view only partly implemented; does not ship Canon SDK files, you must supply them and build a tarball; originally for a photo-booth app — [npm @brick-a-brack/napi-canon-cameras](https://npmjs.com/package/@brick-a-brack/napi-canon-cameras)
- Sony precedent (Node): `camera-remote-web-api` 1.3.7 (MIT, last publish 2026-04-28) is a TS/JS SDK "built on Sony Camera Remote SDK V2.01.00" that starts a native binary and exposes REST + WebSocket live view + SSE; "npm automatically installs the correct native binary for your platform (macOS ARM64, Linux x64/ARM64, Windows x64)"; the browser client is documented for "React, Next.js client components, or Electron renderers" — [npm registry readme](https://registry.npmjs.org/camera-remote-web-api)
- `@alpha-sdk/crsdk` 0.1.0 (MIT) is a CLI, not a binding: accepts EULA, extracts the Sony zip, builds Sony's `RemoteCli` sample — [npm registry readme](https://registry.npmjs.org/@alpha-sdk%2Fcrsdk)

**Tauri v2**
- Sidecars: list binaries under `bundle.externalBin` in `tauri.conf.json`; each needs a copy suffixed with the target triple (`rustc --print host-tuple`); grant `shell:allow-execute` / `shell:allow-spawn` with `"sidecar": true`; from Rust use `app.shell().sidecar(name).spawn()` which yields a `CommandEvent` receiver (`Stdout`, …) and a child handle usable for stdin; the page does not cover signing or cleanup-on-exit — [Tauri v2 sidecar docs](https://v2.tauri.app/develop/sidecar/)
- Tauri does not clean up sidecars itself; store the child handle and kill it in `RunEvent::ExitRequested` — [tauri-sidecar skill page](https://stuffbucket.github.io/skills/catalog/tauri-sidecar/)
- IPC: normal command return values are JSON-serialised, which "can slow down your application" for large data; return `tauri::ipc::Response::new(bytes)` for array buffers; channels (`tauri::ipc::Channel<&[u8]>`) are "the recommended mechanism for streaming data" and preserve ordering, whereas events are JSON-only, async, and may be processed out of order — [Tauri v2 Calling Rust docs](https://v2.tauri.app/develop/calling-rust/)
- macOS signing: `bundle.macOS.signingIdentity` or `APPLE_SIGNING_IDENTITY`; notarization via `APPLE_API_ISSUER/APPLE_API_KEY/APPLE_API_KEY_PATH` or `APPLE_ID/APPLE_PASSWORD/APPLE_TEAM_ID`; notarization required for Developer ID certs; free accounts cannot notarize; ad-hoc identity `"-"` possible but users must allow in Privacy & Security; the page does not document hardened-runtime entitlements or nested dylib/sidecar signing — [Tauri v2 macOS signing](https://v2.tauri.app/distribute/sign/macos/)
- Rust FFI to C++: `cxx` crate 1.0.205 (86M downloads) — [crates.io/cxx](https://crates.io/api/v1/crates/cxx); the `zxing-cpp` Rust crate's bindings are "automatically generated by rust-bindgen" — [docs.rs zxing-cpp](https://docs.rs/crate/zxing-cpp/0.5.1/source/src/lib.rs)
- Tauri camera plugins exist (`crabcamera`, a Rust camera/audio plugin for Tauri) but none wrap a vendor tethering SDK — [docs.rs crabcamera](https://docs.rs/crate/crabcamera/0.5.0/source/README.md)

**Qt 6**
- Qt LGPL: with dynamic linking "keeping application code proprietary is allowed but not required"; static linking "may take the application outside 'work that uses the library'"; Qt recommends a commercial licence "when there is doubt" — [qt.io LGPL obligations](https://www.qt.io/licensing/open-source-lgpl-obligations)
- qDslrDashboard is described as a cross-platform app controlling Nikon, Canon and Sony cameras; a 2016 forum thread reports Sony A7 worked over Wi-Fi (not USB) by joining the camera's network and using the Sony icon — [dslrdashboard forum](https://dslrdashboard.info/phpBB3/viewtopic.php?p=4642) (old; licence/source status not confirmed)
- A Qt mailing-list thread from 2019 discusses enabling hardened runtime for notarizing Qt builds — [Qt interest list](https://lists.qt-project.org/pipermail/interest/2019-July/033419.html)

**Flutter desktop**
- `NativeCallable.listener` (since Dart 3.1) creates a callback native code can invoke from *any* thread; arguments are sent over a SendPort to the creating isolate; the isolate stays alive until `close()`; by contrast `Pointer.fromFunction` aborts the process if called from another thread — [api.flutter.dev NativeCallable.listener](https://api.flutter.dev/flutter/dart-ffi/NativeCallable/NativeCallable.listener.html)
- Each `NativeCallable.listener` has its own receive port, so callbacks with different signatures may arrive out of order relative to the native thread — [dart-lang/native#1760](https://github.com/dart-lang/native/issues/1760)
- `camera_desktop` 2.0.0 (AVFoundation on macOS, Media Foundation on Windows) exposes an FFI-based BGRA image stream for webcam frames — [pub.dev camera_desktop](https://pub.dev/packages/camera_desktop); `camera_pro` compiles a C core via Flutter's native-assets hook — [pub.dev camera_pro](https://pub.dev/documentation/camera_pro/0.0.1/)
- No Flutter wrapper of any vendor tethering SDK (Canon/Sony/gphoto2) was found — [search result](https://pub.dev/packages/camera_desktop)

**.NET (Avalonia / MAUI)**
- `[UnmanagedCallersOnly]` methods can be passed as `delegate* unmanaged` function pointers to native code; must be static, blittable-args only, not called from managed code; available .NET 5+ — [MS Learn UnmanagedCallersOnlyAttribute](https://learn.microsoft.com/en-us/dotnet/api/system.runtime.interopservices.unmanagedcallersonlyattribute)
- Avalonia macOS: `dotnet-bundle` is unmaintained; docs now recommend Parcel (bundle, Info.plist, signing, notarization, DMG in one command); JIT entitlement (`com.apple.security.cs.allow-jit`) is required to run Avalonia under hardened runtime — [Avalonia macOS deployment docs](https://docs.avaloniaui.net/docs/deployment/macOS); [Avalonia blog deployment guide](https://avaloniaui.net/blog/the-definitive-guide-to-building-and-deploying-avalonia-applications-for-macos)
- Microsoft: the .NET appHost must be notarized with at least the default .NET entitlements — [MS Learn macOS notarization issues](https://learn.microsoft.com/dotnet/core/install/macos-notarization-issues)
- An Avalonia app notarized OK but stapling failed (error 65) when signed with `--deep`; sign nested binaries individually — [Apple forums thread 688182](https://developer.apple.com/forums/thread/688182)
- A Nikon tethering desktop app was built in C#/WPF on the Nikon SDK (shows the .NET + vendor SDK pattern) — [codementor profile](https://www.codementor.io/@helenavargas)
- digiCamControl is described as open source and supporting "many Nikon, Canon, Sony SLR models", but another (older) source claims Nikon-only and Windows-only; sources conflict — [alternativeto digiCamControl](https://alternativeto.net/software/ncc)

**Native Swift (macOS) + WinUI**
- `rocc` (simonmitchell/rocc): Swift framework for Wi-Fi control/transfer of Sony cameras, MIT, 148 stars, last push 2025-03-23, SwiftPM/Carthage, "tried and tested codebase" used by the Camrote iOS app — [GitHub rocc](https://github.com/simonmitchell/rocc); [GitHub API](https://api.github.com/repos/simonmitchell/rocc)
- Apple: notarization requires Hardened Runtime; dylibs built against pre-10.9 SDK fail notarization; a dylib failing to load under hardened runtime was fixed with `com.apple.security.cs.allow-unsigned-executable-memory` in one case — [Apple forums 659964](https://developer.apple.com/forums/thread/659964); [Apple forums 124810](https://developer.apple.com/forums/thread/124810); [Xcode hardened runtime doc](https://developer.apple.com/documentation/xcode/configuring-the-hardened-runtime.md)

**CrSDK-specific packaging gotchas (apply to all frameworks)**
- Sony ships separate arm64 and x86_64 dylibs (not lipo-merged) and bundles its own libusb; `libCr_Core` hardcodes plugin lookup to `Contents/Frameworks/CrAdapter` (app-bundle layout) with no fallback to the binary's directory; author copies `CrAdapter/` to both locations in a CMake POST_BUILD step; macOS `ptpcamerad` races the SDK for the USB PTP session; SDK segfaults if `Init()` is called after `Release()` in the same process (workaround: exit the bridge process after disconnect); stale PTP session gives `SessionAlreadyOpened (0x8210)` — [eclipseClick blog](https://eclipseclick-website.pages.dev/blog/sony-crsdk-macos-apple-silicon)
- CrSDKPy docs: "the vendor SDK resolves `CrAdapter` against the host executable's own directory, so a runtime staged anywhere else is not found" — [PyPI CrSDKPy](https://pypi.org/pypi/crsdkpy/json)
- Sony's licence allows commercial sale of apps built with the SDK, but wrappers "do not redistribute Sony headers, libraries, DLLs, samples" — users obtain the SDK themselves — [PyPI CrSDKPy](https://pypi.org/pypi/crsdkpy/json); [Sony SDK licence](https://support.d-imaging.sony.co.jp/app/sdk/licenseagreement_d/en-US.html)

### Inferences
- The CrSDK's `Contents/Frameworks/CrAdapter` hardcode means a macOS `.app` layout is the natural home for the SDK; frameworks that produce a proper bundle (Xcode, Tauri, electron-builder, Parcel/Avalonia) all work, but a sidecar binary placed in `Contents/MacOS/` needs the `CrAdapter/` copy next to it as well.
- In-process loading (N-API addon, dart:ffi, P/Invoke, cxx) is feasible but concentrates risk: the documented SDK segfault on re-`Init()` would take the whole UI down. That is why both known Sony/CrSDK integrations (eclipseClick, camera-remote-web-api) run the SDK in a child process.
- For live view at 15-30 fps, the key rule in every framework is "never JSON-encode the JPEG": Electron -> binary IPC/`Buffer` or a localhost WebSocket/MJPEG; Tauri -> `ipc::Response`/`Channel<&[u8]>` or a localhost HTTP server; Flutter -> `dart:ffi` zero-copy buffer to `ui.decodeImageFromPixels`/`Image.memory`; .NET -> `Span<byte>`/`WriteableBitmap`; Qt/Swift -> native `QImage`/`CGImage`. A 30 fps stream of ~100-300 KB JPEGs is 3-9 MB/s, comfortably inside any of these paths; the risk is per-frame allocation/decoding on the UI thread, not bandwidth.
- Electron's `sharedTexture` is aimed at GPU-texture producers (e.g. game capture); for JPEG frames from a camera SDK a plain binary message plus `createImageBitmap`/`<img>` blob or a WebCodecs `ImageDecoder` is simpler and not experimental.
- The .NET 8/9 EOL date (today) means a new .NET project should target .NET 10 LTS.

### Gaps
- No published benchmark of CrSDK (or EDSDK) live-view fps in Electron, Tauri, Flutter or .NET was found; frame-rate claims must be measured on real hardware.
- Electron docs do not state which version introduced `sharedTexture`; its platform matrix is unspecified.
- No open-source Qt 6 or Flutter app wrapping CrSDK was found; qDslrDashboard's licence/source status could not be confirmed.
- Tauri docs do not describe how bundled dylibs and sidecars are signed/notarized with hardened runtime; must be verified by building.
- No source confirmed whether the CrSDK dylibs are themselves signed with a Sony Developer ID or need re-signing; test with `codesign -dv`.

---

## Key Question 2: Sidecar architecture (C++ helper process + local WebSocket/gRPC/stdio)

### Takeaway
Running CrSDK in a separate helper process is the de-facto pattern in the Sony-wrapper ecosystem: eclipseClick uses a C++ child process speaking JSON-RPC over stdio; camera-remote-web-api ships a native binary exposing REST + WebSocket live view + SSE; CrSDKPy uses a user-built `crsdkpy_host` executable. The cost is a second binary to build, sign and keep alive; the benefit is crash isolation from a vendor SDK with documented segfault and process-lifetime quirks.

### Cited Findings
- eclipseClick: "The Sony bridge runs as a child process of eclipseClick and talks to it over JSON-RPC on stdio"; bridge is C++/CMake with `std::thread`, IOKit, cJSON; macOS-specific: exit the bridge process after disconnect to avoid the `Init()`-after-`Release()` segfault; a forked subprocess loops `killall ptpcamerad` — [eclipseClick blog](https://eclipseclick-website.pages.dev/blog/sony-crsdk-macos-apple-silicon)
- camera-remote-web-api: `CameraManager` "Starts the native binary, discovers cameras, auto-connects, and handles reconnection"; options include `port`, `autoPort`, `connectionMode: 'remote' | 'remote-transfer' | 'contents'`, `autoReconnect`, `pollInterval`; events `camera-found/lost/ready/disconnected`; per-camera SSE events (`propertyChanged`, `downloadComplete`, `transferProgress`); live view over WebSocket; browser client usable from Electron renderers — [npm registry readme](https://registry.npmjs.org/camera-remote-web-api)
- CrSDKPy 0.1.0b3: pure-Python wheel plus a user-built `crsdkpy_host` (CMake, `-DCRSDK_ROOT=`); "There is no lawful prebuilt binary to publish"; hardware-validated only on ILME-FX3A / CRSDK 2.02.00 / USB; includes a simulator backend — [PyPI CrSDKPy](https://pypi.org/pypi/crsdkpy/json); [GitHub CrSDKPy](https://github.com/PICKLERICK2005/CrSDKPy)
- Tauri sidecar mechanics (externalBin, target-triple naming, shell capabilities, stdout events, stdin writes) — [Tauri v2 sidecar docs](https://v2.tauri.app/develop/sidecar/); Tauri does not kill sidecars automatically — [tauri-sidecar skill](https://stuffbucket.github.io/skills/catalog/tauri-sidecar/)
- Tauri channels are the recommended ordered streaming mechanism; `ipc::Response` for binary — [Tauri Calling Rust](https://v2.tauri.app/develop/calling-rust/)
- Sony's latest SDK is listed as Camera Remote SDK Ver. 2.02.00 on the Japanese support site — [Sony SDK portal (ja)](https://support.d-imaging.sony.co.jp/app/sdk/ja/index.html)

### Inferences
- A sidecar lets the UI layer be anything (Electron, Tauri, Flutter, Swift) while the SDK code is a single C++ codebase compiled per-OS; this also means one place to handle the macOS `CrAdapter/` path and the Windows DLL set.
- Transport choice: stdio JSON-RPC (eclipseClick) is simplest and needs no port, but is awkward for a 30 fps binary stream; a localhost WebSocket (camera-remote-web-api) handles binary frames naturally and lets a webview `<img>`/canvas consume them directly; gRPC adds codegen and a heavier dependency with no clear benefit for a single local client. A pragmatic hybrid is stdio/JSON for control and a localhost WebSocket or MJPEG endpoint for live view.
- Because the helper is a separate Mach-O/PE binary, it must be individually signed with hardened runtime and bundled where the SDK expects `CrAdapter/`; on Windows it must sit beside the Sony DLLs.
- On macOS, process lifetime should be handled explicitly (kill on `ExitRequested`/`before-quit`) and the helper should tolerate being restarted after each disconnect, per eclipseClick's workaround.

### Gaps
- No public source code for eclipseClick's bridge or camera-remote-web-api's native binary was located (the npm package's `repository` field is empty), so their internal threading model is unknown.
- No measured latency comparison of stdio vs WebSocket vs gRPC for local live view was found.

---

## Key Question 3: Scanning the camera's QR code from a laptop webcam

### Takeaway
In a webview-based app (Electron/Tauri), `getUserMedia` + `zxing-wasm` (ZXing-C++ compiled to WebAssembly, ~1 MB reader build, actively maintained) is the lowest-friction path; native apps can use Apple Vision `VNDetectBarcodesRequest` (macOS 10.13+) and, on Windows, either the built-in camera barcode scanner (Windows 10 1803+) or zxing-cpp directly. On macOS the app needs `NSCameraUsageDescription` and the user gets a TCC prompt; on Windows desktop apps are governed by one global "Let desktop apps access your camera" switch, not a per-app prompt.

### Cited Findings
- zxing-wasm: "ZXing-C++ WebAssembly as an ES/CJS module with types. Read or write barcodes in various JS runtimes: Web, Node.js, Bun, and Deno"; reader-only subpath wasm is about 966 KiB; repo 294 stars, MIT, pushed 2026-10-31; npm 3.1.6 — [GitHub Sec-ant/zxing-wasm](https://github.com/Sec-ant/zxing-wasm); [jsDelivr README](https://cdn.jsdelivr.net/npm/zxing-wasm@3.0.2/README.md)
- zxing-cpp (C++): 2,095 stars, Apache-2.0, pushed 2026-11-07 — [GitHub zxing-cpp](https://github.com/zxing-cpp/zxing-cpp); Rust crate `zxing-cpp` 0.5.3 with bindgen-generated bindings — [docs.rs](https://docs.rs/crate/zxing-cpp/0.5.1/source/src/lib.rs)
- Rust-native alternative `rxing` (Rust port of ZXing) with `rxing-wasm` and a Node binding `@rxing/rxing` at 0.0.2; `wascan` wraps rxing with camera access for browsers — [rxing issue](https://github.com/rxing-core/rxing/issues/1); [wascan README](https://docs.rs/crate/wascan/0.1.2/source/README.md)
- `jsqr` last published 2021-04 (1.4.0); `@sec-ant/barcode-detector` last published 2023-09 — [npm registry](https://registry.npmjs.org/jsqr)
- Apple Vision `VNDetectBarcodesRequest`: macOS 10.13+; returns `[VNBarcodeObservation]`; `symbologies` property; `supportedSymbologies()`; current revision `VNDetectBarcodesRequestRevision3` — [Apple docs JSON](https://developer.apple.com/tutorials/data/documentation/vision/vndetectbarcodesrequest.json)
- Windows camera barcode scanner: "a standard lens camera with auto-focus capabilities… combined with a software decoder, which Windows dynamically pairs to create a fully functional barcode scanner for WinRT apps"; decoder by Digimarc; requires Windows 10 1803+ — [MS Learn camera barcode scanner](https://learn.microsoft.com/en-us/windows/apps/develop/devices-sensors/pos/camerabarcode)
- Electron: default session denies `getUserMedia`; grant `media`/`camera` in main-process `setPermissionRequestHandler`; add `NSCameraUsageDescription` via electron-builder `mac.extendInfo`; macOS prompts on first launch, Windows generally does not prompt desktop apps — [Dynamsoft Electron tutorial](https://www.dynamsoft.com/codepool/electron-barcode-mrz-document-scanner.html)
- Apple `NSCameraUsageDescription` Info.plist key — [Apple docs](https://developer.apple.com/tutorials/data/documentation/bundleresources/information-property-list/nscamerausagedescription.md)
- Windows: "Desktop apps cannot be individually toggled, but access for those apps can be controlled using Let desktop apps access your camera"; added in Windows 10 1903; desktop apps "might not appear in the list… They might still be able to access the camera or microphone even when these settings are turned off" — [Microsoft support: camera privacy](https://support.microsoft.com/windows/privacy/windows-camera-microphone-and-privacy); [Manage app permissions for a camera](https://support.microsoft.com/en-US/windows/privacy/manage-app-permissions-for-a-camera-in-windows)
- Sony manuals: the camera shows a QR code with SSID `DIRECT-xxxx` and password; scanning registers SSID+password on the phone; QR becomes invalid after `[SSID/PW Reset]` or `[Edit Device Name]` — [Sony a6300 manual](https://www.manualslib.com/manual/1201768/Sony-A6300.html?page=212); [Sony a7S III help guide](https://www.manualslib.com/manual/1981148/Sony-A7siii.html?page=419)
- Flutter: `camera_desktop` exposes BGRA frames via FFI on macOS/Windows — [pub.dev camera_desktop](https://pub.dev/packages/camera_desktop); Dynamsoft shows a lightweight Flutter desktop camera plugin for barcode scanning — [Dynamsoft Flutter desktop](https://dynamsoft.com/codepool/flutter-camera-sdk-1d-2d-barcode-scanner.html)

### Inferences
- For Electron/Tauri the whole QR flow (camera preview + decode) can live in the webview with zero native code: `navigator.mediaDevices.getUserMedia` -> `<video>` -> `canvas.getImageData` (or `ImageBitmap`) -> `zxing-wasm readBarcodes`. Tauri on macOS uses WKWebView, so `getUserMedia` goes through WebKit and the host app still needs `NSCameraUsageDescription` (same as Electron).
- Native stacks: Swift -> AVCaptureSession + Vision (no third-party dependency); WinUI/.NET -> `MediaCapture` + `Windows.Devices.PointOfService.BarcodeScanner` or ZXing.Net; Qt -> `QCamera`/`QVideoSink` frames into zxing-cpp.
- Decoding at camera-preview rate is unnecessary; sampling 5-10 fps for QR is enough and keeps CPU low.

### Gaps
- The exact Sony QR payload format (field layout containing SSID/password/model) was not found in any public document; it must be reverse-engineered by scanning a real camera QR. Do not rely on the unverified community pattern.
- Whether WinRT `BarcodeScanner` works from an unpackaged Win32/Electron process (vs. packaged MSIX) was not confirmed from the page retrieved.
- No source found on whether Windows 11 24H2+ ever shows a per-app camera consent prompt for packaged (MSIX) desktop apps.

---

## Key Question 4: Bluetooth Low Energy from a desktop app

### Takeaway
BLE is usable from all candidate stacks (CoreBluetooth on macOS, WinRT Bluetooth on Windows) via btleplug (Rust), SimpleBLE (C++), noble (Node), or Electron's Web Bluetooth, but macOS hides peripheral MAC addresses behind per-host UUIDs and apps cannot programmatically unpair; the Android open-source Sony BLE remote shows the protocol is limited to button-press emulation (shutter/focus/zoom) with minimal status, not a settings API.

### Cited Findings
- btleplug 0.13.5 (1,194 stars, pushed 2026-10-10): macOS 10.15+ requires an app bundle whose Info.plist has `NSBluetoothAlwaysUsageDescription` (for CLI tools, grant Bluetooth to the terminal in Privacy settings); `adapter_address()` returns `Ok(None)` on CoreBluetooth; `PeripheralId` is the portable identity; no Windows-specific caveats listed, Windows 10 minimum — [docs.rs btleplug](https://docs.rs/crate/btleplug/latest)
- SimpleBLE platform notes: macOS/iOS "does not expose the raw MAC address… assigns a unique UUID to each peripheral" persistent per host but different across hosts; apps "can't unpair or forget a device programmatically"; the OS may keep the connection alive after the app disconnects; CoreBluetooth reports only the last manufacturer-data entry; Windows: 32-bit builds on 64-bit may report Bluetooth disabled; MSIX packages need `<DeviceCapability Name="bluetooth" />`, unpackaged executables don't — [SimpleBLE platform notes](https://docs.simpleble.org/simpleble/platform_notes)
- SimpleBLE FAQ claims the UUID "is not persistent across reboots" — conflicts with its own platform notes; treat cautiously — [SimpleBLE FAQ](https://simpleble.readthedocs.io/en/latest/simpleble/faq.html)
- MathWorks: on Windows the address is the 48-bit MAC; on macOS "a random 128-bit OS-assigned address" — [MathWorks ble](https://www.mathworks.com/help/matlab/ref/ble.html)
- Electron Web Bluetooth: handle `select-bluetooth-device` on `webContents`, call `event.preventDefault()` and the callback with a device id; `ses.setBluetoothPairingHandler` is for Windows/Linux PIN flows — "macOS does not require a handler because macOS handles the pairing automatically" — [Electron Device Access](https://electronjs.org/docs/latest/tutorial/devices)
- Electron added Web Bluetooth PIN pairing support in PR #35416 — [commit mirror](https://ayakael.net/mirrors/electron/commit/697a219bcb7bc520bdf2d39a76387e2f99869a2a)
- `@abandonware/noble` 1.9.2-26, MIT, 601 stars, last published/pushed 2025-02-09 (community-maintained fork) — [GitHub abandonware/noble](https://github.com/abandonware/noble); [npm](https://registry.npmjs.org/@abandonware%2Fnoble)
- Flutter: `flutter_blue_plus` 2.3.15 (2026-11-05) and `universal_ble` 3.2.0 (2026-10-30) exist on pub.dev — [pub.dev API](https://pub.dev/api/packages/flutter_blue_plus)
- α-Remote (Staacks/alpharemote): Kotlin Android BLE remote for Sony cameras, GPL-3.0, 150 stars, pushed 2026-11-01; works with any camera compatible with Sony's physical Bluetooth remote; confirmed on α6100/6400/6600/6700, α7 III/IV/C II, α7R III/IV/V, α9, ZV-E10; the author states controls for ISO/shutter/aperture are "not possible via Bluetooth (at least with the protocol that I am aware of)" and status feedback is minimal (focus, shutter state) — [GitHub alpharemote](https://github.com/Staacks/alpharemote); [F-Droid](https://f-droid.org/packages/org.staacks.alpharemote/)

### Inferences
- Because macOS never exposes the camera's BLE MAC, any flow that needs a MAC (e.g. CrSDK's wired-LAN/Wi-Fi connection-info objects that take a MAC) cannot obtain it via CoreBluetooth; it must come from the QR code, mDNS/SSDP, ARP, or the SDK's own enumeration. On Windows the MAC is available from WinRT.
- Electron's Web Bluetooth path requires no native module and is the fastest way to prototype a "wake camera over BLE" button, but it is Chromium's GATT-only API (no advertisement scanning without flags) and the device chooser must be implemented by the app.
- Phone-style BLE pairing with Sony cameras (camera menu "Bluetooth Pairing") produces a bonded relationship; SimpleBLE's note that apps cannot unpair on macOS means "forget camera" must send users to System Settings.
- The Android app's protocol limits (button presses only) suggest BLE's desktop value is wake/shutter/location-sync UX, not a replacement for CrSDK/PTP-IP control.

### Gaps
- No documentation of Sony's BLE "wake over Bluetooth → bring up Wi-Fi" handshake used by Creators' App was found in this research; whether a desktop app can trigger it is unverified.
- Windows BLE pairing-dialog behaviour for unpackaged desktop apps (whether `DevicePairingKinds` prompts appear) was not covered by retrieved sources.
- btleplug's "Discover MAC address" table row is supported on 3 of 4 platforms but the docs page's column alignment did not allow confirming which platform is missing (almost certainly macOS/iOS given the CoreBluetooth caveat).

---

## Key Question 5: Programmatically joining the camera's Wi-Fi access point

### Takeaway
Both OSes technically allow an app to join a Wi-Fi network (macOS `CWInterface.associate(to:password:)`, Windows `WiFiAdapter.ConnectAsync` / `WlanConnect`), but both now gate the surrounding scan/SSID APIs behind Location permission (macOS 14.4+, Windows feature release since late 2024), and macOS Sequoia has shown SSID-redaction regressions that break `associate(withName:)` in some process types. A robust product should attempt programmatic join as a convenience and fall back to clear "join `DIRECT-xxxx` in the Wi-Fi menu" instructions.

### Cited Findings
**macOS**
- `CWInterface` provides `associate(to:password:)`, `associate(toEnterpriseNetwork:…)`, `disassociate()`, `scanForNetworks(withName:/withSSID:/includeHidden:)`, `cachedScanResults()`; Apple: use interfaces vended by `CWWiFiClient.interface()` so the app can "adopt App Sandbox… without the need for any special exceptions" (direct instantiation does low-level socket access not allowed in the sandbox) — [Apple CWInterface docs JSON](https://developer.apple.com/tutorials/data/documentation/corewlan/cwinterface.json)
- Since macOS 14.4, CoreWLAN withholds SSID names unless Location Services is enabled and the app is authorized — [Apple forums 732431](https://developer.apple.com/forums/thread/732431)
- Sequoia 15.1.1 (Apple silicon): `scanForNetworks` returned `nil` SSID/BSSID even with Location "Always Authorized", so `associate(withName:)`/`associate(withSSID:)` could not be used; same code worked on 15.0.1 and worked when wrapped in a GUI app; Apple DTS (Quinn) noted provisioning profiles are irrelevant (CoreWLAN has no restricted entitlement), suspected run-loop/main-thread differences, called the GUI-vs-CLI difference "mystifying"; root cause unresolved in-thread — [Apple forums 769950](https://developer.apple.com/forums/thread/769950)
- LaunchAgent case: location authorized but `ssid()` still nil/redacted — [Apple forums 816619](https://developer.apple.com/forums/thread/816619)
- Third-party write-up: real BSSIDs only returned to apps with Location permission signed with a stable Developer ID; launchd daemons struggle; a wrapper app bundle is used to get the prompt for CLI tools — [dev.to: reading Wi-Fi data from Go on macOS](https://dev.to/jaisonerick/reading-wi-fi-data-from-go-on-macos-after-apple-removed-airport-19g)
- Older Apple DTS guidance (10.11 era): "CoreWLAN is not supported within the sandbox"; later DTS: usable in a sandboxed app via `CWWiFiClient`-vended interfaces; scanning worked sandboxed once `com.apple.security.network.client` was enabled; App Sandbox guide says a sandboxed app cannot modify system network configuration "with the System Configuration framework, the CoreWLAN framework, or other similar APIs" — [Apple forums 11307](https://developer.apple.com/forums/thread/11307); [Apple forums 11350](https://developer.apple.com/forums/thread/11350); [Apple forums 776936](https://developer.apple.com/forums/thread/776936)
- macOS 26 Tahoe: forum reports of CoreWLAN `setWLANChannel:` misbehaving on Apple N1 Wi-Fi chip / M5 Macs (monitor-mode use case; DTS suspects Wi-Fi hardware changes); 26.4.1 fixed 802.1X join issues on M5 Macs with content-filter extensions; no Apple document listing Tahoe CoreWLAN API changes was found — [Apple forums 818603](https://developer.apple.com/forums/thread/818603); [MacRumors 26.4.1](https://www.macrumors.com/2026/04/09/apple-releases-macos-tahoe-26-4-1/)

**Windows**
- `WiFiAdapter.ConnectAsync` overloads: `(network, reconnectionKind)`, `(+PasswordCredential)`, `(+ssid)` for hidden networks, `(+WiFiConnectionMethod)` (Win10 1709+); every overload carries "Location consent is now required for Wi-Fi BSSID access" — [MS Learn WiFiAdapter.ConnectAsync](https://learn.microsoft.com/en-us/uwp/api/windows.devices.wifi.wifiadapter.connectasync?view=winrt-26100)
- `ConnectAsync`/`FromIdAsync` fail without the `wiFiControl` capability in the app manifest — [Windows IoT WiFiConnector sample](https://github.com/Microsoft/Windows-iotcore-samples/blob/master/Samples/WiFiConnector/CS/README.md)
- Microsoft's Wi-Fi/location change: without precise-location consent, `WlanScan`, `WlanGetAvailableNetworkList`, `WlanGetNetworkBssList`, `WlanQueryInterface(current_connection)` return `ERROR_ACCESS_DENIED`; `WiFiAdapter.RequestAccessAsync` returns `DeniedBySystem` and other `Windows.Devices.WiFi` APIs throw "Access is denied"; a one-time-per-app system prompt appears if the process runs in user context outside `System32`; `wiFiControl` capability itself requires location consent; apps should call `RequestAccessAsync` at a user action, check `AppCapability.CheckAccess("wiFiControl")`, and deep-link `ms-settings:privacy-location`; rollout: Insider Oct 2023, feature release fall 2024 — [MS Learn Wi-Fi access location changes](https://learn.microsoft.com/en-us/windows/win32/nativewifi/wi-fi-access-location-changes)
- To read the connected SSID without location consent use `WlanConnectionProfileDetails.GetConnectedSsid` — [same MS Learn page](https://learn.microsoft.com/en-us/windows/win32/nativewifi/wi-fi-access-location-changes)

### Inferences
- On macOS the app must (1) be a GUI app bundle signed with a stable Developer ID, (2) request `CLLocationManager` authorization with `NSLocationUsageDescription`, and (3) scan with Location granted before `associate(to:password:)` — and still ship a manual fallback because of the Sequoia regressions. The QR code gives SSID+password directly, so the scan step could be avoided if `associate` can take a `CWNetwork` obtained via `scanForNetworks(withName:)`, which itself needs Location.
- A sidecar or helper process (non-GUI) is the *worst* place to put CoreWLAN calls given the CLI/LaunchAgent redaction reports; keep Wi-Fi join in the main GUI process.
- On Windows, `WiFiAdapter` from an unpackaged Electron/Tauri process is possible (WinRT is callable from Win32 via C++/WinRT or the `windows` Rust crate — the IoT sample is UWP, so this is inferred), but the `wiFiControl` capability note implies packaged identity (MSIX / sparse package) is needed for a clean experience; the legacy `WlanConnect` (wlanapi.h) path has no capability requirement but is subject to the same location gate for scanning.
- Mac App Store (sandbox) distribution is risky for programmatic Wi-Fi join; direct Developer ID distribution (DMG) keeps options open.

### Gaps
- No authoritative Apple statement on whether `associate(to:password:)` is permitted inside App Sandbox; forum evidence conflicts.
- No report found of `CWInterface.associate` behaviour specifically on macOS 26 Tahoe; the only Tahoe CoreWLAN reports concern monitor-mode channel setting.
- No Windows 11 24H2+ desktop (non-UWP) worked example of `WiFiAdapter.ConnectAsync` was found; whether unpackaged apps can declare `wiFiControl` was not confirmed.

---

## Key Question 6: Network discovery of the camera on the LAN

### Takeaway
Sony cameras are discoverable with SSDP (M-SEARCH for `urn:schemas-sony-com:service:ScalarWebAPI:1`, parse `LOCATION` → `dd.xml` / `DigitalImagingDesc.xml`), and newer bodies expose PTP/IP on TCP 15740; the Rust `remote-camera-control-rs` library implements exactly this routing. mDNS libraries exist for every stack (bonjour-service, mdns-sd, bonsoir/nsd) but no source confirmed that Sony Alpha bodies advertise over mDNS, so SSDP + port-15740 probing is the evidence-backed approach.

### Cited Findings
- remote-camera-control-rs supports "Sony camera discovery over SSDP, `dd.xml`, and `DigitalImagingDesc.xml`" and routes ScalarWebAPI (older bodies, e.g. a7 III) vs PTP/IP (newer, e.g. a6700); `DigitalImaging` used as capability discovery "especially useful for discovering PTP support" — [GitHub remote-camera-control-rs](https://github.com/Streetblock/remote-camera-control-rs)
- Node example sends M-SEARCH to `239.255.255.250:1900` with `ST: urn:schemas-sony-com:service:ScalarWebAPI:1`; Ruby gem reads `LOCATION` where `ST` matches; some users needed `ST: ssdp:all`; camera and host must be on the same subnet — [StackOverflow via kiwix: discover Sony camera through SSDP](https://kiwix.lema.org/stackoverflow.com_en_all_2023-11/questions/39211631/how-to-discover-sony-camera-through-ssdp); [Sonycam::Scanner](https://rubydoc.info/gems/sonycam/1.3.2/Sonycam/Scanner)
- gPhoto PTP/IP docs: default port 15740, plain TCP — [gphoto PTP/IP doc](https://gphoto.sourceforge.io/doc/ptpip.php); sony-pm-alt notes the camera starts a PTP/IP server on 15740 when "Send to Computer" is enabled and UPnP packets contain connection details; uses gphoto2 `ptpip:` port — [GitHub sony-pm-alt](https://github.com/falk0069/sony-pm-alt)
- libgphoto2 (1,420 stars, LGPL-2.1, pushed 2026-11-09) "fully supports PTP" including Sony models — [GitHub libgphoto2 README](https://github.com/gphoto/libgphoto2)
- Sony's SDK guide: wired-LAN connection requires "Pairing" started by a Connect request from the application and confirmed on the camera screen — [Iconasys RemoteSampleApp IM](https://confluence.iconasys.com/download/attachments/48076985/RemoteSampleApp_IM_v1.06.00.pdf?api=v2)
- Library versions: `bonjour-service` 1.4.4 (npm, pure JS mDNS), `mdns-sd` 0.21.7 (Rust, 7.5M downloads), `simple-mdns` 0.7.0, Flutter `bonsoir` 7.1.6 / `nsd` 5.0.1 — [npm](https://registry.npmjs.org/bonjour-service); [crates.io](https://crates.io/api/v1/crates/mdns-sd); [pub.dev](https://pub.dev/api/packages/bonsoir)
- rocc's `CameraDiscoverer` "will start all the various tasks necessary for device discovery as well as keeping track of WiFi network changes and re-starting the search" — [GitHub rocc README](https://github.com/simonmitchell/rocc)

### Inferences
- When the laptop joins the camera's own AP (`DIRECT-xxxx`), the camera is almost always the gateway; reading the default gateway after association (or the DHCP server address) is a simpler, deterministic alternative to SSDP for the AP-mode case, with SSDP/port-probe reserved for infrastructure-LAN mode. (Inference; not sourced.)
- SSDP multicast requires the socket bound on the right interface; in Electron use Node `dgram` with `addMembership`; in Tauri use Rust `tokio::net::UdpSocket`; Flutter `dart:io RawDatagramSocket`; .NET `UdpClient.JoinMulticastGroup`.
- A TCP 15740 sweep over a /24 is fast and framework-agnostic but will trip corporate security tooling; limit it to the camera-AP subnet.

### Gaps
- No source confirming Sony Alpha mDNS/Bonjour advertisement (service type) was found.
- The exact SSDP response fields that reveal PTP/IP support in `DigitalImagingDesc.xml` were not retrieved (remote-camera-control-rs source would show them).
- Whether CrSDK's own wireless enumeration (`EnumCameraObjects`) uses SSDP or requires a known IP/MAC was not confirmed from retrieved pages.

---

## Key Question 7: Existing open-source desktop Sony camera control projects (fork/study candidates)

### Takeaway
There is no mature open-source *desktop GUI* app for wireless Sony Alpha control; the most useful study material is rocc (Swift, Sony Wi-Fi protocols, production-tested), remote-camera-control-rs (Rust port of rocc with PTP/IP), alphawire (C library + ImGui UI, PTP over USB/TCP), CrSDKPy (sidecar pattern around CrSDK), camera-remote-web-api (closed-source native binary, JS client), and α-Remote (Android, Sony BLE protocol).

### Cited Findings
(All metadata fetched from the GitHub API on 2026-11-10.)
- **simonmitchell/rocc** — Swift, MIT, 148 stars, pushed 2025-03-23; Sony Wi-Fi control/transfer; powers Camrote; SwiftPM/Carthage — [repo](https://github.com/simonmitchell/rocc)
- **Streetblock/remote-camera-control-rs** — Rust, Apache-2.0, 1 star, pushed 2026-04-12; SSDP discovery, ScalarWebAPI + PTP/IP (command/event channels, OpenSession, Sony SDIO handshake, `getAllDevicePropData 0x9209`), live view for ScalarWebAPI; validated on a7 III and a6700 — [repo](https://github.com/Streetblock/remote-camera-control-rs)
- **watsonmw/alphawire** — C/C++, MIT, 8 stars, pushed 2026-11-08; "Library and UI for controlling Sony Alpha cameras remotely"; backends WIA/libusbk/TCP-IP (Windows), IOKit/TCP-IP (macOS), libusb/TCP-IP (Linux); ImGui demo app with live view, focus, capture, fast download; "PTPIP backend doesn't support SSL"; Python convenience API — [repo](https://github.com/watsonmw/alphawire)
- **PICKLERICK2005/CrSDKPy** — Python + C++ host, 0.1.0b3 beta; CrSDK 2.02.00; simulator backend; hardware-validated only FX3A over USB — [PyPI](https://pypi.org/pypi/crsdkpy/json); [repo](https://github.com/PICKLERICK2005/CrSDKPy)
- **camera-remote-web-api** (npm, MIT, 1.3.7, 2026-04-28) — JS SDK + prebuilt native CrSDK binary (macOS arm64, Linux, Windows x64); no repository URL in package metadata — [npm registry](https://registry.npmjs.org/camera-remote-web-api)
- **jakkuh/sony-alpha-python** — Python, no licence, 8 stars, pushed 2026-02-09; "example code… via their modern PTP remote control API over IP"; targets FX30/A6700 with "Remote Shoot Function" enabled — [repo](https://github.com/jakkuh/sony-alpha-python)
- **falk0069/sony-pm-alt** — C, no licence, 320 stars, pushed 2021-12-01; wireless transfer via gphoto2 `ptpip:`; tested ILCE-5000 — [repo](https://github.com/falk0069/sony-pm-alt)
- **Staacks/alpharemote** — Kotlin, GPL-3.0, 150 stars, pushed 2026-11-01; Android BLE remote (button-press protocol) — [repo](https://github.com/Staacks/alpharemote)
- **gphoto/libgphoto2** — C, LGPL-2.1, 1,420 stars, pushed 2026-11-09; PTP incl. Sony, PTP/IP transport — [repo](https://github.com/gphoto/libgphoto2)
- **farhi/matlab-sonyalpha** — MATLAB, GPL; Wi-Fi/USB via curl/ffmpeg/gphoto2; live view ~every 2 s — [repo](https://github.com/farhi/matlab-sonyalpha)
- Older HTTP Camera Remote API (ScalarWebAPI) libraries: `storborg/sonypy`, `jdesfossez/sony-remote`, `alx377/sony-remote`, `timelapseplus/node-sony-camera` (fork of eqot/RemoteCamera) — [awesome.ecosyste.ms sonypy](https://awesome.ecosyste.ms/api/v1/projects/github.com%2Fstorborg%2Fsonypy); [node-sony-camera README](https://github.com/timelapseplus/node-sony-camera/blob/master/README.md)
- Commercial closed-source references: eclipseClick (C++ CrSDK bridge over JSON-RPC stdio) — [blog](https://eclipseclick-website.pages.dev/blog/sony-crsdk-macos-apple-silicon); Capture One historically lacked Sony tethering because Sony had no public SDK for it (older thread) — [Capture One community](https://support.captureone.com/hc/de/community/posts/360012174097-Tethered-for-Sony-Alpha)

### Inferences
- rocc + remote-camera-control-rs together document Sony's wireless control surface (ScalarWebAPI for pre-2020 bodies, PTP/IP + SDIO extension for newer ones) well enough to build without CrSDK, but only CrSDK is vendor-supported and covers live view on new bodies with Sony's blessing; a hybrid (CrSDK primary, PTP/IP fallback for bodies CrSDK doesn't list) is plausible but doubles protocol maintenance.
- alphawire's ImGui UI is the only open-source *desktop GUI* found; it is USB-first and single-developer, so it is a study reference rather than a fork base.
- Licence hygiene: GPL projects (alpharemote, matlab-sonyalpha) can be read but not linked into a proprietary app; MIT/Apache (rocc, alphawire, remote-camera-control-rs, zxing-wasm) can.

### Gaps
- Star counts for rocc/alphawire are low; none of these projects has a visible issue-tracker health signal in the retrieved data.
- qDslrDashboard's source/licence and current Sony support could not be verified (2016 forum evidence only).
- digiCamControl's Sony support is contradictory across sources and was not verified against its GitHub.

---

## Key Question 8: Effort/complexity estimate and recommended stack for a solo developer

### Takeaway
For a solo developer comfortable with web tech: Tauri v2 (Rust shell) or Electron (Node shell) UI + a C++ CrSDK sidecar over localhost WebSocket (control JSON + binary live-view frames) is the lowest-risk path, reusing the pattern already proven by eclipseClick and camera-remote-web-api; QR scanning is pure-webview with zxing-wasm; BLE and Wi-Fi join are optional native "convenience" features with manual fallbacks. For a native-leaning developer, SwiftUI + CrSDK (in-process or XPC helper) on macOS first, then a WinUI 3/.NET 10 port sharing the C++ bridge, gives the best OS integration (Vision, CoreWLAN, CoreBluetooth) at the cost of two UIs.

### Cited Findings
- Tauri bundle ~8-22 MB vs Electron ~180-410 MB on macOS (single-developer measurements) — [dev.to gethopp](https://dev.to/gethopp/tauri-vs-electron-performance-bundle-size-and-the-real-trade-offs-1el4)
- Tauri streaming/binary IPC primitives (`Response`, `Channel`) — [Tauri docs](https://v2.tauri.app/develop/calling-rust/); Electron needs binary IPC or experimental `sharedTexture` — [Electron docs](https://www.electronjs.org/docs/latest/api/shared-texture)
- Koffi lets Node call C ABI without compiling an addon, with main-thread-queued callbacks — [koffi.dev](https://koffi.dev/callbacks); node-addon-api TSF for compiled addons — [threadsafe.md](https://unpkg.com/koffi@2.12.3/vendor/node-addon-api/doc/threadsafe.md)
- CrSDK quirks that favour process isolation (segfault on re-Init, hardcoded CrAdapter path, ptpcamerad race) — [eclipseClick blog](https://eclipseclick-website.pages.dev/blog/sony-crsdk-macos-apple-silicon)
- Sony SDK licence permits commercial apps but SDK files cannot be redistributed by third-party wrappers (so you build/ship the SDK yourself inside your signed app) — [CrSDKPy](https://pypi.org/pypi/crsdkpy/json); [Sony licence](https://support.d-imaging.sony.co.jp/app/sdk/licenseagreement_d/en-US.html)
- macOS Wi-Fi join needs Location permission and is flaky in non-GUI processes — [Apple forums 769950](https://developer.apple.com/forums/thread/769950); Windows Wi-Fi APIs need precise-location consent — [MS Learn](https://learn.microsoft.com/en-us/windows/win32/nativewifi/wi-fi-access-location-changes)
- Windows code signing needs EV or Azure Artifact Signing to avoid SmartScreen — [Electron code signing](https://www.electronjs.org/docs/latest/tutorial/code-signing)
- Qt LGPL dynamic-linking terms — [qt.io](https://www.qt.io/licensing/open-source-lgpl-obligations)
- .NET 8/9 EOL 2026-11-10; .NET 10 LTS current — [endoflife.date](https://endoflife.date/api/dotnet.json)

### Inferences (effort, relative, solo developer)
- **Shared core (any stack): C++ CrSDK sidecar** — connect by IP/MAC, property get/set, shutter, live-view pump, content transfer, reconnect logic; plus CMake packaging of Sony dylibs/DLLs + `CrAdapter/`. This is the bulk of the work and is identical across UI choices. Expect most debugging time here (SDK threading, macOS `ptpcamerad`, re-Init crash, signing of Sony dylibs).
- **Tauri v2 (recommended for web-tech dev)**: small bundle, first-class sidecar config, `Channel<&[u8]>` for frames, Rust crates for mDNS/SSDP (`mdns-sd`, UDP), BLE (`btleplug`), QR decode in webview (`zxing-wasm`). Costs: learning Rust for the glue layer; WKWebView quirks; must verify sidecar + dylib notarization by hand (docs silent). Wi-Fi join needs a small Objective-C/Swift (`objc2`) and `windows` crate layer — or defer to manual instructions.
- **Electron (fastest start)**: Node `child_process` for the sidecar, `ws` for frames, `bonjour-service`/`dgram` discovery, Web Bluetooth built in; Koffi can even load the SDK in-process for prototyping (callbacks queued to main thread). Costs: 10-20x larger download, higher RAM, SmartScreen/EV logistics identical to Tauri.
- **Native Swift + WinUI (recommended for native dev)**: best Wi-Fi/BLE/QR integration (CoreWLAN from the GUI process, CoreBluetooth, Vision), can load CrSDK in-process or via XPC; but two UIs to build and Windows requires a second language (C#/.NET 10 or C++/WinRT). Roughly 1.6-1.8x the UI effort of a single webview UI.
- **Qt 6 / Flutter / Avalonia**: viable single-codebase native UIs; Qt is the most natural for C++ SDK in-process (no FFI) but has LGPL/commercial licence considerations and the heaviest toolchain; Flutter's `NativeCallable.listener` and Avalonia's `UnmanagedCallersOnly` make callbacks tractable, but neither has any camera-tethering precedent found, and Avalonia adds JIT entitlement/notarization steps.
- Feature priority for a Creators'-App clone, by risk: (1) CrSDK control + live view + transfer (must-have, highest effort); (2) QR scan to extract SSID/password (low effort once format is reverse-engineered); (3) LAN discovery via SSDP/port probe (low-medium); (4) programmatic Wi-Fi join (medium, OS-fragile, ship with manual fallback); (5) BLE wake/pairing (medium-high, protocol undocumented, lowest value).

### Gaps
- No published post-mortem with actual developer-hours for a CrSDK desktop app was found; the effort estimates above are relative inferences.
- Tauri 3 is in alpha (3.0.0-alpha.6); whether it changes sidecar/IPC APIs relevant here was not researched.
- Whether Sony's dylibs load correctly under hardened runtime without `allow-unsigned-executable-memory`/`disable-library-validation` entitlements must be tested; no source either way.
