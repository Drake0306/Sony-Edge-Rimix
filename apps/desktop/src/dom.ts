// Small DOM helpers. Text always goes in through textContent.

export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className = '',
  text?: string,
): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (className) e.className = className;
  if (text !== undefined) e.textContent = text;
  return e;
}

const SVG = 'http://www.w3.org/2000/svg';

/** An `<svg>` showing a symbol from the sprite in index.html. */
export function icon(id: string, className = '', viewBox?: string): SVGSVGElement {
  const svg = document.createElementNS(SVG, 'svg');
  if (className) svg.setAttribute('class', className);
  if (viewBox) svg.setAttribute('viewBox', viewBox);
  svg.setAttribute('aria-hidden', 'true');
  const use = document.createElementNS(SVG, 'use');
  use.setAttribute('href', `#${id}`);
  svg.append(use);
  return svg;
}

export function button(className: string, text?: string, label?: string): HTMLButtonElement {
  const b = el('button', className, text);
  b.type = 'button';
  if (label) b.setAttribute('aria-label', label);
  return b;
}

/** Elements that take typed text, where single-key shortcuts must not fire. */
export const isTyping = (t: EventTarget | null): boolean =>
  t instanceof HTMLElement && (t.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName));
