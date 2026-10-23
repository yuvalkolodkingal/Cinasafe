import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

import { mockCommands } from '../api/mock';

describe('mock backend', () => {
  it('implements every command the UI can call', () => {
    const src = readFileSync(new URL('../api/index.ts', import.meta.url), 'utf8');
    const used = [...src.matchAll(/call<[^>]*>\('(\w+)'/g)].map((m) => m[1]);
    expect(used.length).toBeGreaterThan(30);
    expect(used.filter((c) => !mockCommands.includes(c))).toEqual([]);
  });
});
