// Parses the Wi-Fi credentials a Sony camera encodes in its setup QR code.
// Grammar: optional `WIFI:` or `W01:` prefix, then `KEY:VALUE;` fields.

export interface WifiCredentials {
  ssid: string;
  password: string;
  model?: string;
}

const PREFIX = /^(?:WIFI|W01):/i;

/** Reads one field value starting at `start`, up to the next unescaped `;`. */
function readValue(text: string, start: number): { value: string; next: number } | null {
  let value = '';
  let i = start;
  while (i < text.length) {
    const ch = text[i];
    if (ch === '\\') {
      if (i + 1 >= text.length) return null; // dangling escape
      value += text[i + 1];
      i += 2;
    } else if (ch === ';') {
      return { value, next: i + 1 };
    } else {
      value += ch;
      i += 1;
    }
  }
  return { value, next: i }; // end of text closes the last field
}

export function parseWifiQr(text: string): WifiCredentials | null {
  const body = text.trim().replace(PREFIX, '');
  const fields = new Map<string, string>();
  let i = 0;
  while (i < body.length) {
    if (body[i] === ';') {
      i += 1; // empty field, e.g. the closing ";;"
      continue;
    }
    const colon = body.indexOf(':', i);
    if (colon < 0) return null;
    const key = body.slice(i, colon).trim().toUpperCase();
    if (key === '' || key.includes(';')) return null;
    const parsed = readValue(body, colon + 1);
    if (!parsed) return null;
    fields.set(key, parsed.value);
    i = parsed.next;
  }

  const ssid = fields.get('S');
  if (!ssid) return null;
  const result: WifiCredentials = { ssid, password: fields.get('P') ?? '' };
  const model = fields.get('C');
  if (model) result.model = model;
  return result;
}
