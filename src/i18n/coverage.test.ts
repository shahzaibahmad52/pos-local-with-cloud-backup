import { describe, expect, it } from 'vitest';
import { ApiError } from '../api/client';
import { boEn } from './backoffice';
import { describeError } from './errors';
import { fbrEn } from './fbr';
import { offEn } from './offline';
import { repEn } from './reports';
import { opsEn } from './stockops';
import { strings } from './strings';

describe('screen text', () => {
  it('no screen’s words are overwritten by another key of the same name', () => {
    for (const part of [boEn, repEn, opsEn, offEn, fbrEn] as Record<string, string>[]) {
      for (const [key, text] of Object.entries(part)) {
        expect(strings.en[key as keyof typeof strings.en], key).toBe(text);
      }
    }
  });
});

describe('server messages', () => {
  it('every reason the server gives has Urdu words', () => {
    const sources = import.meta.glob<string>('../../crates/pos-server/src/**/*.rs', { query: '?raw', import: 'default', eager: true });
    const reasons = new Set<string>();
    for (const text of Object.values(sources)) {
      for (const m of text.matchAll(/\.because\(\s*"([a-z_.]+)"/g)) reasons.add(m[1]);
    }
    expect(reasons.size).toBeGreaterThan(40);
    const missing = [...reasons].filter((r) => describeError(new ApiError(400, 'bad_request', 'ENGLISH', r, {}), 'ur') === 'ENGLISH');
    expect(missing).toEqual([]);
  });
});
