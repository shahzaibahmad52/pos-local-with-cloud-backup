/** "Rs 1,290" or "Rs 1.60" from paisa. */
export function formatRs(paisa: number): string {
  return `Rs ${formatAmount(paisa)}`;
}

/** "1,290" or "1.60" from paisa, without the currency. */
export function formatAmount(paisa: number): string {
  const negative = paisa < 0;
  const abs = Math.abs(Math.round(paisa));
  const rupees = Math.floor(abs / 100);
  const rest = abs % 100;
  const grouped = rupees.toLocaleString('en-US');
  const text = rest === 0 ? grouped : `${grouped}.${String(rest).padStart(2, '0')}`;
  return negative ? `-${text}` : text;
}

/** Parses what a cashier typed ("1,000", "99.5") into paisa; null if it isn't a number. */
export function parseRupees(input: string): number | null {
  const cleaned = input.replace(/[,\s]/g, '').replace(/^rs\.?/i, '');
  if (cleaned === '' || !/^\d+(\.\d{1,2})?$/.test(cleaned)) return null;
  const [whole, frac = ''] = cleaned.split('.');
  return Number(whole) * 100 + Number(frac.padEnd(2, '0'));
}

/** "625 g" or "1.25 kg". */
export function formatGrams(grams: number): string {
  if (grams >= 1000 && grams % 10 === 0) {
    const kg = grams / 1000;
    return `${kg.toLocaleString('en-US', { maximumFractionDigits: 2 })} kg`;
  }
  return `${grams.toLocaleString('en-US')} g`;
}

/** Keeps a left-to-right value (number, code, "24 btl") in order inside Urdu text. */
export function ltr(text: string): string {
  return `\u2066${text}\u2069`;
}
