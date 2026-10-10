// In-store barcodes for items with none on the pack. EAN-13 codes starting
// with 2 are reserved for a shop's own use, so they never clash with a
// manufacturer's code.

/** The EAN-13 check digit for the first 12 digits. */
export function ean13CheckDigit(first12: string): number {
  let sum = 0;
  for (let i = 0; i < 12; i++) sum += Number(first12[i]) * (i % 2 === 0 ? 1 : 3);
  return (10 - (sum % 10)) % 10;
}

export function isValidEan13(code: string): boolean {
  return /^\d{13}$/.test(code) && ean13CheckDigit(code.slice(0, 12)) === Number(code[12]);
}

/** A new in-store code: "20" + 10 random digits + check digit. */
export function inStoreBarcode(random: () => number = Math.random): string {
  let digits = '20';
  while (digits.length < 12) digits += String(Math.floor(random() * 10));
  return digits + ean13CheckDigit(digits);
}
