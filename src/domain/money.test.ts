import { describe, expect, it } from 'vitest';
import { formatAmount, formatGrams, formatRs, parseRupees } from './money';

describe('money', () => {
  it('formats paisa as rupees', () => {
    expect(formatRs(129_000)).toBe('Rs 1,290');
    expect(formatRs(160)).toBe('Rs 1.60');
    expect(formatAmount(-15_000)).toBe('-150');
  });

  it('parses what the cashier types', () => {
    expect(parseRupees('1,000')).toBe(100_000);
    expect(parseRupees('99.5')).toBe(9_950);
    expect(parseRupees('Rs 20')).toBe(2_000);
    expect(parseRupees('abc')).toBeNull();
    expect(parseRupees('')).toBeNull();
    expect(parseRupees('1.234')).toBeNull();
  });

  it('formats weights', () => {
    expect(formatGrams(625)).toBe('625 g');
    expect(formatGrams(1_000)).toBe('1 kg');
    expect(formatGrams(1_250)).toBe('1.25 kg');
  });
});
