// Purchase line maths, the same as the store server's (crates/pos-server/src/purchases.rs),
// so the purchase screen shows the totals the server will save.

import { roundDiv } from './pricing';

export interface LineMoney {
  gross: number;
  discount: number;
  total: number;
}

/** qty packs × trade price, less the line discount (basis points). */
export function lineMoney(qty: number, tradePrice: number, discBp: number): LineMoney {
  const gross = qty * tradePrice;
  const discount = roundDiv(gross * discBp, 10_000);
  return { gross, discount, total: gross - discount };
}

/** Freight shared across lines by value; the shares add up to exactly `freight`. */
export function splitFreight(lineTotals: number[], freight: number): number[] {
  const shares = lineTotals.map(() => 0);
  if (lineTotals.length === 0 || freight === 0) return shares;
  const sum = lineTotals.reduce((a, b) => a + b, 0);
  if (sum === 0) {
    shares[0] = freight;
    return shares;
  }
  let given = 0;
  lineTotals.forEach((t, i) => {
    shares[i] = Math.trunc((freight * t) / sum);
    given += shares[i];
  });
  let biggest = 0;
  lineTotals.forEach((t, i) => {
    if (t > lineTotals[biggest]) biggest = i;
  });
  shares[biggest] += freight - given;
  return shares;
}

/**
 * Cost of one smallest unit once free (bonus) packs and freight are counted:
 * what was paid for the line divided by everything that arrived.
 */
export function costPerBase(line: LineMoney, freightShare: number, qty: number, bonus: number, packBaseQty: number): number {
  const units = (qty + bonus) * packBaseQty;
  return units > 0 ? roundDiv(line.total + freightShare, units) : 0;
}
