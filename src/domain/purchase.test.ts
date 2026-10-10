import { describe, expect, it } from 'vitest';
import { costPerBase, lineMoney, splitFreight } from './purchase';

describe('purchase maths (same cases as the server)', () => {
  it('takes the line discount off', () => {
    expect(lineMoney(5, 50_000, 200)).toEqual({ gross: 250_000, discount: 5_000, total: 245_000 });
  });

  it('splits freight so it adds up', () => {
    const shares = splitFreight([245_000, 704_000, 1_050_000, 139_680], 30_000);
    expect(shares.reduce((a, b) => a + b, 0)).toBe(30_000);
    expect(shares[2]).toBeGreaterThan(shares[0]);
    expect(splitFreight([0, 0], 500)).toEqual([500, 0]);
    expect(splitFreight([100], 0)).toEqual([0]);
  });

  it('spreads cost over bonus units', () => {
    // 5 boxes of 200 + 1 free, Rs 2,450 + Rs 300 freight.
    expect(costPerBase(lineMoney(5, 50_000, 200), 30_000, 5, 1, 200)).toBe(229);
  });
});
