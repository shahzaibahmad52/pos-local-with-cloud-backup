// Same money maths as crates/pos-server/src/pricing.rs, so what the till shows
// is what the server charges. Amounts are integer paisa (Rs 1 = 100).

/** Division rounded half away from zero. */
export function roundDiv(n: number, d: number): number {
  if (d === 0) return 0;
  if (d < 0) {
    n = -n;
    d = -d;
  }
  return n >= 0 ? Math.floor((n + Math.floor(d / 2)) / d) : -Math.floor((-n + Math.floor(d / 2)) / d);
}

/** Price of `grams` of a weight item, rounded to `roundTo` paisa (100 = Rs 1). */
export function amountForGrams(grams: number, pricePerKg: number, roundTo = 100): number {
  const step = Math.max(1, roundTo);
  return roundDiv(grams * pricePerKg, 1000 * step) * step;
}

/** Grams to hand over when the customer asks for an amount. */
export function gramsForAmount(amount: number, pricePerKg: number): number {
  return roundDiv(amount * 1000, pricePerKg);
}

/** Tax already inside a tax-inclusive amount. */
export function includedTax(amount: number, rateBp: number): number {
  return rateBp <= 0 ? 0 : roundDiv(amount * rateBp, 10_000 + rateBp);
}

export function percentOf(amount: number, bp: number): number {
  return roundDiv(amount * bp, 10_000);
}
