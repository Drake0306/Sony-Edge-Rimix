// The connect screen: scan the camera's QR code, pick a nearby camera, or
// type its Wi-Fi name and password (with the Access Authentication login and
// the address field under their own disclosures), plus recently used cameras.

import type { Actions } from '../actions';
import { fingerprintChangedText, fingerprintText, NOT_A_WIFI_CODE, webcamErrorMessage } from '../copy';
import { button, el, icon } from '../dom';
import { discover, type SshLogin } from '../ipc';
import { parseWifiQr } from '../qr/payload';
import { createScanner, webcamSupported } from '../qr/scanner';
import { cameraCount, watch, type AppStore } from '../state';
import { rowButton } from './sidePanels';

export const DEFAULT_IP = '192.168.122.1';
const DISCOVER_MS = 3000;
const REFRESH_MS = 5000;
/** How long after a failed connect the webcam starts looking for a code again. */
const RESCAN_MS = 3000;
const NOTE = 'First time on this camera? Press OK on the camera screen when it asks to allow this computer.';
const SCANNING = "Hold the camera's QR code up to the webcam.";
export const SSH_PASSWORD_NEEDED = 'Enter the password shown under Access Authen. Info.';
const INFO_GLYPH =
  '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" aria-hidden="true"><circle cx="8" cy="8" r="6.5"/><path d="M8 7v4M8 5v.2" stroke-linecap="round"/></svg>';

function way(title: string, ...children: HTMLElement[]): HTMLElement {
  const a = el('article', 'way glass');
  a.append(el('h2', '', title), ...children);
  return a;
}

function input(id: string, label: string, value = ''): [HTMLLabelElement, HTMLInputElement] {
  const lbl = el('label', 'sr', label);
  lbl.htmlFor = id;
  const i = el('input');
  i.id = id;
  i.type = 'text';
  i.value = value;
  i.spellcheck = false;
  i.autocomplete = 'off';
  return [lbl, i];
}

/** A disclosure button and the box it shows and hides. */
function disclosure(text: string, box: HTMLElement): HTMLButtonElement {
  const t = button('textbtn disclosure', text);
  t.setAttribute('aria-expanded', 'false');
  box.hidden = true;
  return t;
}

function setOpen(toggle: HTMLButtonElement, box: HTMLElement, open: boolean): void {
  box.hidden = !open;
  toggle.setAttribute('aria-expanded', String(open));
}

/** A password field with its Show/Hide button; `extra` goes after them. */
function passwordRow(id: string, label: string, ...extra: HTMLElement[]): [HTMLElement, HTMLInputElement] {
  const [lbl, i] = input(id, label);
  i.type = 'password';
  i.placeholder = label;
  const show = button('textbtn', 'Show');
  show.addEventListener('click', () => {
    const hidden = i.type === 'password';
    i.type = hidden ? 'text' : 'password';
    show.textContent = hidden ? 'Hide' : 'Show';
  });
  const row = el('div', 'field');
  row.append(lbl, i, show, ...extra);
  return [row, i];
}

function submitButton(): HTMLButtonElement {
  const b = button('pbtn', 'Connect');
  b.type = 'submit';
  return b;
}

export function mount(root: HTMLElement, store: AppStore, actions: Actions): () => void {
  const backdrop = el('div', 'webcam');
  backdrop.setAttribute('aria-hidden', 'true');
  backdrop.append(icon('scene-copy', '', '0 0 1600 900'));
  backdrop.firstElementChild!.setAttribute('preserveAspectRatio', 'xMidYMid slice');
  const vignette = el('div', 'vignette');

  const status = el('header', 'status glass capsule');
  status.setAttribute('aria-label', 'Status');
  const quietword = el('span', 'quietword', 'No camera connected');
  status.append(el('span', 'brand', 'Rimix'), quietword);

  const headline = el('div', 'headline');
  // While cameras stay connected (adding another), a way back to them.
  const back = button('textbtn back');
  back.hidden = true;
  back.addEventListener('click', () =>
    store.set({ screen: 'main', keepCameras: false, pendingFingerprint: null, sshPrefill: null, error: null }),
  );
  const message = el('p', 'message');
  message.setAttribute('role', 'status');
  // A camera's SSH fingerprint to compare with its screen before connecting.
  const confirm = el('div', 'fingerprint glass');
  confirm.hidden = true;
  confirm.setAttribute('role', 'alert');
  const confirmText = el('p');
  const matches = button('pbtn', 'It matches, connect');
  const cancel = button('textbtn', 'Cancel');
  const confirmRow = el('div', 'row');
  confirmRow.append(matches, cancel);
  confirm.append(confirmText, confirmRow);
  headline.append(
    back,
    el('h1', '', 'Connect a camera'),
    el('p', '', 'Three ways in. Pick whichever is nearest to hand.'),
    message,
    confirm,
  );

  // 1. Scan the QR code
  const video = el('video');
  video.muted = true;
  video.setAttribute('muted', '');
  video.setAttribute('playsinline', '');
  video.setAttribute('autoplay', '');
  const canvas = el('canvas');
  canvas.hidden = true;
  const preview = el('div', 'preview');
  const tag = el('span', 'tag', 'Webcam');
  tag.prepend(el('i'));
  preview.append(video, canvas, tag, el('div', 'brk'));
  const scanStatus = el('p', 'scanstatus');
  scanStatus.setAttribute('role', 'status');
  const scanAgain = button('textbtn', 'Scan again');
  scanAgain.hidden = true;
  const scanRow = el('div', 'scanrow');
  scanRow.append(scanStatus, scanAgain);
  const pasteToggle = button('textbtn', "Paste the code's text");
  const pasteBox = el('div', 'field paste');
  pasteBox.hidden = true;
  const area = el('textarea');
  area.rows = 2;
  area.spellcheck = false;
  area.setAttribute('aria-label', "The code's text");
  const useText = button('pbtn', 'Use this text');
  pasteBox.append(area, useText);
  const qr = way(
    'Scan the QR code',
    preview,
    scanRow,
    el('p', '', 'On the camera: Menu, Network, Send to Smartphone Func., Send to Smartphone.'),
    pasteToggle,
    pasteBox,
  );

  // 2. Nearby
  const nearbyList = el('div', 'devices');
  const nearby = way(
    'Nearby',
    nearbyList,
    el('p', 'fill', 'Cameras with PC Remote on appear here when this computer is on the same Wi-Fi.'),
  );

  // 3. Type the Wi-Fi name and password, with the address under Advanced
  const creds = el('form', 'creds');
  const [ssidLabel, ssid] = input('ssid', 'Wi-Fi name');
  ssid.placeholder = 'DIRECT-xxxx:ZV-E10';
  const ssidRow = el('div', 'field');
  ssidRow.append(ssidLabel, ssid);
  const [pwRow, pw] = passwordRow('pw', 'Password', submitButton());
  creds.append(ssidRow, pwRow);

  // Access Authentication: the camera's SSH login, never stored.
  const ssh = el('div', 'ssh');
  ssh.setAttribute('role', 'group');
  ssh.setAttribute('aria-label', 'Access Authentication');
  const sshToggle = disclosure('This camera uses Access Authentication', ssh);
  const [userLabel, sshUser] = input('ssh-user', 'Username');
  sshUser.placeholder = 'Username';
  const userRow = el('div', 'field');
  userRow.append(userLabel, sshUser);
  const [sshPwRow, sshPw] = passwordRow('ssh-pw', 'Password');
  ssh.append(userRow, sshPwRow, el('p', '', 'Shown on the camera under Access Authen. Info.'));

  const advanced = el('div', 'advanced');
  const advToggle = disclosure('Advanced: connect by address', advanced);
  const addrForm = el('form', 'field');
  const [ipLabel, ip] = input('ip', 'Camera address', DEFAULT_IP);
  addrForm.append(ipLabel, ip, submitButton());
  advanced.append(addrForm, el('p', '', 'Shown on the camera under Network, Wi-Fi, Display Wi-Fi info.'));
  const typed = way(
    'Type the Wi-Fi name and password',
    creds,
    el('p', '', 'Shown on the camera under Menu, Network, PC Remote Function, Wi-Fi Direct Info.'),
    sshToggle,
    ssh,
    advToggle,
    advanced,
  );

  const ways = el('div', 'ways');
  ways.append(qr, nearby, typed);

  const note = el('p', 'note glass quiet');
  note.innerHTML = INFO_GLYPH;
  note.append(NOTE);
  const recent = el('div', 'recent glass quiet');
  const recentGroup = el('div', 'group');
  recent.append(el('div', 'h', 'Recently used'), recentGroup);
  const foot = el('div', 'foot');
  foot.append(note, recent);

  const parts = [backdrop, vignette, status, headline, ways, foot];
  root.append(...parts);

  // Every Connect button, Use this text and the recent rows wait while a connect runs.
  const applyBusy = () => {
    const busy = store.get().connecting !== null;
    root.querySelectorAll<HTMLButtonElement>('.pbtn, .recent button').forEach((b) => (b.disabled = busy));
  };

  let stopped = false;
  let rescanTimer = 0;
  const newScanner = (deviceId: string | null | undefined) =>
    createScanner({ deviceId, onResult: (text) => void useCode(text, true) });
  let scanner = newScanner(store.get().settings?.webcamId);
  const startScanner = () => {
    scanStatus.textContent = SCANNING;
    scanner.start(video, canvas).catch((e) => {
      if (!stopped) scanStatus.textContent = webcamErrorMessage(e);
    });
  };
  /** Starts again at once, with the webcam the settings name now. */
  const restartScanner = () => {
    window.clearTimeout(rescanTimer);
    scanner.stop();
    scanner = newScanner(store.get().settings?.webcamId);
    if (webcamSupported()) startScanner();
  };

  // The text of a scanned code whose connect failed. Reading it again does
  // nothing, so a code left in view does not retry forever and the error stays.
  let failedText: string | null = null;
  // The webcam watch below fires once on mount, before the first start.
  let scannerStarted = false;
  const setFailedText = (text: string | null) => {
    failedText = text;
    scanAgain.hidden = text === null;
  };

  /** The login to send with a connect: only while the box is open with a username. */
  const login = (): SshLogin | null => {
    const user = sshUser.value.trim();
    return !ssh.hidden && user ? { user, password: sshPw.value, fingerprint: null } : null;
  };

  /**
   * False, with the password field focused and the reason shown, while the
   * Access Authentication box is open without a password.
   */
  const loginReady = (): boolean => {
    if (ssh.hidden || sshPw.value) return true;
    store.set({ error: SSH_PASSWORD_NEEDED });
    sshPw.focus();
    return false;
  };

  const openLogin = () => {
    setOpen(sshToggle, ssh, true);
    (sshUser.value.trim() ? sshPw : sshUser).focus();
  };

  const showPaste = () => {
    pasteBox.hidden = false;
  };

  const rescan = () => {
    if (!stopped) rescanTimer = window.setTimeout(startScanner, RESCAN_MS);
  };

  // The webcam may read another QR code first (Sony's app link, say); the
  // name field takes focus only the first time, so typing is not interrupted.
  let focusedForCode = false;

  /** Connects with a code's text, or explains why it is not a Wi-Fi code. */
  async function useCode(text: string, fromScanner: boolean): Promise<void> {
    const wifi = parseWifiQr(text);
    if (!wifi && !fromScanner) {
      store.set({ error: NOT_A_WIFI_CODE });
      showPaste();
      ssid.focus();
      return;
    }
    if (!wifi) {
      // A stray code left in view is read again every few seconds; it must
      // not replace progress or another error the user still needs.
      const { connecting, error } = store.get();
      if (connecting || (error !== null && error !== NOT_A_WIFI_CODE)) {
        rescan();
        return;
      }
      store.set({ error: NOT_A_WIFI_CODE });
      showPaste();
      if (!area.value) area.value = text;
      if (!focusedForCode) ssid.focus();
      focusedForCode = true;
      rescan();
      return;
    }
    if (!fromScanner) {
      if (store.get().connecting === null && loginReady()) await actions.connectWithWifi(wifi, login());
      return;
    }
    if (text === failedText || store.get().connecting !== null) {
      rescan();
      return;
    }
    // Show what was read, so a failure can be corrected by hand.
    ssid.value = wifi.ssid;
    pw.value = wifi.password;
    area.value = text;
    showPaste();
    if (!loginReady()) {
      rescan();
      return;
    }
    const ok = await actions.connectWithWifi(wifi, login());
    if (!ok) {
      if (!stopped) setFailedText(text);
      rescan();
    }
  }

  pasteToggle.addEventListener('click', () => {
    showPaste();
    area.focus();
  });
  useText.addEventListener('click', () => void useCode(area.value, false));
  scanAgain.addEventListener('click', () => {
    setFailedText(null);
    restartScanner();
  });
  creds.addEventListener('submit', (e) => {
    e.preventDefault();
    const name = ssid.value.trim();
    if (!name) {
      ssid.focus();
      return;
    }
    if (!loginReady()) return;
    void actions.connectWithWifi({ ssid: name, password: pw.value }, login());
  });
  sshToggle.addEventListener('click', () => {
    setOpen(sshToggle, ssh, ssh.hidden);
    if (!ssh.hidden) sshUser.focus();
  });
  advToggle.addEventListener('click', () => {
    setOpen(advToggle, advanced, advanced.hidden);
    if (!advanced.hidden) ip.focus();
  });
  addrForm.addEventListener('submit', (e) => {
    e.preventDefault();
    if (ip.value.trim() && loginReady()) void actions.connect(ip.value, null, login());
  });
  matches.addEventListener('click', () => void store.get().pendingFingerprint?.retry());
  cancel.addEventListener('click', () => store.set({ pendingFingerprint: null }));

  const offs = [
    watch(store, (s) => `${cameraCount(s)}|${s.camera?.model ?? ''}`, (_, s) => {
      const count = cameraCount(s);
      back.hidden = count === 0;
      back.textContent = `Back to ${s.camera?.model ?? (count === 1 ? 'the camera' : 'the cameras')}`;
      quietword.textContent =
        count === 0 ? 'No camera connected' : `${count} ${count === 1 ? 'camera' : 'cameras'} connected`;
    }),
    // Progress while connecting, otherwise the last error.
    watch(store, (s) => s.connecting ?? s.error, (_, s) => {
      message.textContent = s.connecting?.text ?? s.error ?? '';
      message.classList.toggle('error', !s.connecting && !!s.error);
      applyBusy();
    }),
    watch(store, (s) => s.pendingFingerprint, (pending) => {
      confirm.hidden = pending === null;
      // A changed fingerprint has nothing to confirm: only Cancel is offered.
      matches.hidden = !!pending?.changed;
      confirmText.textContent = !pending
        ? ''
        : pending.changed
          ? fingerprintChangedText(pending.changed.expected, pending.changed.actual)
          : fingerprintText(pending.sha256, pending.md5);
    }),
    // A remembered Access Authentication camera: fill in what is known and
    // wait for the password.
    watch(store, (s) => s.sshPrefill, (prefill) => {
      if (!prefill) return;
      sshUser.value = prefill.user;
      sshPw.value = '';
      if (prefill.ssid) {
        ssid.value = prefill.ssid;
        pw.value = '';
      } else {
        ip.value = prefill.ip;
        setOpen(advToggle, advanced, true);
      }
      openLogin();
    }),
    watch(store, (s) => s.settings?.webcamId ?? null, () => {
      if (scannerStarted) restartScanner();
    }),
    watch(store, (s) => s.nearby, (list) => {
      if (list.length === 0) {
        nearbyList.replaceChildren(el('p', 'looking', 'Looking for cameras on this network'));
        return;
      }
      nearbyList.replaceChildren(
        ...list.map((cam) => {
          const d = el('div', 'device');
          const t = el('div', 't');
          t.append(el('b', '', cam.model || cam.friendly_name), el('span', '', cam.ip));
          const b = button('pbtn', 'Connect', `Connect to ${cam.model} at ${cam.ip}`);
          b.addEventListener('click', () => {
            // A camera that offers Access Authentication may need its
            // login: the first click opens it instead of connecting.
            if (cam.ssh_supported === true && ssh.hidden) {
              openLogin();
              return;
            }
            if (loginReady()) void actions.connect(cam.ip, null, login());
          });
          d.append(t, b);
          return d;
        }),
      );
      applyBusy();
    }),
    watch(store, (s) => s.settings?.recentCameras, (list = []) => {
      recent.hidden = list.length === 0;
      recentGroup.replaceChildren(
        ...list.map((rc) => {
          const r = rowButton(rc.model, rc.ssid ?? rc.ip, 'chev');
          const b = r.querySelector('button')!;
          b.prepend(icon('cam', 'camglyph', '0 0 20 20'));
          b.addEventListener('click', () => void actions.connectRecent(rc));
          return r;
        }),
      );
      applyBusy();
    }),
  ];

  if (webcamSupported()) startScanner();
  else scanStatus.textContent = webcamErrorMessage(new Error('unsupported'));
  scannerStarted = true;

  let timer = 0;
  const scan = async () => {
    const started = Date.now();
    try {
      const found = await discover(DISCOVER_MS);
      if (!stopped) store.set({ nearby: found });
    } catch {
      // Discovery is best effort; the other ways always work.
    }
    if (!stopped) timer = window.setTimeout(scan, Math.max(0, REFRESH_MS - (Date.now() - started)));
  };
  void scan();

  return () => {
    stopped = true;
    window.clearTimeout(timer);
    window.clearTimeout(rescanTimer);
    scanner.stop();
    offs.forEach((off) => off());
    parts.forEach((p) => p.remove());
  };
}
