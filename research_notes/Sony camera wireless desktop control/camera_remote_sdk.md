# Sony Camera Remote SDK (CrSDK) — developer-oriented research notes (state as of 2026-11-10)

Scope: Sony's official Camera Remote SDK ("CrSDK"), from the perspective of a developer building a macOS + Windows desktop app that controls Sony Alpha / Cinema Line bodies over Wi-Fi or Ethernet rather than only USB. Primary sources are Sony's SDK portal (EN + JA), the Sony license agreement, a public mirror of Sony's own API Reference (v2.01.00, Sphinx HTML, republished on GitHub by a third party), Sony help guides, and GitHub repositories/issues. Where Sony's pages were unreachable (pro.sony regional download pages return HTTP 403 to automated fetches) this is noted as a gap.

Source-quality note on the API Reference: Sony does not publish the API Reference on the open web; it ships inside the SDK zip. Two GitHub repos mirror the Sphinx HTML verbatim: [amarburg/sony_remote_camera_sdk](https://github.com/amarburg/sony_remote_camera_sdk) (titled "Camera Remote SDK API Reference 2.01.00", © 2026 Sony, pushed 2026-03-13) and [Johnnybyzhang/CrSDK-API-Reference](https://github.com/Johnnybyzhang/CrSDK-API-Reference) (v2.00.0). The 2.02.00 reference (June 2026) differs only by adding ILCE-7RM6 per Sony's release note, so the 2.01.00 mirror is used below as the best available primary text; everything marked "(API ref 2.01.00)" comes from that mirror.

---

## Key Question 1: Current version, release history, supported OS, language and binary layout

### Takeaway
CrSDK is a C++ SDK (headers + prebuilt shared libraries, no source) currently at Ver. 2.02.00 (released 2026-06-10). It runs on Windows 11 x64 (Intel/AMD only, not Windows-on-ARM), macOS 14/15/26 as a universal (arm64 + x86_64) library, and Linux x86_64/ARMv8/ARMv7 — but Wi-Fi is not supported on the Linux ARM builds.

### Cited Findings
- Current release is Ver. 2.02.00, dated 2026-Jun-10; the only listed change is "Added support for ILCE-7RM6" — [Sony SDK portal (EN)](https://support.d-imaging.sony.co.jp/app/sdk/en/index.html); confirmed on the [Japanese portal](https://support.d-imaging.sony.co.jp/app/sdk/ja/index.html).
- Ver. 2.01.00 (2026-Feb-03) added ILCE-7M5 (a7 V), PXW-Z300, PXW-Z380, DSC-RX1RM3, plus peaking/zebra/marker display during video recording (ILX-LR1) — [Sony SDK portal (EN)](https://support.d-imaging.sony.co.jp/app/sdk/en/index.html).
- Ver. 2.00.00 (2025-Jun-23) added BRC-AM7, ILME-FR7 (Ver. 3.00+), ILME-FX3A, ILME-FX2, in-camera data deletion (ILCE-7CR, 7SM3, 7M4, 7CM2, 6700 with latest firmware) and PTZ camera support — [Sony SDK portal (JA)](https://support.d-imaging.sony.co.jp/app/sdk/ja/index.html).
- Full release history (from the EN portal): 1.00.01 (2020-06) → 1.01.00 (2020-07, Linux ARMv8, movie/focus/zoom/WB) → 1.02.00 (2020-07/08, a7S III, Linux ARMv7) → 1.03.00 (2020-12, macOS support, multi-camera control) → 1.04.00 (2021-05, a1, a7R IVA, RX0 II, wired LAN, Linux x86-64) → 1.05.00 (2021-11, a7 IV, contents transfer mode, preset focus/zoom) → 1.06.00 (2022-10, FX6/FX3/FX30, GPS ImageID, save/load settings) → 1.07.00 (2022-10, a7R V) → 1.08.00 (2023-04, ZV-E1, ~20% faster repetitive shooting, remote power on/off, overheating warning) → 1.09.00 (2023-07, a6700) → 1.10.00 (2023-09, a7CR, a7C II, focus position setting, remote menu settings) → 1.11.00 (2023-11, ILX-LR1, BURANO, monitoring function) → 1.12.00 (2024-04-12, a9 III, ~100 new functions, wireless LAN) → 1.13.00 (2024-10-16, PXW-Z200, HXR-NX800, ZV-E10 II, background file transfer, recorded video preview, remote firmware update, RemoteTransferMode) → 1.14.00 (2025-01-29, a1 II) → 2.00.00 (2025-06-23) → 2.01.00 (2026-02-03) → 2.02.00 (2026-06-10) — [Sony SDK portal (EN)](https://support.d-imaging.sony.co.jp/app/sdk/en/index.html).
- Wi-Fi control was first added in Ver. 1.12 (April 2024, NAB) — [Sports Video Group coverage of Sony's 1.12 announcement](https://www.sportsvideo.org/?p=258965); Sony's portal release note for 1.12.00 lists "wireless LAN" — [Sony SDK portal (EN)](https://support.d-imaging.sony.co.jp/app/sdk/en/index.html).
- Supported OS per portal: "macOS 14.1 -", "macOS 15.1 -", "macOS 26.0 -", Windows 11 ("Intel or AMD processors only ... does not work on ARM-based computers"), Linux 64-bit (ARMv8), Linux 32-bit (ARMv7), Linux 64-bit (x86) — [Sony SDK portal (EN)](https://support.d-imaging.sony.co.jp/app/sdk/en/index.html); [JA portal](https://support.d-imaging.sony.co.jp/app/sdk/ja/index.html).
- API Reference "Supporting OS": Windows checked on Windows 11 64-bit; Linux x64 checked on Ubuntu 22.04/24.04; Linux ARMv8 checked on Jetson Orin Nano, Jetson Nano B01, Raspberry Pi 4; Linux ARMv7 on Raspberry Pi 2; macOS checked on "14 or later (Sonoma)", "15 or later (Sequoia)", "26 or later (Tahoe)"; "Provided as a universal library from version 1.08.00" (i.e., Apple Silicon native) — [API ref 2.01.00, Compatibility page](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/compatibility.html).
- Language: the SDK is "a suite of C++ classes and binary libraries to link from c++ projects" — [kchinzei/CrSDKHelper README](https://github.com/kchinzei/CrSDKHelper). Public header set shipped in the SDK: `CameraRemote_SDK.h, CrCommandData.h, CrControlCode.h, CrDefines.h, CrDeviceProperty.h, CrError.h, CrImageDataBlock.h, CrOperationCode.h, CrTypes.h, ICrCameraObjectInfo.h, IDeviceCallback.h` — [crsdk/alpha-sdk-api docs/SDK_SETUP.md](https://github.com/crsdk/alpha-sdk-api/blob/main/docs/SDK_SETUP.md).
- Binary layout (Windows): `Cr_Core.dll/.lib, monitor_protocol.dll, monitor_protocol_pf.dll` and a `CrAdapter/` folder containing `Cr_PTP_IP.dll, Cr_PTP_USB.dll, libssh2.dll, libusb-1.0.dll`; a `RemoteCli.zip` CMake sample app is inside the SDK zip — [jambyylf/onair-camera-control README](https://github.com/jambyylf/onair-camera-control). macOS equivalents: `libCr_Core.dylib, libmonitor_protocol.dylib, libmonitor_protocol_pf.dylib, CrAdapter/libCr_PTP_IP.dylib, CrAdapter/libCr_PTP_USB.dylib, CrAdapter/libssh2.dylib, CrAdapter/libusb-1.0.0.dylib` — [API ref 2.01.00, Environment Setup](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/environment_setup.html).
- The two transport plugins are named Cr_PTP_USB and Cr_PTP_IP, i.e. the network transport is PTP/IP with libssh2 for the SSH-authenticated models — [Jeffxcao/sony-camera-remote-sdk-console-mac README (Sony sample README copy)](https://github.com/Jeffxcao/sony-camera-remote-sdk-console-mac).
- SDK zip file naming observed by developers: `CrSDK_v2.00.00_20251030a_Mac.zip`, `CrSDK_v2.00.00_20251030a_Linux.zip` — [luizribeiro/sony-crsdk-rust README](https://github.com/luizribeiro/sony-crsdk-rust); `CrSDK_v2.02.00_20260610a_Mac` — [Kiriusss/Sony-Camera-Control README](https://github.com/Kiriusss/Sony-Camera-Control); `CrSDK_v2.xx.xx_Linux64PC.zip` — [rathlinus/sony-lan-shutter README](https://github.com/rathlinus/sony-lan-shutter); Windows package folder `CrSDK_Win64` — [davidg-h/scp README](https://github.com/davidg-h/scp).
- Toolchain stated in Sony's sample README (as copied by a developer): Windows needs Visual Studio 2019+, toolset v142, Windows SDK 10.0.17763.0, libusbK 3.0 driver, CMake; macOS needs Xcode 14.1/15.0 and Homebrew cmake/autoconf/automake/libtool; Linux needs libudev-dev, libxml2-dev etc. — [Jeffxcao/sony-camera-remote-sdk-console-mac README](https://github.com/Jeffxcao/sony-camera-remote-sdk-console-mac).
- Sony's companion offering, "Camera Remote Command" (CRC), is a documented proprietary PTP extension (not a library) for USB, wired LAN and Wi-Fi (PTP-IP added in CRC 2024.1.0); Sony positions CRC for multi-brand/PTP-based software and the SDK for "high-level APIs for faster development"; CRC is "available only to corporate customers, not personal users" — [Sony Camera Remote Command portal](https://support.d-imaging.sony.co.jp/app/cameraremotecommand/en/index.html).

### Inferences
- "Universal library from 1.08.00" means Apple Silicon has been natively supported since April 2023; the portal's silence on "Apple Silicon" is just wording. One developer blog reports the macOS SDK ships separate arm64 and x86_64 dylibs rather than a lipo-merged binary (see Q10), so "universal" may mean "both slices provided" rather than one fat file — verify in the actual zip.
- Windows-on-ARM is explicitly unsupported; a Windows app targeting Surface/Snapdragon devices would need x64 emulation and USB driver caveats.

### Gaps
- Sony does not publish the API Reference or release-notes PDF on the open web; the 2.02.00 reference itself could not be read (only the 2.01.00 mirror). Version-to-version API diffs for 2.02.00 beyond "ILCE-7RM6 support" are unknown.
- No official statement found on whether the macOS binaries are Developer-ID signed/notarized (see Q10).

---

## Key Question 2: Licensing, registration, download, redistribution, commercial use

### Takeaway
The SDK is free, requires accepting a click-through EULA via a regional download/contact form, allows commercial apps and inseparable embedding of the binary libraries in your app, but forbids redistributing the SDK itself (so open-source projects cannot vendor it). The licensor is Sony Electronics Inc. (US law, AAA arbitration, class-action waiver).

### Cited Findings
- Sony portal: the SDK is "free of charge"; developers "may sell apps built with the SDK for legitimate purposes"; access is by region — some regions have a download page, others a contact/registration form; materials provided are software module, API reference, sample code — [Sony SDK portal (EN)](https://support.d-imaging.sony.co.jp/app/sdk/en/index.html).
- Regional access links listed on the portal: Europe download page https://pro.sony/en_GB/digital-imaging/sdk-download ; Canada & USA https://pro.sony/ue_US/digital-imaging/camera-remote-sdk-contact-us ; Africa/India/Taiwan/Central Asia & Middle East/Australia-NZ via pro.sony "digital-imaging-contact-us" forms; Mainland China https://www.sonystyle.com.cn/minisite/di/alphauniverse/camera_remote_toolkit/index.htm ; Hong Kong/Korea via email; other APAC via a Sony Engage registration form — [Sony SDK portal (EN)](https://support.d-imaging.sony.co.jp/app/sdk/en/index.html).
- License agreement (Sony Electronics Inc.): limited, non-exclusive, non-transferable license to install/use the SDK to build "APPLICATION SOFTWARE" that remotely controls Sony cameras/peripherals; you "may incorporate the SDK's binary library into your app 'in an inseparable way' and distribute the app to third parties (END-USERS)"; you may not separately distribute, sublicense, rent or transfer the SDK; before giving end users access you must inform them that using the device through the app voids the device's Sony manufacturer warranty, must not imply Sony developed/supports the app, and must provide all support at your own cost — [Sony Camera Remote SDK License Agreement (en-US)](https://support.d-imaging.sony.co.jp/app/sdk/licenseagreement/en-US.html).
- Restrictions: no reverse engineering/decompiling; no circumventing DRM/security; prohibited "Arms"/military intelligence-surveillance-reconnaissance uses which you must pass through to end users; agreement by clicking "I AGREE", downloading, copying or using; Sony may terminate and you must destroy copies within two weeks; liability capped at the greater of amount paid or JPY 1,000,000; "AS IS" warranty disclaimer; binding AAA arbitration with 30-day opt-out, class-action waiver, venue San Diego County; Sony may amend terms by posting notice — [Sony Camera Remote SDK License Agreement (en-US)](https://support.d-imaging.sony.co.jp/app/sdk/licenseagreement/en-US.html).
- Open-source components inside the SDK (e.g. GPL/LGPL "EXCLUDED SOFTWARE") are governed by their own licenses; Sony points to oss.sony.net for component lists — [Sony Camera Remote SDK License Agreement (en-US)](https://support.d-imaging.sony.co.jp/app/sdk/licenseagreement/en-US.html). The bundled transport plugins include libssh2 and libusb-1.0 — [onair-camera-control README](https://github.com/jambyylf/onair-camera-control).
- Developer practice confirms the non-redistribution rule: "Sony's license does not permit redistributing the SDK's shared libraries" — [bgrigsby8/sony-remote](https://github.com/bgrigsby8/sony-remote); "The SDK is not included in this repository: Sony's license does not allow redistributing it ... download the Windows SDK (v2.02.00) and accept Sony's license agreement" — [onair-camera-control README](https://github.com/jambyylf/onair-camera-control); "Sony Camera Remote SDK files and binaries are not redistributed", and even Sony's sample-derived wrapper sources (`CameraDevice`, `PropertyValueTable`, ...) are treated as non-redistributable — [crsdk/alpha-sdk-api README](https://github.com/crsdk/alpha-sdk-api).
- eclipseClick (commercial app) says access "requires registering on Sony's developer portal, which the author describes as free", and that the SDK "must not be distributed, even in a private repository" (they nonetheless keep it in a private vendor repo for CI) — [eclipseClick blog, 2026-03-26](https://eclipseclick-website.pages.dev/blog/supporting-sony-cameras-eclipseclick).
- Shipping a compiled app that bundles the SDK runtime is done in practice: Kiriusss/Sony-Camera-Control's Windows/Linux release packages "already include Python, Qt and the Sony SDK runtime libraries" — [Kiriusss/Sony-Camera-Control README](https://github.com/Kiriusss/Sony-Camera-Control).
- Sony's API Reference notes an SDK "Serial Number" and `GetSDKSerial` / `EditSDKInfo` APIs exist ("Version, Serial Number, Providing Package" section) — [API ref 2.01.00 index](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/index.html).

### Inferences
- A closed-source commercial macOS/Windows app embedding libCr_Core + CrAdapter inside its .app/installer is squarely permitted; an open-source repo must instruct users to download the SDK themselves (every OSS project found does this).
- The license's warranty-void disclosure and "Arms" pass-through obligations should be surfaced in the app's EULA/first-run.

### Gaps
- The US/Canada and Europe pro.sony pages returned HTTP 403 to automated fetches, so the exact registration fields (company name? individual allowed?) and whether approval is manual could not be verified. Camera Remote Command is "corporate customers only"; whether the SDK form accepts individuals is unconfirmed (eclipseClick's "free registration" suggests it is practical).
- What the "SDK Serial Number"/`EditSDKInfo` is used for (per-licensee tracking?) is not explained in the reachable text.

---

## Key Question 3: Supported camera bodies and connection type per body (USB vs Ethernet vs Wi-Fi)

### Takeaway
30 models are supported in 2.01.00 (31 with ILCE-7RM6 in 2.02.00). Every recent Alpha/ZV/FX body supports USB and Wi-Fi via the SDK; only bodies with a LAN port (a1, a1 II, a9 III, a9 II) or a USB-C-to-Ethernet adapter path (FX3/FX3A/FX30/ILX-LR1) support wired Ethernet. Cinema/pro bodies (FX6, BURANO, Z200/Z300/NX800, FR7, BRC-AM7) are LAN/Wi-Fi only with mandatory SSH authentication. Wi-Fi is not available on Linux ARM builds.

### Cited Findings
Table reproduced from "Supporting physical layer" (API ref 2.01.00). R = RemoteControlMode, C = ContentsTransferMode (added 1.05.00), T = RemoteTransferMode (added 1.13.00). Source: [API ref 2.01.00, Compatibility](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/compatibility.html).

| # | Model | USB (R/C/T) | Wired Ethernet (R/C/T) | Wireless Wi-Fi (R/C/T) | Footnotes |
|---|---|---|---|---|---|
| 1 | ILX-LR1 | ✓✓✓ | ✓✓✓ | ✓✓✓ | *2 |
| 2 | ILCE-1M2 (a1 II) | ✓✓✓ | ✓✓✓ | ✓✓✓ | |
| 3 | ILCE-1 (a1) | ✓✓✓ | ✓✓✓ | ✓✓✓ | |
| 4 | ILCE-9M3 (a9 III) | ✓✓✓ | ✓✓✓ | ✓✓✓ | |
| 5 | ILCE-9M2 (a9 II) | ✓ - - | ✓ - - | - - - | |
| 6 | ILCE-7RM5 (a7R V) | ✓✓✓ | - - - | ✓✓✓ | |
| 7 | ILCE-7RM4A | ✓✓ - | - - - | - - - | |
| 8 | ILCE-7RM4 | ✓ - - | - - - | - - - | |
| 9 | ILCE-7CR | ✓✓✓ | - - - | ✓✓✓ | |
| 10 | ILCE-7SM3 (a7S III) | ✓✓✓ | - - - | ✓✓✓ | |
| 11 | ILCE-7M5 (a7 V) | ✓✓✓ | - - - | ✓✓✓ | *1 (SSH mandatory) |
| 12 | ILCE-7M4 (a7 IV) | ✓✓✓ | - - - | ✓✓✓ | |
| 13 | ILCE-7CM2 (a7C II) | ✓✓✓ | - - - | ✓✓✓ | |
| 14 | ILCE-7C | ✓✓ - | - - - | - - - | |
| 15 | ILCE-6700 | ✓✓✓ | - - - | ✓✓✓ | |
| 16 | MPC-2610 (BURANO) | - - - | ✓ - - | ✓ - - | *1 *3 |
| 17 | ILME-FX6V/FX6T (Ver. 3.00+) | - - - | ✓ - - | ✓ - - | *1 *2 *3 |
| 18 | ILME-FX3A | ✓✓✓ | ✓✓✓ | ✓✓✓ | *2 |
| 19 | ILME-FX3 (Ver. 2.00+) | ✓✓✓ | ✓✓✓ | ✓✓✓ | *2 |
| 20 | ILME-FX2 | ✓✓✓ | - - - | ✓✓✓ | |
| 21 | ILME-FX30 | ✓✓✓ | ✓✓✓ | ✓✓✓ | *2 |
| 22 | PXW-Z300 / PXW-Z380 | - - - | ✓ - - | ✓ - - | *1 *3 |
| 23 | PXW-Z200 | - - - | ✓ - - | ✓ - - | *1 *3 |
| 24 | HXR-NX800 | - - - | ✓ - - | ✓ - - | *1 *3 |
| 25 | BRC-AM7 | - - - | ✓ - - | - - - | *1 *3 |
| 26 | ILME-FR7 | - - - | ✓ - - | - - - | *1 *3 |
| 27 | ZV-E1 | ✓✓✓ | - - - | ✓✓✓ | |
| 28 | ZV-E10M2 (ZV-E10 II) | ✓✓ - | - - - | ✓✓ - | |
| 29 | DSC-RX1RM3 | ✓✓✓ | - - - | ✓✓✓ | |
| 30 | DSC-RX0M2 (Ver. 3.00+) | ✓✓ - | - - - | - - - | |
| 31 | ILCE-7RM6 (a7R VI) | ? | ? | ? | added in 2.02.00; not in the 2.01.00 mirror |

- Footnotes verbatim: "*1: Must be used SSH authentication. *2: Use a USB Type C wired LAN adaptor. Use of a Gigabit Ethernet compatible adaptor is recommended. *3: One camera can be controlled by multiple host devices. The maximum number of simultaneous connections is listed in Specific camera. *4 (Wireless column): Camera Remote SDK for Linux 64bit (ARMv8) and Linux 32bit (ARMv7) are not supported." — [API ref 2.01.00, Compatibility](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/compatibility.html).
- "Functions and parameters that are not supported by your camera cannot be used even if they are described in the API specification. Please update each camera to the latest System Software (Firmware) before use." — [API ref 2.01.00, Compatibility](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/compatibility.html).
- The portal model list (2.02.00) matches the table plus ILCE-7RM6: ILX-LR1, BURANO, FX6V/FX6T, FX3A, FX3, FX2, FX30, PXW-Z300/Z380/Z200, HXR-NX800, BRC-AM7, FR7, ILCE-1M2, 1, 9M3, 9M2, 7RM6, 7RM5, 7RM4A, 7RM4, 7CR, 7SM3, 7M5, 7M4, 7CM2, 7C, 6700, ZV-E1, ZV-E10M2, DSC-RX1RM3, DSC-RX0M2 — [Sony SDK portal (EN)](https://support.d-imaging.sony.co.jp/app/sdk/en/index.html).
- Not supported (developer-compiled list): original ZV-E10, a7 III, a6400, a6600, original a9, a7R III, a7S II, RX100 line — [eclipseClick blog, 2026-03-26](https://eclipseclick-website.pages.dev/blog/supporting-sony-cameras-eclipseclick).
- 2.01.00 release notes required firmware updates for ILX-LR1, ILCE-1M2, ILCE-1, ILCE-9M3, ILCE-7RM5, ILCE-7SM3, ILCE-7M4, BURANO, FX3A, FX3, FX30, BRC-AM7 — [API ref 2.01.00 release notes](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/release_notes/2_01_00.html).
- The JA portal warns that a camera firmware update can break operation with older SDK versions and recommends updating to the latest SDK — [Sony SDK portal (JA)](https://support.d-imaging.sony.co.jp/app/sdk/ja/index.html).

### Inferences
- For the requested bodies: a7 IV, a7R V, a7S III, a7C II, a6700, ZV-E1, FX2, a7CR, a7 V, RX1R III = USB + Wi-Fi (no wired); a1, a1 II, a9 III, FX3, FX3A, FX30, ILX-LR1 = USB + wired + Wi-Fi; FX6 = wired + Wi-Fi only with SSH; ZV-E10 II = USB + Wi-Fi but no RemoteTransferMode; a9 II = USB + wired, no Wi-Fi.
- a7 V (ILCE-7M5) is the first Alpha body with "*1 Must be used SSH authentication", suggesting Access Authentication is non-optional on the newest Alpha generation; a7R VI likely follows (unverified).

### Gaps
- Per-interface support for ILCE-7RM6 (2.02.00) is unknown; need the 2.02.00 API reference.
- "Maximum number of simultaneous connections ... listed in Specific camera" page for multi-host bodies was not retrieved.

---

## Key Question 4: SDK capability list

### Takeaway
CrSDK exposes (a) property get/set (hundreds of `CrDeviceProperty_*`, with per-property enable flags and candidate value lists), (b) commands/control codes (shutter, AF, movie REC, focus near/far, zoom, remote keys/touch, power on/off), (c) live view (JPEG frames + OSD image, optional HTTP delivery), (d) three content paths (post-view transfer to host, ContentsTransferMode card browsing, RemoteTransferMode browse/pull/delete/playback while controlling), (e) FTP job control and server settings, (f) settings file save/load, (g) remote firmware update, (h) PTZ, (i) monitoring stream, and (j) multi-camera connect. Discovery/enumeration and SSH-authenticated connect are covered in Q5.

### Cited Findings
- API list (2.01.00) groups: Init/Release/EnumCameraObjects/CreateCameraObjectInfo(USB|Ethernet)/GetFingerprint; Connect/Disconnect/ReleaseDevice; GetDeviceProperties/GetSelectDeviceProperties/SetDeviceProperty; SendCommand; GetLiveViewImage/GetLiveViewImageInfo/GetLiveViewProperties/GetOSDImage; GetDeviceSetting/SetDeviceSetting/SetSaveInfo; GetSDKVersion/GetSDKSerial/EditSDKInfo; Contents Transfer (GetDateFolderList, GetContentsHandleList, GetContentsDetailInfo, PullContentsFile, GetContentsThumbnailImage); Contents Transfer with remote control (GetRemoteTransferCapturedDateList, GetRemoteTransferContentsInfoList, GetRemoteTransferContentsData, GetRemoteTransferContentsDataFile) — [API ref 2.01.00 index](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/index.html).
- Additional API-reference pages present: ftp_information_ftpjob, ftp_information_ftptransfer, setting_file, control_general_setting_file, scene_file, stream_setting, monitoring, movie_playback, remote_firmware_update, control_pan_tilt, preset_ptzf_set/clear, lens_information, media_profile, eframing, pull_postview_image, upload_custom_grid_line_file, get/set/request_timezone_setting, execute_controlcode, execute_operationcode, get_command_queue_count — [amarburg mirror file tree](https://github.com/amarburg/sony_remote_camera_sdk).
- Operation-sequence chapters (2.01.00): Initialize/Release; Enumerate Cameras; Create a "Camera Object" with information known in advance; Connect/Disconnect; Changes in connection status; Connect/Disconnect multiple cameras; Get/Change Camera Properties; Send a Control Command; Get a Live View Image; Capture an Image Sequence; Change the Store Image Folder and File Name; Get the menu display string; Pull out content in ContentsTransferMode; Pull out content in RemoteTransferMode; Get the MediaProfile; SDK Properties; Download and upload setting files; Control the FTP Jobs; Control Monitoring; Memorize Zoom Distance and Focal Distance; Get OSD Image; Movie Playback; Remote Firmware Update (precheck/upload/execute/result); Control Pan/Tilt; Create/Clear PTZF Preset; TimeZone; Lens information and Focus position; Delete Contents from Media (RemoteTransfer); Set/Update Eframing; PostView image transfer to RAM; sample code incl. "connect to multi-camera", "control FTP job", "FTPServerSetting", "get Live view and OSD image with http", "pull/delete/playback content in RemoteTransferMode" — [API ref 2.01.00 index](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/index.html).
- Control codes present (sample of 2.01.00 `CrControlCode_*`): AFAreaPosition, CameraButtonFunction(Multi), CameraDialFunction, CameraLeverFunction, CameraStandBy, FocusOperation(WithInt16), NearFar, MovieRecButton(Toggle/Toggle2), PowerOff, PowerOn, PresetPTZFRecall, Release, RemoteKey{Up,Down,Left,Right,Set,MenuButton,DisplayButton,CancelBackButton}, RemoteTouchOperation, S1Button/S2Button/S1AndS2Button, SaveZoomAndFocusPosition/LoadZoomAndFocusPosition, SelectedMediaFormat, SensorCleaning, StreamButton, TrackingOnAndAFOnButton, USBConnectionModeRequest, ZoomOperation(WithInt16), FlickerScan, PixelMapping, CustomWBCapture — [amarburg mirror file tree](https://github.com/amarburg/sony_remote_camera_sdk).
- 2.01.00 new APIs: ControlGeneralSettingFile, RequestControlGeneralSettingResultFile, GetControlGeneralSettingResultFile, RequestOperation, ReleaseOperationResultObject, GetSelectControlCode, GetSupportedControlCodes, ReleaseControlCodes, ExecuteControlCodeValue, ExecuteControlCodeString, PullPostViewImage, GetCommandQueueCount; new operation code CrOperationCode_GetLicenseInfoList; "CrCommandId will not be expanded in future versions" (CrControlCode replaces it); new callbacks OnCompleteOperation, OnNotifyPostViewImage — [API ref 2.01.00 release notes](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/release_notes/2_01_00.html).
- Portal capability summary: changing camera settings, shutter release, live view monitoring, image data retrieval, video recording control and monitoring, focus/zoom/white balance control, remote firmware update (ILX-LR1), background file transfer and recorded video preview, deleting data stored on the camera, GPS/image ID linking, pan/tilt (BRC-AM7/FR7) — [Sony SDK portal (EN)](https://support.d-imaging.sony.co.jp/app/sdk/en/index.html).
- 1.12 added "about 100 functions ... remote key operations, Fn Menu operation, custom keys", memory-card slot switching, ISO AUTO limits, aimed at "inspections and surveys and e-commerce" — [Sports Video Group](https://www.sportsvideo.org/?p=258965).
- Connect `openMode` values: `CrSdkControlMode_Remote` (default), `CrSdkControlMode_ContentsTransfer` (1.05.00), `RemoteTransfer` (1.13.00); "Switching between RemoteControlMode and ContentsTransferMode cannot be performed while connected"; automatic reconnection is ON by default but "forcibly disabled" in ContentsTransferMode — [API ref 2.01.00, Connection](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/api_reference/connection.html).
- Transfer destinations: `CrDeviceProperty_StillImageStoreDestination` = HostPC / MemoryCard / HostPCAndMemoryCard must be set before release; default partial-transfer sizes: post-view not partial over USB (Win/mac), 1 MB chunks over Ethernet; ContentsTransfer 2047 MB over USB, 1 MB over Ethernet (configurable via `Setting_Key_PartialBuffer`) — [API ref 2.01.00, Tips](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/tips_trouble_shooting/tips_trouble_shooting.html).
- Property gating: some properties require `CrDeviceProperty_PriorityKeySettings = CrPriorityKey_PCRemote` before they can be set (ExposureProgramMode, FocusMode, Drive Mode); check `CrPropertyEnableFlag` and candidate `values` before `SetDeviceProperty` — [API ref 2.01.00, Tips](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/tips_trouble_shooting/tips_trouble_shooting.html).
- Multi-camera: "Multi-camera control" added 1.03.00 — [Sony SDK portal (EN)](https://support.d-imaging.sony.co.jp/app/sdk/en/index.html); a Windows OSS app controls 3 FX30s over Wi-Fi simultaneously — [onair-camera-control](https://github.com/jambyylf/onair-camera-control).
- Live view over the SDK is JPEG frames; one developer notes "Stop Live View" only toggles an SDK flag and the body keeps streaming unless `CrDeviceProperty_LiveViewProtocol` is set to None — [eclipseClick blog, 2026-04-28](https://eclipseclick-website.pages.dev/blog/sony-crsdk-macos-apple-silicon).

### Inferences
- Live view in the SDK is a polled JPEG image API (GetLiveViewImage), not an RTSP/H.264 stream; the separate "Monitoring" (ControlMonitoring, monitor_protocol libs) path exists for cinema bodies but requires extra library files.

### Gaps
- Per-model function support matrices (which `CrDeviceProperty` each body supports) live in the Device Property List pages that were not extracted; the report writer should treat "SDK supports X" as "SDK API exists; per-camera availability must be checked".

---

## Key Question 5: How network connection works — discovery vs manual IP/MAC; pairing/authentication (SSH / Access Authentication / fingerprint)

### Takeaway
Two paths: (1) `EnumCameraObjects()` auto-discovers connectable cameras on USB and the LAN/Wi-Fi segment (Sony's docs list UDP 1900 plus high UDP/TCP port ranges as "used for searching the connected cameras", and note a ~30 s delay after IP setup); (2) `CreateCameraObjectInfoEthernetConnection(model, ipv4, mac[6], sshSupport)` builds a camera object from a known IP (MAC is just an identifier, "not always necessary to specify the MAC address of the camera body"). For SSH/Access-Authentication bodies you call `GetFingerprint()`, have the user compare it with the fingerprint shown on the camera, then `Connect(..., userId, userPassword, fingerprint, size, pairingDisplayName)`. Non-SSH LAN connections instead use the camera's "Pairing" menu (or "Connect without Pairing = Enable").

### Cited Findings
- `EnumCameraObjects` "generates a list of 'connectable' cameras ... Enumerates all supported devices which are currently connected to the PC"; the `timeInSec` parameter "is not supported with the current Camera Remote SDK"; returns `CrError_Adaptor_HandlePlugin` if plugin modules (CrAdapter) are not found — [API ref 2.01.00, Camera Object](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/api_reference/camera_object.html).
- Wi-Fi Direct: "If you call the EnumCameraObjects() API on a host device that has joined the camera's SSID network, only this camera will be detected" — [API ref 2.01.00, Environment Setup](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/environment_setup.html).
- Wired LAN: "After the 'IP Address setting', it takes about 30 seconds for the SDK to recognize the camera" — [API ref 2.01.00, Environment Setup](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/environment_setup.html).
- Ports: "Camera Remote SDK uses the following ports for such as searching the connected cameras ... Remote port UDP 1900, 32768-61000; TCP 80, 8080, 22, 64321, 15740. Local port UDP 1900, 49152-65535; TCP 49152-65535"; firewall may block recognition; "security software makes Warning if your application has no digital signature" — [API ref 2.01.00, Environment Setup](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/environment_setup.html).
- Manual object: `CrError CreateCameraObjectInfoEthernetConnection(ICrCameraObjectInfo** pCameraObjectInfo, CrCameraDeviceModelList model, CrInt32u ipAddress, CrInt8u* macAddress, CrInt32u sshSupport = 0);` — ipAddress is little-endian packed ("192.168.0.5 = 0x0500A8C0"); macAddress "6byte fixed. This value is used to identify the 'Camera Object'. It is not always necessary to specify the MAC address of the camera body. If you create multiple 'Camera Object', it is recommended to specify different 6-byte data for each"; `sshSupport`: "For SSH authentication models, set CrSSHsupport_ON ... If this parameter is omitted for a camera that requires SSH authentication, connect will fail"; errors include `CrError_Api_NotSupportModelOfEthernet`, `CrError_Api_InvalidIpAddress`, `CrError_Api_InvalidMacAddress`; objects from enumeration and from this API must not be mixed — [API ref 2.01.00, Camera Object](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/api_reference/camera_object.html). Header confirms signature — [CameraRemote_SDK.h (copy in sonyalphapy repo)](https://github.com/thewhaleking/sonyalphapy/blob/main/CrSDK_v1/app/CRSDK/CameraRemote_SDK.h).
- Fingerprint: `GetFingerprint(ICrCameraObjectInfo*, char* fingerprint, CrInt32u* size)` "gets a fingerprint data from a camera that requires an SSH authentication connection. Getting and checking the fingerprint is the only way to avoid connecting to the wrong destination ... The user should compare the fingerprint acquired by this API with the fingerprint displayed on the camera body ... If they do not match, the Connect() will fail"; fingerprint is Base64 (no null terminator), changes when the camera is initialized or regenerated, and can be cached and reused across Connect() calls; errors `CrError_Connect_SSH_NotSupported`, `CrError_Connect_SSH_GetFingerprintFailed` ("probable reason is that the IP address ... is incorrect") — [API ref 2.01.00, Camera Object](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/api_reference/camera_object.html).
- Connect: `CrError Connect(ICrCameraObjectInfo*, IDeviceCallback*, CrDeviceHandle*, CrSdkControlMode openMode = Remote, CrReconnectingSet reconnect = ON, const char* userId = 0, const char* userPassword = 0, const char* fingerprint = 0, CrInt32u fingerprintSize = 0, const CrInt16u* pairingDisplayName = nullptr);` asynchronous, result via `IDeviceCallback::OnConnected`/`OnError`; "When operating a camera that requires SSH authentication, it is necessary to set a User name and Password on the camera body. In addition, it is necessary to acquire fingerprint data in advance with GetFingerprint()"; "Repeatedly entering the wrong SSH parameters will lock the camera. In that case, turn off the power switch of the camera and restart it, or wait for a while"; "never call Connect() again before receive OnConnected or OnError" — [API ref 2.01.00, Connection](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/api_reference/connection.html). (The older v1 header lacks `pairingDisplayName` — [CameraRemote_SDK.h v1 copy](https://github.com/thewhaleking/sonyalphapy/blob/main/CrSDK_v1/app/CRSDK/CameraRemote_SDK.h).)
- SSH transport ports: "SSH 22 — Used for SSH connection to the camera. HTTP 8080 → local 58081-58027(sic) — can be used by users to access content in the slot's media. Increases each time Connect() is called ... when multiple SSH-authenticated cameras are connected at the same time, users will be communicating with localhost instead of the camera's IP address. Ex.) Cam1(192.168.10.3) = localhost:58081, Cam2(192.168.10.4) = localhost:58082" — [API ref 2.01.00, Environment Setup](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/environment_setup.html).
- Camera-side wired LAN (non-SSH Alpha, ILCE-1 example): set "Network > Cnct./Remote Sht. > Remote Shoot Function > Remote Shooting = On"; IP Address setting Auto (DHCP or self-assigned link-local) or Manual; perform "Remote Shoot Function > Pairing" on the camera, then call Connect() and confirm OK on the camera; power-cycle to store pairing; or set "Connect without Pairing = Enable" (Sony disclaims liability) — [API ref 2.01.00, Environment Setup](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/environment_setup.html).
- Camera-side SSH (FX6 example): "MENU > Network > Access Authentication" — set User Name and Password; "Show Settings > Fingerprint" to read the fingerprint; Wired LAN Setting On, "Cam. Remote Ctrl = Enable"; DHCP on/off — [API ref 2.01.00, Environment Setup](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/environment_setup.html).
- Camera-side Wi-Fi (ILCE-1 example): Wi-Fi Direct via "Remote Shoot Function > Wi-Fi Direct Info" (SSID/password; fixed camera IP; cannot auto-reconnect on camera start — "If you want to connect automatically when the camera starts, please use Access Point connection"); Access Point via "Network > Wi-Fi > Access Point Set."; "When controlling remotely using Wi-Fi Access Point, the default setting for SSH authentication is enabled for secure connection. 'Network > Network Option > Access Authen. Settings > Access Authen.' ... There is no problem even if you leave the default value"; Pairing is disabled with Wi-Fi Direct or SSH, enabled only when using an AP with SSH off; recommended: keep camera within 5 m of host/AP, turn Bluetooth off, use 5 GHz; troubleshooting: firewall exceptions (Windows Defender / macOS "Allow incoming connections"), Airplane Mode off, turn off VPN software on the host — [API ref 2.01.00, Environment Setup](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/environment_setup.html).
- Sony help guide (FX3A, same flow as Imaging Edge Desktop): Wi-Fi Connect On → "Remote Shoot Function > Remote Shooting = On" → read "Access Authen. Info" (user name, password, fingerprint) → enter in the PC app; Pairing only needed when Access Authen. is Off; "If Access Authen. is set to Off, communication uses SSH without authentication or encryption"; cannot control from PC while a smartphone is connected; pairing info erased on camera initialize — [Sony Help Guide ILME-FX3A, Remote Shoot Function](https://helpguide.sony.net/ilc/2510/v1/en/contents/0903_pc_remote_function.html).
- Real-world implementation (sonshell, Linux, a6700 over Wi-Fi): defaults to `EnumCameraObjects` and takes the first camera; `--host <IPv4>` + optional `--mac` + `--model` builds an Ethernet object; `--user/--pass` for Access Auth; caches the fingerprint at `~/.cache/sonshell/fp_enumerated.bin`; prefers the fingerprint from the discovered object, falls back to cache; if Connect fails with 0x8202 it retries once with the opposite `CrSSHsupport` flag — [goudvuur/sonshell src/main.cpp](https://github.com/goudvuur/sonshell/blob/master/src/main.cpp); [README](https://github.com/goudvuur/sonshell).
- Rust wrapper usage shows the typical manual path: `sonyctl --ip 192.168.1.100 --mac 00:00:00:00:00:00` with `SONY_SSH_USER/SONY_SSH_PASSWORD`, and "Find your camera's MAC address with: arp -n <camera-ip>" — [luizribeiro/sony-crsdk-rust README](https://github.com/luizribeiro/sony-crsdk-rust).

### Inferences
- Discovery over LAN/Wi-Fi is most likely SSDP/UPnP-style (UDP 1900 is the SSDP port) carried by the Cr_PTP_IP adapter, with PTP/IP on TCP 15740 and the camera's HTTP on 80/8080; Sony does not name the protocol. "Connected to the PC" in the EnumCameraObjects text includes LAN-attached cameras per the Wi-Fi Direct note and sonshell's working Wi-Fi enumeration.
- Enumeration only finds cameras on the same L2 segment; across subnets/VLANs you must use the manual IP object. The MAC is effectively an opaque 6-byte object ID, so an app can fill it with anything unique.
- A robust desktop app should: enumerate, show fingerprint for user confirmation on first connect, cache fingerprint per camera serial, pass credentials, and handle the "retry with flipped sshSupport" ambiguity for bodies where Access Authen. can be toggled off.

### Gaps
- Sony does not document the exact discovery protocol or how to distinguish USB vs IP objects in the enumeration result beyond `connectTypeName`/`adaptorName` strings in `CreateCameraObjectInfo`.
- `pairingDisplayName` semantics (what the camera shows during pairing) not described beyond "refer to the help guide".

---

## Key Question 6: Which cameras can be controlled over Wi-Fi via the SDK (requested list)

### Takeaway
All requested current bodies except FX6 and a9 II support Wi-Fi R/C/T via the SDK; FX6 supports Wi-Fi in RemoteControlMode only with mandatory SSH; ZV-E10 II lacks RemoteTransferMode over Wi-Fi; a7 V requires SSH.

### Cited Findings
Source for every row: [API ref 2.01.00, Compatibility](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/compatibility.html).

| Camera (model code) | SDK Wi-Fi control | Notes |
|---|---|---|
| a7 IV (ILCE-7M4) | Yes — R, C, T | USB yes; no wired LAN |
| a7R V (ILCE-7RM5) | Yes — R, C, T | USB yes; no wired LAN |
| a1 (ILCE-1) | Yes — R, C, T | USB + wired LAN too |
| a1 II (ILCE-1M2) | Yes — R, C, T | USB + wired LAN too |
| a7S III (ILCE-7SM3) | Yes — R, C, T | USB yes; no wired LAN |
| a7C II (ILCE-7CM2) | Yes — R, C, T | USB yes; no wired LAN |
| FX3 (ILME-FX3, fw 2.00+) / FX3A | Yes — R, C, T | USB + wired via USB-C LAN adapter |
| FX30 (ILME-FX30) | Yes — R, C, T | USB + wired via USB-C LAN adapter |
| FX6 (ILME-FX6V/T, fw 3.00+) | Yes — R only | No USB; SSH mandatory; multi-host |
| FX2 (ILME-FX2) | Yes — R, C, T | USB yes; no wired |
| a9 III (ILCE-9M3) | Yes — R, C, T | USB + wired LAN too |
| a9 II (ILCE-9M2) | No | USB R and wired R only |
| a6700 (ILCE-6700) | Yes — R, C, T | USB yes |
| ZV-E1 | Yes — R, C, T | USB yes |
| ZV-E10 II (ZV-E10M2) | Yes — R, C (no T) | USB R, C |
| a7 V (ILCE-7M5) — exists, added SDK 2.01.00 | Yes — R, C, T | SSH authentication mandatory (*1) |
| a7CR (ILCE-7CR) | Yes — R, C, T | |
| RX1R III (DSC-RX1RM3) | Yes — R, C, T | |
| a7R VI (ILCE-7RM6) — added SDK 2.02.00 | Unknown | Not in 2.01.00 table |
| ILX-LR1 | Yes — R, C, T | |
| BURANO, PXW-Z200/Z300/Z380, HXR-NX800 | Yes — R only | SSH mandatory; no USB |
| BRC-AM7, FR7 | No (wired only) | |
| a7R IV, a7R IVA, a7C, RX0 II | No | USB only |

- Wi-Fi is unsupported on the Linux ARMv8/ARMv7 SDK builds (*4) — [API ref 2.01.00, Compatibility](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/compatibility.html).
- Sony's own Wi-Fi caveat: "the data transfer band is narrower than that of wired LAN, and it is more susceptible to radio wave interference ... intended for indoor use such as video content production and tethered shooting, and cannot be guaranteed for all use cases ... Because the Wi-Fi chips and antenna performance installed in cameras are different, there will be differences in throughput and connection stability" — [API ref 2.01.00, Environment Setup](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/environment_setup.html).
- Field confirmations of Wi-Fi control via SDK: a6700 (sonshell, Linux) — [goudvuur/sonshell](https://github.com/goudvuur/sonshell); FX30 ×3 simultaneously (Windows) — [onair-camera-control](https://github.com/jambyylf/onair-camera-control); a6700 via Home Assistant add-on using user/password/fingerprint — [rathlinus/sony-lan-shutter](https://github.com/rathlinus/sony-lan-shutter); macOS Wi-Fi console with SDK 2.00.00 — [Jeffxcao/sony-camera-remote-sdk-console-mac](https://github.com/Jeffxcao/sony-camera-remote-sdk-console-mac).

### Inferences
- A macOS/Windows wireless app can target essentially the entire 2021+ Alpha/ZV/FX lineup; the practical exclusions are pre-2020 bodies (a7 III, a6400/6600, a7R III/IV, original a9/ZV-E10), which need PTP/IP reverse-engineering or libgphoto2 instead.

### Gaps
- No throughput/latency measurements for SDK live view over Wi-Fi were found in any source.

---

## Key Question 7: Using the SDK from other languages — community bindings/wrappers

### Takeaway
There is no mature, maintained binding in any language. Everything is a small hobby/early project (0–5 stars), and none can ship the SDK binaries; the most complete are a Rust wrapper (bindgen + async API + TUI, network/SSH aware, last push Dec 2025), two Python wrappers (CrSDKPy on PyPI, Windows/USB validated, Aug 2026; davidg-h/scp nanobind, Nov 2026), and a C# N.I.N.A. astrophotography plugin (Windows, a7 V, Oct 2026). No Node.js N-API addon exists; "sony-crsdk-rs", "pysony" (that is the old HTTP API) and "crsdk pip" as named in the brief do not exist as described.

### Cited Findings
Python
- [PICKLERICK2005/CrSDKPy](https://github.com/PICKLERICK2005/CrSDKPy) / PyPI [crsdkpy](https://pypi.org/project/crsdkpy/): "A python wrapper for the SONY Camera Remote SDK"; architecture SDK/Camera/Session objects with `simulator`, `host` (separate helper process because "the vendor SDK resolves its adapter directory relative to the host executable") and `native` backends; targets CRSDK 2.02.00; "The bridge is Windows-tested only"; hardware validated only on ILME-FX3A fw 2.02 over USB; beta; MIT; pure-Python wheel, user must build `crsdkpy_host` against their own SDK; 5 stars, pushed 2026-08-27 (GitHub API).
- [thewhaleking/sonyalphapy](https://github.com/thewhaleking/sonyalphapy) / PyPI [sonyalphapy](https://pypi.org/project/sonyalphapy/): "Python bindings for the Sony Camera Remote SDK v1", self-described fragile, tested only on macOS with an a7 IV; MIT; 5 stars; pushed 2026-03-03; note the repo commits Sony's `CrSDK_v1` headers and sample into the repo (license risk).
- [davidg-h/scp](https://github.com/davidg-h/scp) ("sonycam"): nanobind-based wrapper, Windows 10/11 x64 + VS2022 + Python 3.12, a7R V over USB; discovery/connect/capture/download; MIT; 1 star; pushed 2026-11-03.
- [crsdk/alpha-sdk-python](https://github.com/crsdk/alpha-sdk-python): Python client for the alpha-sdk-api REST server (not a direct binding); 1 star; pushed 2026-08-11.
- [Tomonobu3110/CrSDK_MCP](https://github.com/Tomonobu3110/CrSDK_MCP): Python MCP server around CrSDK; 0 stars; pushed 2026-06-09; no license.
- Not CrSDK: PyPI `pysony`, `libsonyapi`, `sony-camera-server`, `alpha-sdk-client`, and GitHub `karhohs/scrappy` wrap Sony's discontinued HTTP/JSON "Camera Remote API" — [PyPI pysony](https://pypi.org/project/pysony/0.1.7/); [PyPI libsonyapi](https://pypi.org/project/libsonyapi/).

Rust
- [luizribeiro/sony-crsdk-rust](https://github.com/luizribeiro/sony-crsdk-rust): "Safe, idiomatic Rust bindings ... via network or USB"; `crsdk-sys` (bindgen FFI) + `crsdk` safe wrapper (async + blocking) + `sonyctl` CLI/TUI with camera discovery or `--ip/--mac`, `--ssh --user --password`; macOS 12.1+ and Ubuntu 20.04; built against `CrSDK_v2.00.00_20251030a`; setup script "fixes their install names for proper loading" and recreates adapter symlinks on each build; 1 star; no license file; pushed 2025-12-11 (GitHub API). No crate named "sony-crsdk-rs" was found.
- [Streetblock/remote-camera-control-rs](https://gh.r-mj.com/Streetblock/remote-camera-control-rs) is a Rust port of the Swift "rocc" project using the old ScalarWebAPI/PTP, not CrSDK (search result).

C#
- [isky-zh-l/Sony-CrSDK-Camera-Plugin-for-NINA](https://github.com/isky-zh-l/Sony-CrSDK-Camera-Plugin-for-NINA): C# plugin for N.I.N.A. 3.2 (astrophotography): exposure, software bulb, ISO/shutter, JPEG live view, raw ARW receive + Bayer decode; "verified with Sony A7 V (ILCE-7M5)"; requires Windows x64 and Sony SDK 2.02.00 Win x64 imported by the user; three-layer native/managed/NINA architecture; v0.5.6; MIT; pushed 2026-10-26; no multi-camera.
- [kmgallahan/AllSystemsGo.SonyCameraRemoteSDK_SGT](https://github.com/kmgallahan/AllSystemsGo.SonyCameraRemoteSDK_SGT): 2021 attempt at a C# wrapper via SGT; 0 stars. A 2021 CppSharp issue asked for help wrapping it — [mono/CppSharp#1617](https://github.com/mono/CppSharp/issues/1617).
- Older C# `kazyx/KzRemoteApi` targets the legacy Camera Remote API, not CrSDK.

Node.js / TypeScript
- No N-API/node-addon-api binding found. [crsdk/alpha-sdk-typescript](https://github.com/crsdk/alpha-sdk-typescript) is a generated REST client (0 stars). [ajhochy/fps-camcontrol](https://github.com/ajhochy/fps-camcontrol) is a TypeScript/Electron project with an open issue "electron: Sony locator + guided install (no Sony shipped)" (0 stars, pushed 2026-11-05).
- `rathlinus/sony-lan-shutter` is Python/FastAPI + a C++ `sony_shoot` helper, not a Node addon — [rathlinus/sony-lan-shutter](https://github.com/rathlinus/sony-lan-shutter).

Go
- [dup2X/crsdk](https://github.com/dup2X/crsdk): Go, MIT, 0 stars, last push 2021-05-12 (GitHub API). Content not inspected.

Swift / macOS
- [Kiriusss/Sony-Camera-Control](https://github.com/Kiriusss/Sony-Camera-Control): macOS version is SwiftUI (Windows/Linux Qt/C++), SDK 2.02; Apple-silicon zip v1.2.0; "uses local ad-hoc signing"; ILX-LR1 validated; 1 star; pushed 2026-09-22.
- `crsdk/alpha-sdk-swift` (REST client) returned 404 (private/removed).

C++ helpers / other hosts
- [kchinzei/CrSDKHelper](https://github.com/kchinzei/CrSDKHelper): CMake `FindCrSDK.cmake` modules; MIT; macOS Monterey + Raspberry Pi tested; warns older wrapper projects/Ruby gem "do not work with CrSDK, because new API is completely incompatible"; pushed 2025-02-13; 2 stars.
- ROS: [shawnq8861/sony_camera_node](https://github.com/shawnq8861/sony_camera_node) (ROS Noetic, 12 stars, pushed 2021), [nimahmoodi1/sony-rx0m2-ros2](https://github.com/nimahmoodi1/sony-rx0m2-ros2) (ROS 2 Humble, RX0 II, x86_64/AArch64, pushed 2026-09-15).
- TouchDesigner: [ejurquet/CrSDK-4-TD](https://github.com/ejurquet/CrSDK-4-TD) (2024, 0 stars).
- Viam robotics module: [bgrigsby8/sony-remote](https://github.com/bgrigsby8/sony-remote) (USB); claims "Sony does not publish a Darwin build of CrSDK" — contradicted by Sony's macOS universal library (Q1), so treat as that author's error.
- REST/MCP bridge: [crsdk/alpha-sdk-api](https://github.com/crsdk/alpha-sdk-api) — C++ REST/SSE/WebSocket server + MCP server on top of CrSDK, OpenAPI spec, Fern-generated TS/Python clients; MIT; 3 stars; pushed 2026-08-13; ~15 open issues (see Q10).

### Inferences
- For a macOS + Windows desktop app, the realistic choices are: write the camera layer in C++ (or Objective-C++/Swift bridging on macOS and C++/CLI or P/Invoke on Windows) against the headers directly, or run a small C++ bridge process (the pattern used by eclipseClick, CrSDKPy and alpha-sdk-api) and talk to it over IPC/HTTP. Several projects independently chose the bridge-process pattern because of the SDK's working-directory/`CrAdapter` lookup behavior and Init/Release crash (Q10).
- None of the bindings can be "pip/npm/cargo installed and run"; each requires the user to supply the Sony SDK, which kills frictionless distribution for OSS but is irrelevant for a signed commercial binary.

### Gaps
- No download/usage statistics exist for any binding; maturity is inferred from stars, commit dates and stated test coverage.
- Go wrapper internals, and whether `dup2X/crsdk` even compiles against v2, were not checked.

---

## Key Question 8: Open-source apps built on the SDK

### Takeaway
A handful of small open-source apps exist (the largest is sonshell, 257 stars, Linux-only); several 2026 projects target Windows and macOS with Wi-Fi and multi-camera, but all are single-developer efforts validated on one body each. The project names suggested in the brief ("OpenSonyRemote", "crsdk-remote", "camera-control-sony", "Monitor+") were not found; Sony's own "Monitor & Control" is a closed iPad app.

### Cited Findings
- [goudvuur/sonshell](https://github.com/goudvuur/sonshell) — Linux CLI/REPL "ssh into my Sony camera" (name only; no SSH shell); Wi-Fi/Ethernet via CrSDK 2.00.00; auto-download of new captures, exposure/shutter/focus/REC, live view via bundled OpenCV, gamepad/pedal mapping, keepalive reconnection; Apache-2.0; tested only on a6700 fw 2.00; 257 stars, 70 commits, last push 2026-04-23; open issue #2 (a7C II over USB fails with `CrError_Connect_RemoteTransfer_NotSupported 0x8219` on SDK 2.01.00; maintainer: "I've created Sonshell to work with wifi for now") — [issue #2](https://github.com/goudvuur/sonshell/issues/2).
- [jambyylf/onair-camera-control](https://github.com/jambyylf/onair-camera-control) — Windows 10/11 C++ engine + WebView2 UI; auto-connects up to 3 cameras over Wi-Fi, live view (HD/Smooth), per-camera and MASTER REC, ISO/Iris/Shutter/WB/Picture Profile with "MATCH" across cameras, focus AF/MF near/far, audio level, battery/card status, fullscreen monitor with grid/histogram/zebra/peaking, phone/tablet web panel and OBS browser-source page; SDK 2.02.00 required (user-supplied); tested on FX30 only; license "NOASSERTION"; 0 stars; pushed 2026-09-11.
- [Kiriusss/Sony-Camera-Control](https://github.com/Kiriusss/Sony-Camera-Control) — Windows/Linux (Qt + C++ + Python) and macOS (SwiftUI) desktop app: search/connect over USB "and the network connection methods the camera provides, network authentication", live view, exposure params, direct-to-PC capture, burst, manual focus stepping, live-view recording to MJPEG/H.264; SDK 2.02; ILX-LR1 validated; Windows/Linux v1.3.0, macOS arm64 v1.2.0 releases bundle the SDK runtime; Windows requires installing Sony's `srcameradriver.inf` via `pnputil` (replaces MTP driver); 1 star; pushed 2026-09-22.
- [danielhelloworld/Sony-Camera-Control](https://github.com/danielhelloworld/Sony-Camera-Control) — "Lightweight Sony Camera Remote SDK control, live view, capture, storage, and multi-camera system for Windows"; C++; 2 stars; pushed 2026-08-12.
- [RadGoose/sony-remote](https://github.com/RadGoose/sony-remote) — Linux GUI: live view, autofocus, tracking, shutter; 0 stars; pushed 2026-04-08.
- [crsdk/alpha-sdk-api](https://github.com/crsdk/alpha-sdk-api) — "Alpha Camera REST API": local REST/SSE/WebSocket + MCP server built on CrSDK, built from source only; MIT; 3 stars; pushed 2026-08-13; hosted docs at crsdk.github.io/alpha-sdk-api.
- [rathlinus/sony-lan-shutter](https://github.com/rathlinus/sony-lan-shutter) — Home Assistant OS add-on (Docker, Linux x86_64 SDK) that shoots and downloads JPEG/RAW from an a6700 over Wi-Fi/LAN with user/password/fingerprint config; AGPL-3.0; single commit.
- [Jeffxcao/sony-camera-remote-sdk-console-mac](https://github.com/Jeffxcao/sony-camera-remote-sdk-console-mac) — "SDK v2.00.00 - Mac console application with Wi-Fi control"; 0 stars; pushed 2025-10-23.
- [whoosh75/Sony-PI-Camera-Control](https://github.com/whoosh75/Sony-PI-Camera-Control) — BURANO (MPC-2610) control from Raspberry Pi; 1 star; pushed 2026-05-26.
- [sohtamei/CrSDK.IP](https://github.com/sohtamei/CrSDK.IP), [CrSDK.ptz](https://github.com/sohtamei/CrSDK.ptz), [CrSDK.cli](https://github.com/sohtamei/CrSDK.cli), [CrSDK.EnableUSBC_Remote](https://github.com/sohtamei/CrSDK.EnableUSBC_Remote) — small samples (IP connection, PTZ), 2025–2026.
- [Iktwo/Brqtt](https://github.com/Iktwo/Brqtt) — 2020 desktop focus-bracketing app (GPL-3.0, 5 stars, abandoned 2020).
- [IRSL-IMS-ZHAW/SonySDK1v11_Examples](https://github.com/IRSL-IMS-ZHAW/SonySDK1v11_Examples) — academic examples for SDK 1.11 (2024).
- Closed-source but documented: eclipseClick (commercial eclipse-photography app) uses a `SonyBridge`/`sony-camera-bridge` process built on CrSDK 2.02.00 for Windows and macOS, falling back to PTP bridge/libgphoto2 for unsupported bodies — [eclipseClick blog 2026-03-26](https://eclipseclick-website.pages.dev/blog/supporting-sony-cameras-eclipseclick).
- Microsoft Store listing "Camera Remote Toolbox for Sony" (free) exists but its publisher and whether it uses CrSDK could not be read — [Microsoft Store listing](https://apps.microsoft.com/detail/9pgcqhwrktdp).
- Bitfocus Companion has an open module request for Sony ILCE/ILME cameras via the SDK (2025-09) with no implementation — [bitfocus/companion-module-requests#1936](https://github.com/bitfocus/companion-module-requests/issues/1936).

### Inferences
- No open-source project yet offers a polished cross-platform (macOS + Windows) wireless Sony controller; onair-camera-control (Windows, Wi-Fi, multi-cam) and Kiriusss/Sony-Camera-Control (mac + Win) are the closest reference implementations for UI and build/packaging patterns.

### Gaps
- Projects named "OpenSonyRemote", "crsdk-remote", "sony-camera-remote" (as a CrSDK app) and "Monitor+" were not found on GitHub or the web; they may not exist.

---

## Key Question 9: What the SDK does NOT do that Sony's Creators' App does

### Takeaway
The SDK assumes the camera is already on the network (or on Wi-Fi Direct) and in Remote Shooting mode; it has no Bluetooth, no Wi-Fi provisioning/QR pairing, no cloud, and no "wake via Bluetooth" path. Creators' App additionally handles Bluetooth first-time pairing, joining the camera to an access point, Creators' Cloud / Ci Media Cloud upload, settings save/apply, system-software update, upgrade-license install and livestreaming. The SDK does cover remote firmware update, settings-file save/load and FTP job control (which Creators' App does not expose to developers).

### Cited Findings
- Creators' App help-guide topics: installing the app; first-time camera connection including Bluetooth pairing; connecting via USB cable; connecting via an access point (Wi-Fi); image import; remote shooting; uploading images to the cloud; linking the camera with the cloud; uploading camera images directly to the cloud; Ci Media Cloud transfer; external cloud transfer; saving and applying camera settings; updating camera system software; installing camera upgrade licenses; livestreaming — [Creators' App Help Guide](https://helpguide.sony.net/di-app/ca/v1/en/index.html).
- a7 V help guide lists smartphone functions: Monitor & Control, pairing (Smartphone Connection), remote shooting with Remote Shoot Setting, Select on Cam & Send, "Cnct. while Power OFF", reading location information from a smartphone — [Sony Help Guide ILCE-7M5, Creators' App](https://helpguide.sony.net/ilc/2540/v1/en/contents/221h_creators_app.html).
- Creators' App pairing "starts over Bluetooth" and then hands off to Wi-Fi; a smartphone connection blocks computer Remote Shooting — [Camera Times a7 V remote guide, 2026-09-28](https://cameratimes.org/sony-a7-v-remote-control-guide-bluetooth-wi-fi-usb-and-tethered-options/); "You cannot control the camera from a computer while it is connected to a smartphone" — [Sony Help Guide FX3A](https://helpguide.sony.net/ilc/2510/v1/en/contents/0903_pc_remote_function.html).
- SDK network prerequisites are all camera-menu driven: Wi-Fi join via "Network > Wi-Fi > Access Point Set." or Wi-Fi Direct Info; Remote Shooting On; Access Authentication/pairing — and Sony recommends turning the camera's Bluetooth off for SDK Wi-Fi use — [API ref 2.01.00, Environment Setup](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/environment_setup.html).
- SDK does provide: remote firmware update sequence (precheck/upload/execute/result), "Download and upload setting files", FTP job control and FTPServerSetting sample, remote power on/off (`CrControlCode_PowerOn/PowerOff`), stream settings/`CrControlCode_StreamButton` pages — [API ref 2.01.00 index](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/index.html); remote power on/off added 1.08.00 — [Sony SDK portal (EN)](https://support.d-imaging.sony.co.jp/app/sdk/en/index.html); power-off "works best with 'Remote Power OFF/ON' and 'Network Standby' enabled on the camera" — [sonshell README](https://github.com/goudvuur/sonshell).
- Wi-Fi Direct connections "cannot be established automatically when the camera is started"; only Access Point mode auto-reconnects — [API ref 2.01.00, Environment Setup](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/environment_setup.html).

### Inferences
- A desktop app cannot replicate Creators' App's zero-touch onboarding: the user must put the camera on the Wi-Fi network via the camera menu (or join the laptop to the camera's Wi-Fi Direct SSID). An in-app "setup wizard" with the exact menu paths and a fingerprint-confirmation screen is the best achievable UX.
- Bluetooth-based remote power-on (Creators' App "Cnct. while Power OFF") has no SDK equivalent; the SDK's `PowerOn` control code depends on the camera's network-standby feature over LAN/Wi-Fi.

### Gaps
- Whether the SDK's remote firmware update is limited to ILX-LR1 (the portal mentions only ILX-LR1) or extends to Alpha bodies was not confirmed.
- No source lists which Creators' App features (e.g., livestream setup) are explicitly unavailable to third parties; the comparison above is by omission.

---

## Key Question 10: Known limitations and developer complaints (Wi-Fi reliability, macOS/Windows quirks, firmware, driver, packaging)

### Takeaway
Documented pain points cluster around: (1) Sony's own Wi-Fi caveats and firewall/VPN/port requirements; (2) macOS packaging — hard-coded `Contents/Frameworks/CrAdapter` plugin lookup, separate-arch dylibs, quarantine attributes, `ptpcamerad` racing the SDK over USB, SIGSEGV on `Init()` after `Release()` in one process; (3) Windows needing the libusbK-based Sony driver that replaces the MTP driver; (4) firmware churn (Sony requires latest firmware and warns new firmware can break old SDKs); (5) SDK hangs (`ReleaseDevice` after a stuck transfer) and opaque error codes; (6) general "incredibly hard to work with and each camera is different".

### Cited Findings
Wi-Fi / network
- Sony's Wi-Fi caution (throughput, interference, chip/antenna variance, "indoor use ... cannot be guaranteed") and recommendations (≤5 m, Bluetooth off, 5 GHz, firewall exceptions, Airplane Mode off, disable VPN) — [API ref 2.01.00, Environment Setup](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/environment_setup.html).
- Wrong SSH credentials repeatedly entered "will lock the camera" (power-cycle or wait) — [API ref 2.01.00, Connection](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/api_reference/connection.html).
- Unsigned apps trigger security-software warnings because of the port usage — [API ref 2.01.00, Environment Setup](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/environment_setup.html).
- Live view stability depends on link traffic, Get/Set frequency, host CPU and "Some functions ... such as Wi-Fi function" on single-board computers; Sony advises minimizing live view size and transfer frequency — [API ref 2.01.00, Tips](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/tips_trouble_shooting/tips_trouble_shooting.html).
- Older DPReview/Adobe commentary: a Lightroom community commenter attributed Adobe's choice of Sony's PTP extension over the SDK to the SDK's lack of Wi-Fi/wired LAN tethering at the time and noted it is not available for Windows on ARM (user inference, not Adobe statement) — [Adobe Community tethering thread](https://community.adobe.com/t5/lightroom-classic-discussions/p-tethering-support-requests-and-information/m-p/14723168).

macOS
- SDK ships "separate native arm64 + x86_64 dylibs (separate, not lipo-merged)" with bundled libusb; `libCr_Core` "hardcodes its plugin lookup at Contents/Frameworks/CrAdapter, an .app-bundle path, with no fallback to the binary's directory" → `0x8703 CrError_Adaptor_Create` unless CrAdapter is copied to both locations (Sony's RemoteCli does a dual copy); `ptpcamerad` opens a PTP session and races the SDK → `0x8208 CrError_Connect_TimeOut` (workaround: kill loop, since `launchctl bootout/disable` is ineffective under SIP); `SCRSDK::Init()` after `Release()` in the same process crashes (SIGSEGV) → bridge process exits and is respawned; stale camera-side PTP session → `0x8210 CrError_Connect_SessionAlreadyOpened` (workaround: IOKit `USBDeviceReEnumerate`); cameras enumerate as `IOUSBHostDevice`; author calls the macOS port "half-ported" — [eclipseClick blog, 2026-04-28, a7 IV on macOS 15](https://eclipseclick-website.pages.dev/blog/sony-crsdk-macos-apple-silicon).
- macOS SDK "ships as frameworks, built per architecture and combined with lipo"; a non-recursive copy step omitted `CrAdapter/` from every macOS tarball so "CrSDK initialized but found zero devices"; developers' absolute paths baked into rpath crashed on other machines — [eclipseClick blog, 2026-03-26](https://eclipseclick-website.pages.dev/blog/supporting-sony-cameras-eclipseclick).
- Sony requires removing quarantine attributes from all seven dylibs (`xattr -d com.apple.quarantine`) after download — [API ref 2.01.00, Environment Setup](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/environment_setup.html).
- alpha-sdk-api fixed "macOS MCPB bundling: put CrAdapter where libCr_Core actually looks" — [crsdk/alpha-sdk-api#37](https://github.com/crsdk/alpha-sdk-api/issues/37); Rust wrapper's setup script must fix dylib install names and create adapter symlinks — [luizribeiro/sony-crsdk-rust README](https://github.com/luizribeiro/sony-crsdk-rust).
- Kiriusss's macOS build "uses local ad-hoc signing" (no Developer ID/notarization) — [Kiriusss/Sony-Camera-Control README](https://github.com/Kiriusss/Sony-Camera-Control).

Windows
- USB on Windows requires installing Sony's device driver (RemoteSampleApp manual, libusbK 3.0.7.0 / 3.1.0.0 verified) — [API ref 2.01.00, Environment Setup](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/environment_setup.html); [RemoteSampleApp IM v1.06.00 PDF (via search snippet)](https://confluence.iconasys.com/download/attachments/48076985/RemoteSampleApp_IM_v1.06.00.pdf?api=v2).
- The Sony driver (`srcameradriver.inf`, device shows as "Sony Remote Control Camera" under "libusbK Usb Devices") "replaces the camera's MTP driver"; to restore normal file transfer the user must reselect the Microsoft MTP driver in Device Manager — [Kiriusss/Sony-Camera-Control README](https://github.com/Kiriusss/Sony-Camera-Control). Third-party vendors report Imaging Edge's libusbK driver preventing other apps from seeing the camera — [Dragonframe FAQ](https://www.dragonframe.com/ufaqs/my-sony-appears-in-sony-imaging-edge-but-not-dragonframe/).
- Windows-on-ARM unsupported — [Sony SDK portal (EN)](https://support.d-imaging.sony.co.jp/app/sdk/en/index.html).
- MSVC strict mode error C7744 on hex escapes in Sony sample strings — [eclipseClick blog, 2026-03-26](https://eclipseclick-website.pages.dev/blog/supporting-sony-cameras-eclipseclick).
- (Wi-Fi/LAN avoids the driver problem entirely — the driver is only for the USB transport; inference.)

Linux (where it matters)
- `usbfs_memory_mb` must be raised to 150 MB (Sony's "USB Bulk Transfer Rate should be changed to 150") — [API ref 2.01.00, Environment Setup](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/environment_setup.html); alpha-sdk-api issue notes the 16 MB default "is far below Sony's required 150MB and is undocumented" — [crsdk/alpha-sdk-api#45](https://github.com/crsdk/alpha-sdk-api/issues/45).
- No Wi-Fi on Linux ARM builds — [API ref 2.01.00, Compatibility](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/compatibility.html).

SDK behavior / robustness
- "SDK ReleaseDevice() hangs forever after a stuck transfer — server cannot exit, camera needs a physical replug" — [crsdk/alpha-sdk-api#41](https://github.com/crsdk/alpha-sdk-api/issues/41); "RemoteTransfer full-size download silently never completes and latches the whole subsystem into 0x8D03" — [#40](https://github.com/crsdk/alpha-sdk-api/issues/40); "ILCE-7RM4: remote auto-transfer hangs (red LED stuck, 0 bytes, no downloadComplete)" — [#57](https://github.com/crsdk/alpha-sdk-api/issues/57); `0x8208 CrError_Connect_TimeOut` misattributed to camera settings — [#39](https://github.com/crsdk/alpha-sdk-api/issues/39).
- Device-busy `0x8746` during rapid bracketing required a 5-attempt retry loop; `0x8402 CrError_Api_InvalidCalled` from `SetDeviceProperty` after long exposures can leave the previous shutter speed in effect — [eclipseClick blogs](https://eclipseclick-website.pages.dev/blog/sony-crsdk-macos-apple-silicon).
- a7C II over USB with SDK 2.01.00 failed with `0x8219 CrError_Connect_RemoteTransfer_NotSupported` in sonshell (unresolved, May 2026) — [goudvuur/sonshell#2](https://github.com/goudvuur/sonshell/issues/2).
- Camera-state gating: shutter release may fail unless M mode + MF; focus Near/Far not accepted in AF-S; must switch PriorityKeySettings to PCRemote; memory-card-full blocks release — [API ref 2.01.00, Tips](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/tips_trouble_shooting/tips_trouble_shooting.html).
- Community sentiment: "the SDK is incredibly hard to work with and each camera is different" — [bitfocus/companion-module-requests#1936 comment, 2026-04-15](https://github.com/bitfocus/companion-module-requests/issues/1936); older wrapper projects "do not work with CrSDK, because new API is completely incompatible" — [kchinzei/CrSDKHelper](https://github.com/kchinzei/CrSDKHelper).
- Firmware: "Please update each camera to the latest System Software (Firmware) before use" — [API ref 2.01.00, Compatibility](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/compatibility.html); camera firmware updates may break older SDK versions — [Sony SDK portal (JA)](https://support.d-imaging.sony.co.jp/app/sdk/ja/index.html); "the SDK rejects older firmware" — [rathlinus/sony-lan-shutter README](https://github.com/rathlinus/sony-lan-shutter). Illustrative of firmware churn: a7 IV firmware 3.01 broke Lightroom Classic 13.3 USB tethering until a driver workaround — [Adobe Community bug thread, May 2024](https://community.adobe.com/t5/lightroom-classic-bugs/p-cannot-tether-sony-a7-iv-ilce-7m4-with-latest-3-01-firmware/idi-p/14630158).
- "PC Remote" mode: USB requires selecting "Remote Shoot (PC Remote)" on the camera's USB Connect Mode prompt (or default it via Setup > USB > USB Connection Mode); network requires "Remote Shoot Function > Remote Shooting = On" — [API ref 2.01.00, Environment Setup](https://github.com/amarburg/sony_remote_camera_sdk/blob/main/other/environment_setup.html).

### Inferences
- For a wireless-first macOS/Windows app, the Windows driver problem disappears (no USB), but the firewall/port and VPN constraints become the main support burden; the app should request firewall exceptions at install and detect VPNs.
- Code-signing/notarization: nothing suggests Sony's dylibs are notarized; a shipping macOS app must re-sign all seven dylibs with its own Developer ID inside the .app (Hardened Runtime + notarytool) and mirror the `Contents/Frameworks/CrAdapter` layout the core library expects. This is standard practice but unconfirmed against Sony's binaries.
- Architect the SDK into a separate helper process (as eclipseClick, CrSDKPy and alpha-sdk-api did) so `Init/Release` crashes and `ReleaseDevice` hangs can be recovered by restarting the helper.

### Gaps
- No Sony statement on code signing/notarization of the macOS libraries; no first-hand report of a notarized App Store or Developer-ID app shipping CrSDK was found (eclipseClick ships one but did not describe notarization).
- No quantitative Wi-Fi reliability data (drop rates, live-view FPS) from developers; Reddit/DPReview threads specific to CrSDK Wi-Fi were not found by search.
- The Sony Alpha Forum "New Camera Remote SDK exchange experiences" thread exists but returned HTTP 403 — [thread URL](https://www.sonyalphaforum.com/topic/13369-new-camera-remote-sdk-exchange-experiences/).
