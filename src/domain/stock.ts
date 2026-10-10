// Showing stock kept in the smallest unit (tablets, grams…) the way the
// shop counts it: "12 Strip + 4 Tab", "38.5 kg".

import { formatGrams } from './money';

export interface PackLevel {
  name: string;
  baseQty: number;
}

/** The pack the shop usually counts in: the smallest one bigger than a single unit. */
export function countingUnit<T extends PackLevel>(units: T[]): T | null {
  const packs = units.filter((u) => u.baseQty > 1).sort((a, b) => a.baseQty - b.baseQty);
  return packs[0] ?? null;
}

export function formatStock(stockBase: number, sellMode: 'unit' | 'weight', baseUnit: string, units: PackLevel[]): string {
  if (sellMode === 'weight') return formatGrams(stockBase);
  const pack = countingUnit(units);
  if (!pack || stockBase < pack.baseQty) return `${stockBase.toLocaleString('en-US')} ${baseUnit}`;
  const whole = Math.floor(stockBase / pack.baseQty);
  const loose = stockBase % pack.baseQty;
  const base = units.find((u) => u.baseQty === 1)?.name ?? baseUnit;
  return loose ? `${whole.toLocaleString('en-US')} ${pack.name} + ${loose} ${base}` : `${whole.toLocaleString('en-US')} ${pack.name}`;
}
