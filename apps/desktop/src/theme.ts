// Puts the chosen appearance and layout on <html> as `data-theme` and
// `data-layout`, which the stylesheets key on. "System" resolves to dark or
// light from the OS and follows it while it stays chosen; settings keep the
// choice, never the resolved value.

import type { Appearance, Layout } from './ipc';

export type MatchMedia = (query: string) => MediaQueryList;

const DARK_QUERY = '(prefers-color-scheme: dark)';

const systemMedia: MatchMedia | undefined =
  typeof window !== 'undefined' && typeof window.matchMedia === 'function'
    ? (q) => window.matchMedia(q)
    : undefined;

/** Stops following the OS theme; set while "System" is applied. */
let stopFollowing: (() => void) | null = null;

export function applyAppearance(
  choice: Appearance,
  doc: Document = document,
  matchMedia: MatchMedia | undefined = systemMedia,
): void {
  stopFollowing?.();
  stopFollowing = null;
  const root = doc.documentElement;
  if (choice !== 'System') {
    root.setAttribute('data-theme', choice === 'Light' ? 'light' : 'dark');
    return;
  }
  const mq = matchMedia?.(DARK_QUERY);
  const sync = () => root.setAttribute('data-theme', mq && !mq.matches ? 'light' : 'dark');
  sync();
  if (!mq) return;
  mq.addEventListener('change', sync);
  stopFollowing = () => mq.removeEventListener('change', sync);
}

export function applyLayout(choice: Layout, doc: Document = document): void {
  doc.documentElement.setAttribute('data-layout', choice === 'Ring' ? 'ring' : 'dock');
}
