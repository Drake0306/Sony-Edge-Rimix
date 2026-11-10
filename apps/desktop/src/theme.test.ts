import { afterEach, describe, expect, test, vi } from 'vitest';
import { applyAppearance, applyLayout } from './theme';

/** A stand-in for `window.matchMedia` whose `change` listeners the test can fire. */
function fakeMedia(dark: boolean) {
  const listeners = new Set<() => void>();
  const mq = {
    matches: dark,
    addEventListener: vi.fn((_: string, fn: () => void) => listeners.add(fn)),
    removeEventListener: vi.fn((_: string, fn: () => void) => listeners.delete(fn)),
  };
  const matchMedia = vi.fn(() => mq as unknown as MediaQueryList);
  const flip = (next: boolean) => {
    mq.matches = next;
    listeners.forEach((fn) => fn());
  };
  return { matchMedia, mq, flip, listeners };
}

const html = () => document.documentElement;

describe('theme', () => {
  afterEach(() => applyAppearance('Dark', document, fakeMedia(true).matchMedia));

  test('applying_light_sets_html_data_theme', () => {
    applyAppearance('Light');
    expect(html().getAttribute('data-theme')).toBe('light');
    applyAppearance('Dark');
    expect(html().getAttribute('data-theme')).toBe('dark');
  });

  test('system_follows_media_query', () => {
    const media = fakeMedia(false);
    applyAppearance('System', document, media.matchMedia);
    expect(media.matchMedia).toHaveBeenCalledWith('(prefers-color-scheme: dark)');
    expect(html().getAttribute('data-theme')).toBe('light');
    media.flip(true);
    expect(html().getAttribute('data-theme')).toBe('dark');
    media.flip(false);
    expect(html().getAttribute('data-theme')).toBe('light');
    // Choosing a fixed theme stops following the system.
    applyAppearance('Dark', document, media.matchMedia);
    expect(media.listeners.size).toBe(0);
    media.flip(false);
    expect(html().getAttribute('data-theme')).toBe('dark');
  });

  test('system falls back to dark without matchMedia', () => {
    applyAppearance('System', document, undefined);
    expect(html().getAttribute('data-theme')).toBe('dark');
  });

  test('layout_sets_html_data_layout', () => {
    applyLayout('Ring');
    expect(html().getAttribute('data-layout')).toBe('ring');
    applyLayout('Dock');
    expect(html().getAttribute('data-layout')).toBe('dock');
  });
});
