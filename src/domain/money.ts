/** "Rs 1,290" or "Rs 1.60" from paisa. */
export function formatRs(paisa: number): string {
  return `Rs ${formatAmount(paisa)}`;
}

/** "1,290" or "1.60" from paisa, without the currency. */
export function formatAmount(paisa: number): string {
  // Sign after rounding, so a tiny negative shows as 0, not -0.
  const negative = Math.round(paisa) < 0;
  const abs = Math.abs(Math.round(paisa));
  const rupees = Math.floor(abs / 100);
  const rest = abs % 100;
  const grouped = rupees.toLocaleString('en-US');
  const text = rest === 0 ? grouped : `${grouped}.${String(rest).padStart(2, '0')}`;
  return negative ? `-${text}` : text;
}

/** Parses what a cashier typed ("1,000", "99.5") into paisa; null if it isn't a number. */
export function parseRupees(input: string): number | null {
  const text = input
    // Urdu (۰–۹) and Arabic (٠–٩) digits, and the Urdu decimal point.
    .replace(/[\u06F0-\u06F9]/g, (d) => String(d.charCodeAt(0) - 0x06f0))
    .replace(/[\u0660-\u0669]/g, (d) => String(d.charCodeAt(0) - 0x0660))
    .replace(/\u066B/g, '.')
    .replace(/\s/g, '')
    .replace(/^rs\.?/i, '');
  // Commas only between groups of 2 or 3 digits: 1,000 and 1,00,000, not 1,2,3.
  if (!/^\d+(,\d{2,3})*(\.\d{1,2})?$/.test(text)) return null;
  const [whole, frac = ''] = text.replace(/,/g, '').split('.');
  const paisa = Number(whole) * 100 + Number(frac.padEnd(2, '0'));
  // Rs 10 crore is the most the store server accepts.
  return paisa <= 10_000_000_000 ? paisa : null;
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
