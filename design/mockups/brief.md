# Rimix mockup brief

Rimix is a desktop app (macOS first, Windows second, built with Tauri) that connects to Sony Alpha cameras over Wi-Fi and gives the photographer live view, remote shooting, camera settings, and image transfer, the way Sony's phone app does but on a laptop. The owner's camera is a Sony ZV-E10, a small vlogging-oriented mirrorless camera. Users are solo creators and photographers, not broadcast engineers (though a multi-camera mode comes later).

Each mockup is ONE self-contained HTML file showing the SAME two screens with the SAME content, so the owner can compare pure design direction. Do not change the content; change how it looks and is organised.

## Screen 1: Main screen (fills the first viewport, target 1440x900, must still work at 1100x700)

Content that must appear:
- App name: Rimix. Camera: ZV-E10, connected over Wi-Fi Direct, signal strong, battery 72%, card slot 1 with 238 shots left, firmware 2.00.
- Live view: a 16:9 image area using the shared placeholder scene (SVG below). Overlays on it: a focus frame (green, locked), a rule-of-thirds grid (subtle), a small histogram, a level indicator. Make the live view the biggest element.
- Exposure controls, all visible without opening menus: Mode M, Shutter 1/125, Aperture f/4.0, ISO 800, Exposure comp +0.3, White balance 5200K, Focus AF-C, Drive Single, Format RAW+JPEG, Picture profile Off.
- Primary actions: Shoot (the shutter button, the most prominent control), Record (movie), Focus (autofocus now), and four preset slots C1 C2 C3 C4 (C1 is active, named "Talking head").
- Capture destination row: saving to "~/Pictures/Rimix/2026-11-10", with three selectable options: "Custom folder", "Lightroom watched folder", "New folder per date" (the third is selected).
- Filmstrip of the last six captures: DSC01231 to DSC01236, each a small tile using a cropped/tinted version of the scene (CSS filter or gradient is fine), DSC01236 marked as "transferring 64%".
- Secondary tools, visible but quieter: Scopes (Waveform, False colour, Zebra), Interval timer (off), Cameras switcher showing "ZV-E10 (this)" and "a7 IV (offline)".
- Status line: "Live view 24 fps · last shot 0.8 s ago".

## Screen 2: Connect screen (directly below screen 1, its own full-height section; this is what a user sees before a camera is connected)

Content that must appear:
- Headline: "Connect a camera".
- Three ways, equally visible: (1) "Scan the QR code" with a webcam preview placeholder and the instruction "On the camera: Menu, Network, Send to Smartphone, then show the QR code"; (2) "Nearby over Bluetooth" listing "ZV-E10 · pairing enabled · 2 m away" with a Connect button; (3) "Enter the camera's address" with an IP field prefilled "192.168.122.1" and a Connect button.
- A note: "First time on this camera? Press OK on the camera screen when it asks to allow this computer."
- A small list of "Recently used": "ZV-E10 (Wi-Fi Direct)", "a7 IV (home router)".

## Shared placeholder scene (inline this SVG exactly once per live view; you may wrap it, you may not replace it with a photo)

<svg viewBox="0 0 1600 900" preserveAspectRatio="xMidYMid slice" xmlns="http://www.w3.org/2000/svg" role="img" aria-label="Placeholder scene: a person at a desk by a window">
  <defs>
    <linearGradient id="sky" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#9fb7c9"/><stop offset="1" stop-color="#e3d9c6"/></linearGradient>
    <linearGradient id="wall" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#5c6168"/><stop offset="1" stop-color="#3a3e44"/></linearGradient>
    <linearGradient id="desk" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#8a6a4a"/><stop offset="1" stop-color="#5b4430"/></linearGradient>
    <radialGradient id="face" cx="0.4" cy="0.35" r="0.8"><stop offset="0" stop-color="#e8c3a6"/><stop offset="1" stop-color="#b98a6b"/></radialGradient>
  </defs>
  <rect width="1600" height="900" fill="url(#wall)"/>
  <rect x="980" y="80" width="460" height="520" rx="8" fill="url(#sky)"/>
  <rect x="1200" y="80" width="14" height="520" fill="#2b2e33"/>
  <rect x="980" y="330" width="460" height="12" fill="#2b2e33"/>
  <rect x="0" y="620" width="1600" height="280" fill="url(#desk)"/>
  <rect x="300" y="560" width="220" height="130" rx="10" fill="#1d1f23"/>
  <rect x="312" y="572" width="196" height="106" rx="4" fill="#2f3f55"/>
  <ellipse cx="700" cy="620" rx="150" ry="60" fill="#2a2d33"/>
  <path d="M560 640 Q700 400 840 640 Z" fill="#2f5d8a"/>
  <circle cx="700" cy="400" r="95" fill="url(#face)"/>
  <path d="M605 395 Q700 270 795 395 Q760 330 700 330 Q640 330 605 395 Z" fill="#2b2320"/>
  <rect x="1120" y="520" width="120" height="140" rx="4" fill="#4b5563"/>
  <circle cx="1180" cy="455" r="55" fill="#d8b36a" opacity="0.7"/>
</svg>

## Rules for every mockup

- One HTML file, everything inline (CSS, SVG, any tiny JS). Google Fonts via <link> are allowed and encouraged, but every font must have a real fallback stack so the page still looks right offline.
- Static mockup: buttons do not need to work, but hover and keyboard focus states must be visible. No frameworks, no build step.
- No lorem ipsum, no fake people names, no placeholder images from the web. Use the content above verbatim.
- A single committed theme (dark or light) is fine for an app mockup; pick what the direction calls for and set every colour explicitly, including the body background.
- Desktop-first: design for 1440x900, keep it usable at 1100x700, and do not let the page scroll horizontally.
- Respect prefers-reduced-motion. No decorative page-load animations.
- Spend boldness in one place. Each design has exactly one memorable idea; everything else is quiet and disciplined.
- Avoid the generic AI look: no cream background with a serif and a terracotta accent; no near-black with a single acid green; no identical rounded cards with the same grey shadow on everything; no all-caps tracked eyebrow labels on every heading; no middle-dot meta strings everywhere; no monospace for every small label unless the direction is specifically about that; no "→" appended to buttons.
- Write copy from the user's side: "Shoot", "Record", "Save to", "Connect". Sentence case.
- Put a <title> that is the design's name (for example "Rimix — Viewfinder") and, as the first element in the body, a small fixed badge in a corner that says the design's name and number so screenshots are identifiable.
- At the very end of the file, inside an HTML comment, write 3 to 5 lines: the design idea in one sentence, the palette as hex values, the typefaces, and what you deliberately avoided.
