import type { Product, SaleLineInput } from '../api/types';
import { amountForGrams, gramsForAmount, includedTax, percentOf, roundDiv } from './pricing';

export type UnitLine = {
  key: string;
  kind: 'unit';
  product: Product;
  unitId: string;
  qty: number;
};

/** Weight items are entered either as an amount ("Rs 10 of sugar") or as grams. */
export type WeightLine = {
  key: string;
  kind: 'weight';
  product: Product;
  mode: 'amount' | 'grams';
  /** Paisa when mode is amount, grams when mode is grams. */
  value: number;
};

export type CartLine = UnitLine | WeightLine;

export interface PricedLine {
  total: number;
  /** Base units taken from stock (tablets, pieces, grams…). */
  baseQty: number;
  /** Per pack, or per kg for weight lines. */
  unitPrice: number;
  grams?: number;
  tax: number;
}

export function priceLine(line: CartLine, weightRoundTo = 100): PricedLine {
  if (line.kind === 'weight') {
    const perKg = line.product.pricePerKg ?? 0;
    const grams = line.mode === 'grams' ? line.value : perKg > 0 ? gramsForAmount(line.value, perKg) : 0;
    const total = line.mode === 'amount' ? line.value : amountForGrams(line.value, perKg, weightRoundTo);
    return { total, baseQty: grams, unitPrice: perKg, grams, tax: includedTax(total, line.product.taxRateBp) };
  }
  const unit = line.product.units.find((u) => u.id === line.unitId);
  const price = unit?.price ?? 0;
  const total = price * line.qty;
  return {
    total,
    baseQty: (unit?.baseQty ?? 0) * line.qty,
    unitPrice: price,
    tax: includedTax(total, line.product.taxRateBp),
  };
}

export interface Totals {
  subtotal: number;
  discount: number;
  total: number;
  tax: number;
  itemCount: number;
}

export function cartTotals(lines: CartLine[], discountBp: number, weightRoundTo = 100): Totals {
  let subtotal = 0;
  let lineTax = 0;
  let itemCount = 0;
  for (const line of lines) {
    const p = priceLine(line, weightRoundTo);
    subtotal += p.total;
    lineTax += p.tax;
    itemCount += line.kind === 'unit' ? line.qty : 1;
  }
  const discount = percentOf(subtotal, discountBp);
  const total = subtotal - discount;
  const tax = subtotal > 0 ? roundDiv(lineTax * total, subtotal) : 0;
  return { subtotal, discount, total, tax, itemCount };
}

let counter = 0;
const nextKey = () => `l${Date.now().toString(36)}${(counter++).toString(36)}`;

/**
 * Adds a product to the bill. Scanning the same pack again adds one more;
 * a weight item already on the bill is left for the cashier to adjust.
 */
export function addToCart(lines: CartLine[], product: Product, unitId?: string | null): CartLine[] {
  if (product.sellMode === 'weight') {
    if (lines.some((l) => l.product.id === product.id)) return lines;
    return [...lines, { key: nextKey(), kind: 'weight', product, mode: 'grams', value: 1000 }];
  }
  const unit =
    product.units.find((u) => u.id === unitId) ??
    // Default to the strip-sized pack: the most common way medicine is sold.
    product.units.find((u) => u.baseQty > 1 && u.baseQty <= 30) ??
    product.units[0];
  if (!unit) return lines;
  const existing = lines.find((l) => l.kind === 'unit' && l.product.id === product.id && l.unitId === unit.id);
  if (existing) {
    return lines.map((l) => (l === existing && l.kind === 'unit' ? { ...l, qty: l.qty + 1 } : l));
  }
  return [...lines, { key: nextKey(), kind: 'unit', product, unitId: unit.id, qty: 1 }];
}

/** Base units of this product already on the bill, across all its lines. */
export function baseQtyOnBill(lines: CartLine[], productId: string): number {
  return lines.filter((l) => l.product.id === productId).reduce((sum, l) => sum + priceLine(l).baseQty, 0);
}

export function toSaleLines(lines: CartLine[]): SaleLineInput[] {
  return lines.map((l) =>
    l.kind === 'unit'
      ? { productId: l.product.id, unitId: l.unitId, qty: l.qty }
      : l.mode === 'grams'
        ? { productId: l.product.id, grams: l.value }
        : { productId: l.product.id, amount: l.value },
  );
}
