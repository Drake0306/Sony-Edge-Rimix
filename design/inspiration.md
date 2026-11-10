# Rimix – visual inspiration survey

Research notes on how camera remote-control, tethering and on-set monitoring apps are laid out, gathered 2026-11-10 from web search and official docs. Images were not downloaded; where colour or typography is described, it comes from reviews, store listings or docs rather than from viewing screenshots, and is marked "reported" when thin. Each entry: layout, colour, type feel, and one idea worth stealing.

## 1. Sony's own apps

**Imaging Edge Desktop – Remote** ([features](https://support.d-imaging.sony.co.jp/app/imagingedge/en/instruction/1_1_features.php), [Remote window](https://support.d-imaging.sony.co.jp/app/imagingedge/en/instruction/4_2_remote_display.php), [Operation panel](https://support.d-imaging.sony.co.jp/app/imagingedge/en/instruction/4_3_panels.php)). Three zones: a thin toolbar (zoom, grid), a large Live View, and a stacked Operation panel of sub-panels (Main Settings: mode/shutter/aperture/ISO/EV; Sub Settings; Focus with AF/MF and near/far buttons; Shooting with Shutter/Movie/AF/AEL buttons; Histogram; Save). Panels can be reordered by drag; "settings will apply to the camera immediately." Reported as a flat, grey Windows-style utility with small type and dropdowns. Idea to steal: panel order is user-draggable and the camera mirrors every change instantly, with no "apply" step.

**Creators' App** ([App Store](https://apps.apple.com/gb/app/creators-app/id1616622856), [Phoblographer hands-on](https://www.thephoblographer.com/?p=220123)). Phone-first: live view fills the screen, a row of "quick tap icons" sits over the frame, and "a nice long menu with many more advanced options" lives behind one button. Reviewer needed "some menu digging to get the Live View working like I wanted." White/light Sony consumer styling; reviews complain about dropped connections and transfer permissions. Idea to steal: a short row of quick-tap settings on the frame, with everything else one level down.

**Monitor & Control (iPad/Mac)** ([App Store](https://apps.apple.com/us/app/sony-monitor-control/id1643900558), [Videomaker](https://www.videomaker.com/news/sony-releases-updated-monitor-control-app/)). Cinema-style dark monitor: full-bleed image with a "control bar at the side of the screen," switchable shutter speed/angle, waveform, false colour, histogram, zebra, vectorscope and real-time LUTs; multi-camera monitoring up to 20 bodies on a paid plan. Users call the scopes "low resolution and blocky," miss focus peaking and audio meters, and call the Mac build "a poorly optimized iPadOS port." Idea to steal: a toggle that turns Live View off "for more stable control" when you only need the parameters.

**Remote Camera Tool** ([help guide](https://helpguide.sony.net/ilc/1830/v1/en/contents/TP0002390917.html), [LAN remote shooting](https://helpguide.sony.net/ilc/2040/v1/en/contents/TP1000423586.html)). Sparse wired-LAN utility for PC remote shooting; documented as single-camera settings plus shutter, no live-view aids. Grey dialogue aesthetic. Idea to steal: nothing visual; it is a reminder that Sony leaves the pro desktop space thin.

## 2. Pro tethering

**Capture One (tethered Live View)** ([Capture One blog](https://www.captureone.com/blog/shooting-tethered-with-capture-one-pro), [Image Alchemist walkthrough](https://imagealchemist.net/capture-one-tethering-continued/), [support forum](https://support.captureone.com/hc/en-us/community/posts/360009384558-Tethering-Capture-Focusing-and-Live-View)). Live View opens as a separate window with a toolbar, two tool tabs and a viewer; the Camera tool "mirrors the camera's LCD" with clickable ISO/aperture/EV. Last capture replaces the viewer; a Focus Meter lets you place up to three focus areas whose bars rise as focus improves. Dark charcoal palette, small neutral sans type, dense. Idea to steal: numeric focus meters on user-placed points.

**Lightroom Classic tethered capture** ([Adobe help](https://helpx.adobe.com/lightroom-classic/help/import-photos-tethered-camera.html), [Lightroom Killer Tips](https://lightroomkillertips.com/a-look-at-the-latest-updates-to-shooting-tethered-part-1/)). A floating horizontal bar over the normal Library: camera name, shutter/aperture/ISO/WB as click-to-change values, Live toggle, big round shutter button, settings gear. Option-click collapses it to just the shutter. Dark grey, Adobe's compact sans. Idea to steal: a collapsible floating capture bar that shrinks to a single shutter button.

**Smart Shooter 5** ([Tether Tools blog](https://tethertools.com/blog/introducing-the-new-way-to-tether-with-smart-shooter-5/), [PetaPixel](https://petapixel.com/2023/06/27/smart-shooter-5-tethering-software-for-lightroom-adds-fujifilm-support)). "MyWorkspace" with dockable, saveable window layouts, dual-monitor support, up to 10 cameras with four simultaneous live views and camera groups; the selected camera receives commands. Reported utilitarian dark Qt look. Idea to steal: a transparent Overlay Mode on live view for aligning repeat shots.

**Cascable (iOS/iPad/Mac)** ([PetaPixel on v6 redesign](https://petapixel.com/2021/06/17/camera-control-app-cascable-revamps-interface-supports-phase-one), [Macworld review](https://www.macworld.com/article/230834/cascable-3-for-ios-review-use-your-iphone-as-a-full-featured-remote-for-your-wi-fi-enabled-dlslr-ca.html), [App Store](https://apps.apple.com/us/app/cascable-studio-tether-more/id974193500)). Live view with a strip of exposure "pills" that open pickers; the v6 remote is "optimized for all screen size combinations from the smallest iPhone to the largest iPad." Black UI, rounded cards, SF-style type; "Shutter Robot" automation and instant RAW previews. Idea to steal: one layout system that reflows from phone to desktop.

**digiCamControl** ([main window doc](https://www.digicamcontrol.com/doc/userguide/interface/mainwin), [Photo Art from Science](https://photoartfromscience.com/post/2018/03/29/remote-camera-control-using-digicamcontrol)). Windows WPF app: left camera/property panel, centre image or Live View, thumbnail strip below; a dedicated live view window with focus controls "in live view screen bottom part," histogram, click-to-set focus point (green square). Theme picker with light and dark schemes. Idea to steal: selectable themes and a persistent focus-step readout.

**Canon EOS Utility** ([Canon support](https://asia.canon/en/support/8203590600), [community thread](https://community.usa.canon.com/t5/Camera-Software/EOS-Utility-Live-View-with-Timer-Shooting/m-p/310534)). Small capture window mimicking the camera's top LCD, plus a separate Remote Live View window with draggable enlargement frame; Quick Preview pops each capture. Beige/grey dialogue chrome, tiny type; camera body controls are disabled while live view is open. Idea to steal: the drag-to-magnify frame for focus checks.

**Nikon NX Tether 2** ([Live View window help](https://nikonimglib.com/nx_tether/onlinehelp/en/03_liveview_window_15.html), [Digital Camera World](https://www.digitalcameraworld.com/news/nikon-nx-tether-update-brings-a-raft-of-new-features-to-its-free-tethering-app)). Live view window with histogram, focus points, grid, zebras, virtual horizon and zoom toggles; click image to move focus point; "adjust the live view and control displays to suit your workflow." Dark neutral, cleanly spaced. Idea to steal: virtual horizon overlay.

**Fujifilm X Acquire / Tether Shooting Plug-in PRO** ([Fujifilm guide](https://fujifilm-x.com/en-us/stories/fujifilm-x-acquire-features-users-guide/), [JM Peltier](https://www.jmpeltier.com/fujifilm-tethering-workarounds-lightroom-capture-one/)). X Acquire is a near-invisible menu-bar transfer agent to a hot folder; the Plug-in adds a Control Panel and LIVE VIEW window inside Lightroom. Idea to steal: a silent "watch and dump to folder" mode that other apps can consume.

## 3. Video monitoring

**Blackmagic Camera (iOS/iPad)** ([Good Design case study](https://good-design.org/projects/blackmagic-camera/), [Newsshooter first look](https://www.newsshooter.com/2023/09/15/blackmagic-camera-app-first-look/), [RedShark review](https://redsharknews.com/blackmagic-camera-app-for-iphone-reviewed-definitely-worth-the-download)). Full-bleed image; a HUD strip of tappable values (FPS, shutter angle, ISO, WB, tint) along the top with "a multi-level drawer system… adjustments made via a vertical strip" on the side; four tabs (Camera, Media, Chat, Cloud). Black background, Blackmagic's orange/yellow accents, condensed caps type like the Pocket 4K. Idea to steal: every HUD readout is also the button that edits it.

**Blackmagic Camera 2.0 multi-cam / Camera Control** ([MacRumors](https://www.macrumors.com/2024/08/07/blackmagic-camera-app-2-ipad-support/), [ProVideo Coalition](https://www.provideocoalition.com/blackmagic-adds-remote-controlling-and-monitoring-multiple-iphones-to-the-blackmagic-camera-app/)). iPad monitors "up to nine iPhone cameras" in a grid or scrolls them singly with focus/zoom/FPS/shutter/WB per tile; setup called "not entirely intuitive." Idea to steal: grid-or-single toggle for multi-cam.

**Atomos AtomOS** ([Videoguys on AtomOS 10](https://videoguys.com/blogs/news-and-sales/supercharge-your-atomos-ninja-v-monitor-recorder-with-atomos-10), [CineD](https://www.cined.com/?p=97577)). Image stays visible always; tap clears all overlays; a status bar under the image holds battery, record state, HDR waveform, audio meters and REC/PLAY/MON/EDIT. Overlay colours were deliberately desaturated so "the monitored image stands out." Idea to steal: desaturated chrome and a one-tap "clean feed."

**SmallHD PageOS 6** ([SmallHD OS 6](https://smallhd.com/pages/os-6), [touchscreen guide](https://guide.smallhd.com/a/833092-user-interface-quick-start-touchscreen), [Newsshooter](https://www.newsshooter.com/2025/02/04/smallhd-pageos-6-0-1-firmware-update/)). Up to eight user-built "Pages", each a preset combination of tools; swipe down for a zoomed-out page overview; exposure, colour and record are "anchored on every Page." Dark UI, bold white labels readable from a metre away; GPI tally draws a red border when rolling. Idea to steal: Pages—named tool bundles you flip between.

**Frame.io Camera to Cloud** ([getting started](https://support.frame.io/en/articles/8896457-c2c-getting-started-with-camera-to-cloud), [ProVideo Coalition review](https://www.provideocoalition.com/review-frame-io-camera-to-cloud-atomos-shogun-connect/)). Web dashboard: a C2C Connections tab lists paired devices with three-dot menus (Suspend/Forget), QR-code pairing, uploads auto-filed by date/type/device. Dark slate UI, large thumbnails, generous type. Idea to steal: device list with pairing code and suspend/forget actions.

**Teradek VUER / Serv Pro** ([VUER rundown](https://teradek.com/blogs/articles/vuer-a-rundown), [ProVideo Coalition](https://www.provideocoalition.com/review-teradek-serv-pro-vuer/)). 2x2, dual or single multiview; waveform with its own opacity/position options, vectorscope modes, false colour in Discrete/Full/Zebra, LUTs per feed or all, frame grab. Black UI with small icon buttons on all four edges. Idea to steal: apply a look to one feed or all feeds.

**Accsoon SEE / Zacuto iPhone EVF** ([Newsshooter](https://www.newsshooter.com/2023/04/19/accsoon-zacuto-iphone-evf-solution-first-look-at-nab-2023/), [B&H](https://www.bhphotovideo.com/explora/node/114066)). Phone-as-monitor with LUTs, record and stream; the Z-Finder mount leaves a strip of screen free for touch controls. Idea to steal: keep a thin control margin even when the image is fullscreen.

## 4. Consumer / creator

**DJI Mimo** ([App Store](https://apps.apple.com/gb/app/id1431720653), [CineD Pocket 3 review](https://www.cined.com/es/dji-osmo-pocket-3-review-is-it-any-good-for-professional-documentary-work/)). Live view with mode carousel along the bottom, PRO toggle revealing ISO/shutter/EV strips; recent redesign to a "white modular layout" for the home dashboard. Idea to steal: PRO mode as an explicit switch that reveals manual strips.

**Insta360** ([Insta360 blog](https://insta360.com/blog/news/insta360-app-update-new-interface.html), [RedShark](https://www.redsharknews.com/insta360-updates-mobile-app-with-new-interface-and-much-faster-exporting)). Swipe left/right on the bottom to change modes, swipe up for the full option sheet; black UI with yellow accent; one store review called the new layout "terrible" and switched back. Idea to steal: the swipe-up sheet for rarely used options.

**GoPro Quik** ([Digital Camera World](https://www.digitalcameraworld.com/reviews/gopro-quik-app-review), [TechRadar](https://www.techradar.com/pro/software-services/gopro-quik-review)). Preview in the top half, tools along the bottom; blue-accent dark UI, heavy rounded type; 2024 reviews report frequent crashes. Idea to steal: a big thumb-friendly mode/record cluster.

**Halide Mark II** ([Lux launch post](https://www.lux.camera/pro-camera-action-introducing-halide-mark-ii/), [PetaPixel](https://petapixel.com/2020/10/22/halide-camera-app-gets-huge-mark-ii-update-with-new-design-instant-raw-and-more)). "All controls fall within easy thumb reach"; swipe up/down for exposure, a focus dial that activates a Focus Loupe only while dragging ("Intelligent Activation"); histogram or waveform tucked beside the notch. Black with custom "Ambrotype" typeface, including a monospaced cut for values, evoking engraved camera text. Idea to steal: a monospaced numerals face so values don't jitter, plus tools that appear only while in use.

**Filmic Pro v6/v7** ([CineD v7](https://www.cined.com/filmic-pro-v7-launched-redesigned-ui-custom-function-button-and-more/), [PetaPixel v7](https://petapixel.com/2022/12/09/filmic-pro-v7-brings-new-features-and-a-redesigned-camera-interface), [180° shutter doc](https://filmicpro.helpscoutdocs.com/article/37-achieving-a-180-degree-shutter-in-filmic-pro)). v6 used two arc sliders (left thumb exposure, right thumb focus) and separate circle/square reticles that turn red when locked; v7 replaced arcs with vertical sliders, "Quick Action Modal" groups, a mappable Fn button and an Action Slider with live readouts. Dark UI with red lock states. Idea to steal: lock state shown by colouring the control red; separate exposure and focus targets.

## 5. Streaming

**OBS Studio 31/32** ([AlternativeTo on 32.1](https://alternativeto.net/news/2026/4/obs-studio-32-1-released-with-overhauled-audio-mixer-webrtc-simulcast-support-and-more), [forum reaction](https://obsproject.com/forum/threads/can-we-get-the-old-ui-back-in-the-settings.196365/)). Preview centred, dockable Scenes/Sources/Mixer/Controls panes; 31.1 added font-size and density options; 32 moved to a vertical mixer that some users found "unfamiliar and strange." Dark grey Qt. Idea to steal: a user-set density slider.

**Elgato Camera Hub** ([Elgato Prompter page](https://www.elgato.com/ww/zh/explorer/products/teleprompter/elgato-prompter-pro-teleprompting-made-easy/), [SourceForge](https://sourceforge.net/app/elgato-camera-hub/)). Large preview left, settings column right with sliders for exposure/picture/processing, "three areas", framing grid, and a compact mode that "keeps controls accessible without dominating the desktop." Dark, modern, roomy; Stream Deck integration. Idea to steal: compact mode.

**Ecamm Live** ([Primal Video review](https://primalvideo.com/guides/ecamm-live-review-best-livestream-software-for-mac), [Six Colors](https://sixcolors.com/post/2022/04/learning-to-love-ecamm-live/)). Native Mac windows, Scenes and Overlays panel on the right, floating source bar over the preview, hotkeys per scene; light/dark follows macOS. Idea to steal: feel like a Mac app, not a cross-platform shell.

## 6. Open-source Sony controllers (from README text)

**RemoteCam** ([GitHub](https://github.com/RemingtonWilcox/RemoteCam)). WPF/Avalonia/SwiftUI; "Three tabs, nothing hidden in menus" (Video, Photo, Setup); picture large in the middle with a bar beneath for histogram/zebra/peaking/grid toggles; histogram and meter float over a corner and are draggable; presets C1–C4 "turn green while the camera matches"; red outline around the picture when recording; window opens at the frame's aspect ratio. Idea to steal: preset buttons that light up when the camera state matches them.

**SonyCamViewer** ([GitHub](https://github.com/Scryptor/SonyCamViewer)). SwiftUI/Compose, ZV-E10 as reference. Live view fills the screen, status bar on top, a bottom row of values "like the camera's own display," a left strip of camera tiles for up to four bodies, right-side Fn/zoom/focus/record buttons that auto-hide; a 14-tile Monitor menu for waveform/vectorscope/false colour/LUTs; red frame when recording, orange dot on the preset button when settings drift. Idea to steal: mirror the camera's own bottom info row so the owner already knows how to read it.

**alpha2web** ([GitHub](https://github.com/Knallli/alpha2web)). Go web UI: dashboard (connection, logs, Immich queue), MJPEG live view, gallery, and a settings page of cards grouped by exposure/focus/drive/WB, each property getting "a fitting widget: switch, select, slider, number box"; dangerous actions behind an explicit enable. Idea to steal: property cards with search/filter, and a safety gate on destructive settings.

**onair-camera-control** ([GitHub](https://github.com/jambyylf/onair-camera-control)). WebView2 app for three FX30s: one tile per camera with REC timer, ISO/Iris/Shutter/WB/Picture Profile pickers, AF/MF, near/far, audio; a bottom bar with MASTER REC, MATCH (copy look to all) and Presets; fullscreen monitor with grid/histogram/zebra/peaking; phone and OBS Browser Source views. Idea to steal: MATCH – copy the selected camera's look to the others.

## 7. Concept work (Dribbble / Behance)

- [Ember – Wireless Camera Control App](https://www.dribbble.com/shots/22258592-Ember-Wireless-Camera-Control-App): Wi-Fi control dashboard for photo and video (resolution, frame rate, audio); tagged dashboard/controller; dark cards, large numerals.
- [Camera Controller Concept (yuhang)](https://dribbble.com/GregoryKovalev/collections/4504235-Idea-project): a dial-and-viewport layout where the live image sits in a window beside a big rotary control.
- [Fujifilm Camera Remote App concept (Victor Yuyang Luo)](https://dribbble.com/luoyyisvic): onboarding, home, aperture-control and photos screens, with a dedicated dark-mode variant; aperture as a horizontal scrub strip.
- [Fuji Film Camera Remote redesign (bigchaz collection)](https://dribbble.com/bigchaz/collections/552948-Drone-Sports-App): rounded shutter button under a tape-style exposure gauge, 280+ likes.
- [Camera Remote App – dial/knob (apankrat "Good UI" collection)](https://dribbble.com/apankrat/collections/28461-Good-UI): skeuomorphic textured wheels; a reminder that realism reads well in stills but dates fast.
- [Light Manual Camera App (Afnizar Nur Ghifari)](https://dribbble.com/tags/dslr): flat-minimal viewfinder with skeuomorphic shutter; exposure values as a thin ruler.
- [Halide – Manual Mode (Sebastiaan de With)](https://dribbble.com/shots/3538909-Halide): the canonical "ruler slider" for exposure on black.
- [Fujifilm App Redesign – student project (Manuela Santos, Behance)](https://www.behance.net/gallery/80103633/Fujifilm-App-Redesign-Student-Project): remote screen with live view top, settings bottom; light theme, high whitespace.
- [Camshare – camera sharing app (Behance)](https://www.behance.net/gallery/87276939/Camshare-Camera-Sharing-App-Full-Preview?locale=en_US): gallery-first navigation with camera tiles.
- [Canon DSLR Bluetooth remote concept (Jeremey Fleischer)](https://weandthecolor.com/camera-remote-iphone-app-concept-by-jeremey-fleischer/16221): black UI, red accent, big circular shutter and scrolling value drums.

## 8. Synthesis

### Recurring patterns

1. **Live view is the hero.** Nearly every app puts the image full-bleed or centred and shrinks everything else ([AtomOS](https://videoguys.com/blogs/news-and-sales/supercharge-your-atomos-ninja-v-monitor-recorder-with-atomos-10), [RemoteCam](https://github.com/RemingtonWilcox/RemoteCam), [Blackmagic](https://good-design.org/projects/blackmagic-camera-app/)).
2. **Exposure as a strip of tappable values**, each opening a picker, drum or slider rather than a form ([Blackmagic](https://good-design.org/projects/blackmagic-camera-app/), [Lightroom bar](https://helpx.adobe.com/lightroom-classic/help/import-photos-tethered-camera.html), [Cascable](https://petapixel.com/2021/06/17/camera-control-app-cascable-revamps-interface-supports-phase-one)).
3. **Record state = red border plus a timer**, never just a dot ([SonyCamViewer](https://github.com/Scryptor/SonyCamViewer), [PageOS GPI tally](https://smallhd.com/pages/os-6), [RemoteCam](https://github.com/RemingtonWilcox/RemoteCam)).
4. **Dark by default; light is the exception** (Sony Creators' App, Mimo home, Behance student work). Monitoring tools go further and desaturate chrome ([AtomOS](https://videoguys.com/blogs/news-and-sales/supercharge-your-atomos-ninja-v-monitor-recorder-with-atomos-10)).
5. **Scopes float and are draggable/resizable** ([RemoteCam](https://github.com/RemingtonWilcox/RemoteCam), [SonyCamViewer](https://github.com/Scryptor/SonyCamViewer), [VUER](https://teradek.com/blogs/articles/vuer-a-rundown)).
6. **Captured images show as a filmstrip or last-shot thumbnail**, with the newest replacing the viewer in photo tethering ([Capture One](https://imagealchemist.net/capture-one-tethering-continued/), [digiCamControl](https://www.digicamcontrol.com/doc/userguide/interface/mainwin), [Halide](https://www.lux.camera/pro-camera-action-introducing-halide-mark-ii/)).
7. **Settings depth is split**: a short quick row on the frame, a long list behind one button ([Creators' App](https://www.thephoblographer.com/?p=220123), [Insta360 swipe-up](https://insta360.com/blog/news/insta360-app-update-new-interface.html), [Filmic QAMs](https://www.cined.com/filmic-pro-v7-launched-redesigned-ui-custom-function-button-and-more/)).
8. **Multi-cam = tiles with one "selected" camera** that receives commands, plus a master record ([Smart Shooter](https://tethertools.com/blog/introducing-the-new-way-to-tether-with-smart-shooter-5/), [onair](https://github.com/jambyylf/onair-camera-control), [Blackmagic 2.0](https://www.macrumors.com/2024/08/07/blackmagic-camera-app-2-ipad-support/)).
9. **Dockable/saveable workspaces** in desktop pro tools; **Pages** in monitors ([Smart Shooter](https://tethertools.com/blog/introducing-the-new-way-to-tether-with-smart-shooter-5/), [OBS](https://alternativeto.net/news/2026/4/obs-studio-32-1-released-with-overhauled-audio-mixer-webrtc-simulcast-support-and-more), [PageOS](https://smallhd.com/pages/os-6)).
10. **Single-letter keyboard toggles** for overlays (H, Z, P, G) and Space/F9 for record ([RemoteCam](https://github.com/RemingtonWilcox/RemoteCam), [onair](https://github.com/jambyylf/onair-camera-control)).
11. **Presets that reflect state**, not just apply it (green when matched, orange dot when drifted) ([RemoteCam](https://github.com/RemingtonWilcox/RemoteCam), [SonyCamViewer](https://github.com/Scryptor/SonyCamViewer)).
12. **Transfer is a separate, quieter surface**: dashboards, queues, device lists ([Frame.io C2C](https://support.frame.io/en/articles/8896457-c2c-getting-started-with-camera-to-cloud), [alpha2web](https://github.com/Knallli/alpha2web), [X Acquire](https://fujifilm-x.com/en-us/stories/fujifilm-x-acquire-features-users-guide/)).

### Standout ideas worth borrowing

- **Readout-is-the-control HUD** (Blackmagic Camera): tap the shutter value to edit it, no separate panel.
- **Pages / anchored functions** (SmallHD PageOS 6): named overlay bundles, with exposure and record fixed on all of them.
- **Collapsible floating capture bar** (Lightroom Classic): shrinks to just the shutter when the image matters.
- **Presets that light up when matched** (RemoteCam) and **drift indicator** (SonyCamViewer).
- **MATCH – copy look to all cameras** (onair-camera-control).
- **Desaturated chrome and tap-to-clear overlays** (AtomOS 10).
- **Monospaced numerals and appear-only-while-dragging tools** (Halide Mark II).
- **Red lock state and separate exposure/focus reticles** (Filmic Pro).

### Pitfalls seen in real apps

- Cramped Windows-utility chrome and dropdowns for everything (Imaging Edge, EOS Utility, Remote Camera Tool).
- "Menu digging" to reach live view or manual mode ([Creators' App](https://www.thephoblographer.com/?p=220123)); PRO mode silently resetting ([Mimo](https://apps.apple.com/gb/app/id1431720653)).
- Blocky, low-resolution scopes and missing basics like peaking or audio meters ([Monitor & Control](https://apps.apple.com/us/app/sony-monitor-control/id1643900558)).
- Ports that don't feel native on Mac ("poorly optimized iPadOS port").
- Tiny values unreadable from a tripod's distance; monitors solve this with bold white labels on dark.
- Layout churn that users reject ([OBS vertical mixer](https://obsproject.com/forum/threads/can-we-get-the-old-ui-back-in-the-settings.196365/), [Insta360](https://insta360.com/blog/news/insta360-app-update-new-interface.html)).
- Multi-cam setup "not entirely intuitive" and tied to network quirks ([Blackmagic 2.0](https://forums.macrumors.com/threads/blackmagic-camera-app-2-0-adds-multi-iphone-control-and-ipad-version.2433108)).

### Three suggested directions for Rimix

**A. "Field monitor" (recommended for a ZV-E10 solo creator).** Black, full-bleed live view; a single HUD strip of tappable values along the bottom in the same order as the camera's own display (SonyCamViewer); a thin right-hand rail for scopes/overlays toggles and record; red border plus timer when rolling. Chrome desaturated (AtomOS), values in a monospaced face (Halide), big enough to read from two metres. Presets as four lit buttons. Why: it fits one person checking framing and exposure across the room, scales naturally to multi-cam tiles and scopes, and avoids the cramped-utility trap. Risk: fewer visible settings at once, so the long list must be one keystroke away.

**B. "Mac-native tether studio".** A proper macOS window with sidebar (cameras, presets, transfer queue), centre live view that swaps to the last capture, a Lightroom-style floating capture bar that collapses to a shutter, and a Capture One-style inspector on the right with property cards (alpha2web). Light/dark follows the system (Ecamm). Why: strongest for stills, image transfer and feeling at home on macOS; easiest for a non-designer because AppKit/SwiftUI conventions do the design. Risk: looks like software rather than gear; video scopes feel bolted on.

**C. "Pages dashboard".** Borrow PageOS: user-named Pages (Frame, Expose, Record, Transfer, Multi), each a saved arrangement of live view, scopes, exposure strip and filmstrip; exposure and record anchored on every page; swipe/⌘-number to flip; a zoomed-out page overview. Dark slate with one accent colour reserved for record/alerts. Why: grows gracefully from today's single camera to multi-cam and scopes without redesign, and gives the owner an obvious place to put each future feature. Risk: more to build up front; needs good defaults so Pages aren't an empty canvas.

Suggested pick: start with A for v1, with B's transfer sidebar as a slide-over, and keep C's "anchored record and exposure on every view" as a rule from day one.
