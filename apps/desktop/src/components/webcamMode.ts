// Webcam mode: while it is on, the camera's live picture is served on this
// computer for other apps (through OBS and its virtual camera) and this
// panel takes the live view's place, saying so and how to pick it up.
//
// Rimix cannot add a camera to macOS by itself: only an app signed with an
// Apple Developer ID may. So the panel gives the address of the feed and
// the three steps that turn it into a camera every app can choose.

import type { Actions } from '../actions';
import { button, el } from '../dom';
import type { FeedQuality, FeedStatus } from '../ipc';
import { watch, type AppStore } from '../state';

/** How often the count of apps watching is read while the mode is on. */
const POLL_MS = 2000;
/** The feed's size at each quality, as the Rust side's `Quality::size`. */
const SCALE: Record<FeedQuality, [number, number]> = { Full: [1, 1], Medium: [3, 4], Low: [1, 2] };
const QUALITIES: FeedQuality[] = ['Full', 'Medium', 'Low'];

/** "768 × 432" for a quality, or "" until the camera's frame size is known. */
export function feedSize(feed: FeedStatus, quality: FeedQuality): string {
  if (!feed.width || !feed.height) return '';
  const [num, den] = SCALE[quality];
  const even = (n: number) => (quality === 'Full' ? n : Math.max(2, Math.floor((n * num) / den) & ~1));
  return `${even(feed.width)} × ${even(feed.height)}`;
}

export function watchingText(viewers: number): string {
  if (viewers === 0) return 'No app is showing the picture yet';
  return viewers === 1 ? '1 app is showing the picture' : `${viewers} apps are showing the picture`;
}

export function mount(root: HTMLElement, store: AppStore, actions: Actions): () => void {
  const panel = el('section', 'feedpanel glass');
  panel.setAttribute('aria-label', 'Webcam mode');
  panel.hidden = true;

  const title = el('h2');
  const lead = el(
    'p',
    'lead',
    "The live picture is going to other apps, so it is not shown here. Sound comes from this computer's microphone as usual.",
  );
  const watch_ = el('p', 'watch');
  watch_.setAttribute('role', 'status');

  const quality = el('div', 'quality segc');
  quality.setAttribute('role', 'radiogroup');
  quality.setAttribute('aria-label', 'Picture quality');
  const choices = QUALITIES.map((q) => {
    const b = button('');
    b.setAttribute('role', 'radio');
    b.append(el('b', '', q), el('small'));
    b.addEventListener('click', () => void actions.setWebcamQuality(q));
    quality.append(b);
    return { q, b };
  });

  const addr = el('div', 'addr');
  const url = el('code');
  const copy = button('pathbtn', 'Copy');
  copy.addEventListener('click', () => {
    void navigator.clipboard?.writeText(url.textContent ?? '').then(
      () => actions.toast('Address copied'),
      () => actions.toast('Could not copy the address'),
    );
  });
  addr.append(url, copy);

  const steps = el('ol', 'steps');
  for (const text of [
    'Install OBS (free, obsproject.com). This is needed once.',
    'In OBS, add a Browser source and paste the address above.',
    'Click Start Virtual Camera in OBS, then choose OBS Virtual Camera in Zoom, Meet or Teams.',
  ]) {
    steps.append(el('li', '', text));
  }

  const stop = button('stop pbtn', 'Stop using as a webcam');
  stop.addEventListener('click', () => void actions.setWebcamMode(false));

  panel.append(
    title,
    lead,
    watch_,
    el('h3', '', 'Quality'),
    quality,
    el('h3', '', 'Address of the picture'),
    addr,
    el('h3', '', 'To use it in a call'),
    steps,
    stop,
  );
  root.append(panel);

  let timer = 0;
  const render = () => {
    const { feed, camera } = store.get();
    const on = !!feed?.enabled;
    panel.hidden = !on;
    root.classList.toggle('webcam-on', on);
    window.clearTimeout(timer);
    if (!on || !feed) return;
    title.textContent = `${camera?.model || 'The camera'} is in use as a webcam`;
    watch_.textContent = watchingText(feed.viewers);
    url.textContent = feed.url ?? '';
    for (const { q, b } of choices) {
      const chosen = feed.quality === q;
      b.classList.toggle('on', chosen);
      b.setAttribute('aria-checked', String(chosen));
      b.querySelector('small')!.textContent = feedSize(feed, q);
    }
    timer = window.setTimeout(() => void actions.refreshFeed(), POLL_MS);
  };

  const offs = [watch(store, (s) => s.feed, render), watch(store, (s) => s.camera?.model, render)];
  void actions.refreshFeed();

  return () => {
    window.clearTimeout(timer);
    offs.forEach((off) => off());
    root.classList.remove('webcam-on');
    panel.remove();
  };
}
