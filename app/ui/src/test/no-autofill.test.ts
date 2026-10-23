// Brief 10: no spell-check dictionaries and no form autofill. Every text-like
// <input> and every <textarea> must say spellCheck={false} and
// autoComplete="off" (the webview could otherwise keep what was typed).

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const SRC = new URL('..', import.meta.url).pathname;

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) return files(p);
    return f.endsWith('.tsx') ? [p] : [];
  });
}

/** The JSX opening tags of `<input` and `<textarea`, braces respected (arrow functions contain `>`). */
export function openingTags(text: string): { tag: string; line: number }[] {
  const out: { tag: string; line: number }[] = [];
  const re = /<(input|textarea)\b/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    let depth = 0;
    let i = m.index + m[0].length;
    for (; i < text.length; i++) {
      const c = text[i];
      if (c === '{') depth++;
      else if (c === '}') depth--;
      else if (c === '>' && depth === 0) break;
    }
    out.push({ tag: text.slice(m.index, i + 1), line: text.slice(0, m.index).split('\n').length });
  }
  return out;
}

const NON_TEXT = /\btype="(checkbox|radio|range|button|submit|file|color|hidden)"/;

describe('no spell-check or autofill on text fields', () => {
  it('finds tags whose attributes contain arrow functions', () => {
    const tags = openingTags('<input value={x} onChange={(e) => f(e)} spellCheck={false} />');
    expect(tags).toHaveLength(1);
    expect(tags[0].tag).toContain('spellCheck');
  });

  for (const f of files(SRC)) {
    const rel = f.slice(SRC.length);
    const tags = openingTags(readFileSync(f, 'utf8')).filter((t) => !NON_TEXT.test(t.tag));
    if (tags.length === 0) continue;
    it(rel, () => {
      const bad = tags
        .filter((t) => !t.tag.includes('spellCheck={false}') || !t.tag.includes('autoComplete="off"'))
        .map((t) => `${rel}:${t.line}`);
      expect(bad).toEqual([]);
    });
  }
});
