// Brief 7.4: layout uses CSS logical properties only; no left/right anywhere
// in stylesheets or inline styles, so Hebrew and Arabic mirror without a
// separate stylesheet.

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const SRC = new URL('..', import.meta.url).pathname;

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) return files(p);
    return /\.(css|tsx)$/.test(f) ? [p] : [];
  });
}

const CSS_PHYSICAL = [
  /(^|[\s;{])(margin|padding|border)-(left|right)\b/,
  /(^|[\s;{])border-(top|bottom)-(left|right)-radius\b/,
  /(^|[\s;{])(left|right)\s*:/,
  /\btext-align\s*:\s*(left|right)\b/,
  /\bfloat\s*:\s*(left|right)\b/,
  /\bclear\s*:\s*(left|right)\b/,
  /\binset-(left|right)\b/,
  /\bto (left|right)\b/,
];

const JSX_PHYSICAL = [
  /\b(margin|padding|border)(Left|Right)\b/,
  /\bborder(Top|Bottom)(Left|Right)Radius\b/,
  /[{,]\s*(left|right)\s*:/,
  /\btextAlign\s*:\s*['"](left|right)['"]/,
  /\bfloat\s*:\s*['"](left|right)['"]/,
];

describe('no physical left/right in styles', () => {
  for (const f of files(SRC)) {
    const rel = f.slice(SRC.length);
    it(rel, () => {
      const text = readFileSync(f, 'utf8');
      const rules = f.endsWith('.css') ? CSS_PHYSICAL : JSX_PHYSICAL;
      const hits = text
        .split('\n')
        .map((line, i) => ({ line: line.trim(), n: i + 1 }))
        .filter(({ line }) => !line.startsWith('//') && !line.startsWith('*') && !line.startsWith('/*'))
        .filter(({ line }) => rules.some((r) => r.test(line)));
      expect(hits.map((h) => `${rel}:${h.n}: ${h.line}`)).toEqual([]);
    });
  }
});
