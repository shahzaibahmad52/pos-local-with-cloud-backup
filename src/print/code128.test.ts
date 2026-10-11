import { describe, expect, it } from 'vitest';
import { code128Modules, code128Values } from './code128';

describe('Code 128', () => {
  it('adds the start, checksum and stop symbols', () => {
    // "A" in set B: start 104, A = 33, checksum (104 + 33) % 103 = 34, stop.
    expect(code128Values('A')).toEqual([104, 33, 34, 106]);
  });

  it('draws 11 modules a symbol and 13 for the stop, starting and ending with a bar', () => {
    const m = code128Modules('B1-T01-000482')!;
    expect(m.length).toBe((1 + 13 + 1) * 11 + 13);
    expect(m[0]).toBe(true);
    expect(m[m.length - 1]).toBe(true);
  });

  it('refuses characters it cannot print', () => {
    expect(code128Values('رسید')).toBeNull();
  });
});
