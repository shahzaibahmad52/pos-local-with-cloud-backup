import { describe, expect, it } from 'vitest';
import { amountForGrams, gramsForAmount, includedTax, percentOf, roundDiv } from './pricing';

// Same cases as the Rust tests in crates/pos-server/src/pricing.rs.
describe('pricing', () => {
  it('rounds half away from zero', () => {
    expect(roundDiv(5, 2)).toBe(3);
    expect(roundDiv(4, 2)).toBe(2);
    expect(roundDiv(-5, 2)).toBe(-3);
    expect(roundDiv(7, 0)).toBe(0);
  });

  it('prices sugar by amount and by weight', () => {
    expect(gramsForAmount(1_000, 16_000)).toBe(63);
    expect(gramsForAmount(10_000, 16_000)).toBe(625);
    expect(amountForGrams(250, 16_000, 100)).toBe(4_000);
    expect(amountForGrams(1_000, 16_000, 100)).toBe(16_000);
    expect(amountForGrams(10, 16_000, 100)).toBe(200);
    expect(amountForGrams(10, 16_000, 1)).toBe(160);
    expect(amountForGrams(333, 16_000, 500)).toBe(5_500);
  });

  it('works out included tax and discounts', () => {
    expect(includedTax(66_000, 1_800)).toBe(10_068);
    expect(includedTax(66_000, 0)).toBe(0);
    expect(percentOf(129_000, 500)).toBe(6_450);
  });
});
