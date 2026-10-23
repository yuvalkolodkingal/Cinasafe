import { describe, expect, it } from 'vitest';

import ar from '../i18n/ar.json';
import en from '../i18n/en.json';
import he from '../i18n/he.json';

const PLURAL = /_(zero|one|two|few|many|other)$/;

/** Every leaf key, with plural suffixes folded (languages need different plural forms). */
function keys(obj: Record<string, unknown>, prefix = ''): Set<string> {
  const out = new Set<string>();
  for (const [k, v] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${k}` : k;
    if (v && typeof v === 'object') keys(v as Record<string, unknown>, path).forEach((x) => out.add(x));
    else out.add(path.replace(PLURAL, '_plural'));
  }
  return out;
}

function leaves(obj: Record<string, unknown>, prefix = ''): [string, string][] {
  return Object.entries(obj).flatMap(([k, v]) =>
    v && typeof v === 'object' ? leaves(v as Record<string, unknown>, prefix ? `${prefix}.${k}` : k) : [[prefix ? `${prefix}.${k}` : k, String(v)] as [string, string]],
  );
}

const vars = (s: string) => [...s.matchAll(/\{\{(\w+)\}\}/g)].map((m) => m[1]).sort();

describe('translations', () => {
  const base = keys(en);
  for (const [name, dict] of Object.entries({ he, ar })) {
    it(`${name} has exactly the English keys`, () => {
      const k = keys(dict);
      expect([...base].filter((x) => !k.has(x)), 'missing').toEqual([]);
      expect([...k].filter((x) => !base.has(x)), 'extra').toEqual([]);
    });

    it(`${name} uses the same {{variables}}`, () => {
      const enLeaves = new Map(leaves(en).map(([k, v]) => [k.replace(PLURAL, ''), vars(v)]));
      for (const [k, v] of leaves(dict)) {
        const want = enLeaves.get(k.replace(PLURAL, ''));
        // Plural forms may spell the count out ("שני סרטים") instead of using {{count}}.
        const got = vars(v).filter((x) => x !== 'count');
        expect(got, k).toEqual((want ?? []).filter((x) => x !== 'count'));
      }
    });

    it(`${name} has no empty strings`, () => {
      expect(leaves(dict).filter(([, v]) => !v.trim())).toEqual([]);
    });
  }

  it('English plural keys have _one and _other', () => {
    const all = leaves(en).map(([k]) => k);
    for (const k of all.filter((x) => x.endsWith('_one'))) expect(all).toContain(k.replace(/_one$/, '_other'));
  });

  it('Arabic plural keys cover zero, one, two, few, many and other', () => {
    const all = leaves(ar).map(([k]) => k);
    const stems = new Set(all.filter((k) => PLURAL.test(k)).map((k) => k.replace(PLURAL, '')));
    for (const s of stems) for (const f of ['zero', 'one', 'two', 'few', 'many', 'other']) expect(all).toContain(`${s}_${f}`);
  });

  it('error codes from the contract all have messages', async () => {
    const src = await import('node:fs').then((fs) => fs.readFileSync(new URL('../api/types.ts', import.meta.url), 'utf8'));
    const block = src.slice(src.indexOf('export type ErrorCode'), src.indexOf(';', src.indexOf('export type ErrorCode')));
    const codes = [...block.matchAll(/'(\w+)'/g)].map((m) => m[1]);
    expect(codes.length).toBeGreaterThan(10);
    for (const c of codes) expect(Object.keys(en.errors), c).toContain(c);
  });
});
