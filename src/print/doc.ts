// Printable documents (sale receipt, return slip, Z-report) as a list of
// simple blocks. The same blocks are shown on screen (DocView) and drawn
// into the 1-bit image sent to the thermal printer (raster.ts), so the
// preview and the paper always match.

import type { KhataPaymentSlip, KhataStatement, Receipt, ReceiptLanguage, ReturnSlip, ShiftSummary, StoreHeader, TransferView } from '../api/types';
import { formatAmount, formatGrams } from '../domain/money';

export type Block =
  | { kind: 'title'; text: string }
  | { kind: 'center'; text: string; bold?: boolean; small?: boolean; ltr?: boolean }
  | { kind: 'rule'; heavy?: boolean }
  | { kind: 'row'; label: string; value: string; bold?: boolean; big?: boolean }
  /** Full-width text; product names are always left-to-right. */
  | { kind: 'text'; text: string; bold?: boolean; ltr?: boolean }
  /** A QR code, centred (FBR's invoice number). */
  | { kind: 'qr'; text: string };

export interface Doc {
  dir: 'ltr' | 'rtl';
  /** True when the text has Urdu in it (needs taller lines). */
  urdu: boolean;
  blocks: Block[];
}

const L = {
  receiptNo: ['Receipt #', 'رسید نمبر'],
  fbrInvoice: ['FBR invoice #', 'FBR انوائس نمبر'],
  fbrVerify: ['Verify with FBR’s Tax Asaan app', 'FBR کی ٹیکس آسان ایپ سے تصدیق کریں'],
  returnNo: ['Return #', 'واپسی نمبر'],
  originalReceipt: ['Original receipt', 'اصل رسید'],
  date: ['Date', 'تاریخ'],
  cashier: ['Cashier', 'کیشیئر'],
  customer: ['Customer', 'گاہک'],
  walkIn: ['Walk-in', 'عام گاہک'],
  item: ['ITEM', 'آئٹم'],
  amount: ['AMOUNT', 'رقم'],
  items: ['Items', 'کل اشیاء'],
  subtotal: ['Subtotal', 'ذیلی کل'],
  discount: ['Discount', 'رعایت'],
  tax: ['* Sales tax (incl.)', '* سیلز ٹیکس (شامل)'],
  total: ['TOTAL Rs', 'کل رقم (روپے)'],
  change: ['Change', 'واپسی'],
  khataBalance: ['Khata balance', 'کھاتہ بقایا'],
  cash: ['Paid — Cash', 'ادا شدہ — نقد'],
  card: ['Paid — Card', 'ادا شدہ — کارڈ'],
  jazzcash: ['Paid — JazzCash', 'ادا شدہ — JazzCash'],
  easypaisa: ['Paid — Easypaisa', 'ادا شدہ — Easypaisa'],
  khata: ['On khata (udhaar)', 'کھاتے میں (اُدھار)'],
  policy: [
    'Medicines returnable within 7 days with this receipt (unopened strips only). Fridge items & injections non-returnable.',
    'دوائیں 7 دن کے اندر اس رسید کے ساتھ واپس ہو سکتی ہیں (صرف بند پتے)۔ فریج والی اشیاء اور انجیکشن واپس نہیں ہوں گے۔',
  ],
  thanks: ['Thank you! Get well soon.', 'شکریہ! اللہ آپ کو صحت دے۔'],
  perKg: ['/ kg', '/ کلو'],
  returnTitle: ['RETURN / REFUND', 'واپسی / ریفنڈ'],
  refund: ['REFUND Rs', 'ریفنڈ (روپے)'],
  refundedAs: ['Refunded as', 'ریفنڈ بذریعہ'],
  approvedBy: ['Approved by', 'اجازت'],
  restock: ['back to stock', 'اسٹاک میں واپس'],
  damaged: ['damaged', 'خراب'],
  expired: ['expired', 'میعاد ختم'],
  refundCash: ['Cash', 'نقد'],
  refundKhata: ['Khata credit', 'کھاتے میں'],
  refundCard: ['Card', 'کارڈ'],
  zTitle: ['SHIFT CLOSE (Z-REPORT)', 'شفٹ بندش (Z رپورٹ)'],
  till: ['Till', 'ٹِل'],
  opened: ['Opened', 'شروع'],
  closed: ['Closed', 'بند'],
  bills: ['Bills', 'بل'],
  sales: ['Sales', 'سیل'],
  returns: ['Returns', 'واپسیاں'],
  openingCash: ['Opening cash', 'ابتدائی نقد'],
  cashSales: ['+ Cash sales', '+ نقد سیل'],
  cashIn: ['+ Cash in', '+ نقد اندر'],
  cashOut: ['− Cash out', '− نقد باہر'],
  cashRefunds: ['− Cash refunds', '− نقد ریفنڈ'],
  expectedCash: ['Expected in drawer', 'دراز میں ہونا چاہیے'],
  counted: ['Counted', 'گنا گیا'],
  difference: ['Difference', 'فرق'],
  notInDrawer: ['Not in drawer', 'دراز میں نہیں'],
  note: ['Note', 'نوٹ'],
  khataPayment: ['KHATA PAYMENT', 'کھاتہ ادائیگی'],
  statement: ['KHATA STATEMENT', 'کھاتہ اسٹیٹمنٹ'],
  received: ['RECEIVED Rs', 'وصول (روپے)'],
  balanceBefore: ['Balance before', 'پہلے بقایا'],
  balanceNow: ['Balance now', 'اب بقایا'],
  receivedBy: ['Received by', 'وصول کنندہ'],
  paidBy: ['Paid by', 'ادائیگی بذریعہ'],
  bank: ['Bank', 'بینک'],
  bill: ['Bill', 'بل'],
  returnLine: ['Return', 'واپسی'],
  oldBalance: ['Old balance', 'پرانا بقایا'],
  payment: ['Payment', 'ادائیگی'],
  balanceDue: ['BALANCE DUE Rs', 'واجب الادا (روپے)'],
  creditLimit: ['Credit limit', 'اُدھار کی حد'],
  printed: ['Printed', 'پرنٹ'],
  recoveryTitle: ['OWNER RECOVERY CODE', 'مالک کا ریکوری کوڈ'],
  transferTitle: ['STOCK TRANSFER', 'اسٹاک منتقلی'],
  transferTo: ['To branch', 'برانچ کو'],
  transferFrom: ['From branch', 'برانچ سے'],
  sentQty: ['Sent', 'بھیجا'],
  receivedQty: ['Received', 'وصول'],
  carriedBy: ['Carried by', 'لے جانے والا'],
  signature: ['Received by (sign)', 'وصول کنندہ (دستخط)'],
  recoveryHelp: [
    'Keep this safe. On the main PC, choose the owner on the sign-in screen, then "Forgot password?". Works once.',
    'اسے محفوظ رکھیں۔ مین PC پر سائن اِن اسکرین پر مالک کو چنیں، پھر "پاس ورڈ بھول گئے؟"۔ ایک بار چلتا ہے۔',
  ],
} as const;

type Key = keyof typeof L;

function make(lang: ReceiptLanguage) {
  return (key: Key) => {
    const [en, ur] = L[key];
    return lang === 'ur' ? ur : lang === 'both' ? `${en} · ${ur}` : en;
  };
}

function docDir(lang: ReceiptLanguage): Pick<Doc, 'dir' | 'urdu'> {
  return { dir: lang === 'ur' ? 'rtl' : 'ltr', urdu: lang !== 'en' };
}

export function formatDateTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const p = (n: number) => String(n).padStart(2, '0');
  return `${p(d.getDate())}-${p(d.getMonth() + 1)}-${d.getFullYear()} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

function storeHeader(store: StoreHeader): Block[] {
  const out: Block[] = [{ kind: 'title', text: store.name }];
  if (store.address) out.push({ kind: 'center', text: store.address });
  if (store.phone) out.push({ kind: 'center', text: `Ph: ${store.phone}`, ltr: true });
  const ids = [store.ntn && `NTN: ${store.ntn}`, store.drugLicense && `Drug Lic: ${store.drugLicense}`].filter(Boolean);
  if (ids.length) out.push({ kind: 'center', text: ids.join(' · '), ltr: true });
  out.push({ kind: 'rule' });
  return out;
}

/** The store's own closing line in the receipt's language, or the default thanks. */
function footer(store: StoreHeader, lang: ReceiptLanguage): string {
  const en = store.receiptFooter ?? L.thanks[0];
  const ur = store.receiptFooterUr ?? L.thanks[1];
  return lang === 'ur' ? ur : lang === 'both' ? `${en}\n${ur}` : en;
}

const paidKey: Record<string, Key> = { cash: 'cash', card: 'card', jazzcash: 'jazzcash', easypaisa: 'easypaisa', khata: 'khata' };

/** FBR's invoice number and its QR code, once FBR has accepted the invoice. */
function fbrBlocks(no: string | null | undefined, t: (k: Key) => string): Block[] {
  if (!no) return [];
  return [
    { kind: 'rule' },
    { kind: 'row', label: t('fbrInvoice'), value: no, bold: true },
    { kind: 'qr', text: no },
    { kind: 'center', text: t('fbrVerify'), small: true },
  ];
}

export function saleDoc(r: Receipt, lang: ReceiptLanguage): Doc {
  const t = make(lang);
  const b: Block[] = [
    ...storeHeader(r.store),
    { kind: 'row', label: t('receiptNo'), value: r.receiptNo, bold: true },
    { kind: 'row', label: t('date'), value: formatDateTime(r.createdAt) },
    { kind: 'row', label: t('cashier'), value: `${r.cashierName} · ${r.tillCode}` },
    { kind: 'row', label: t('customer'), value: r.customerName ?? t('walkIn') },
    { kind: 'rule' },
    { kind: 'row', label: t('item'), value: t('amount'), bold: true },
  ];
  for (const line of r.lines) {
    b.push({ kind: 'text', text: `${line.productName}${line.taxIncluded > 0 ? '  *' : ''}`, bold: true, ltr: true });
    const detail =
      line.sellMode === 'weight'
        ? `${formatGrams(line.qty)} @ ${formatAmount(line.unitPrice)} ${t('perKg')}`
        : `${line.qty} ${line.unitName} × ${formatAmount(line.unitPrice)}${line.batchNo && line.batchNo !== '-' ? `  B:${line.batchNo}` : ''}`;
    b.push({ kind: 'row', label: detail, value: formatAmount(line.lineTotal) });
  }
  b.push({ kind: 'rule' });
  b.push({ kind: 'row', label: t('items'), value: String(r.itemCount) });
  b.push({ kind: 'row', label: t('subtotal'), value: formatAmount(r.subtotal) });
  if (r.discount > 0) b.push({ kind: 'row', label: t('discount'), value: `-${formatAmount(r.discount)}` });
  if (r.taxIncluded > 0) b.push({ kind: 'row', label: t('tax'), value: formatAmount(r.taxIncluded) });
  b.push({ kind: 'rule', heavy: true });
  b.push({ kind: 'row', label: t('total'), value: formatAmount(r.total), bold: true, big: true });
  b.push({ kind: 'rule', heavy: true });
  for (const p of r.payments) {
    // Cash is stored as what stayed in the drawer; the receipt shows what was handed over.
    const shown = p.method === 'cash' ? p.amount + r.changeDue : p.amount;
    b.push({ kind: 'row', label: t(paidKey[p.method] ?? 'cash'), value: formatAmount(shown) });
  }
  if (r.changeDue > 0) b.push({ kind: 'row', label: t('change'), value: formatAmount(r.changeDue), bold: true });
  if (r.khataBalance !== null) b.push({ kind: 'row', label: t('khataBalance'), value: formatAmount(r.khataBalance) });
  b.push(...fbrBlocks(r.fbrInvoiceNo, t));
  b.push({ kind: 'rule' });
  b.push({ kind: 'center', text: lang === 'both' ? `${L.policy[0]}\n${L.policy[1]}` : t('policy'), small: true });
  b.push({ kind: 'center', text: footer(r.store, lang), bold: true });
  b.push({ kind: 'center', text: r.receiptNo, small: true, ltr: true });
  return { ...docDir(lang), blocks: b };
}

const conditionKey: Record<string, Key> = { restock: 'restock', damaged: 'damaged', expired: 'expired' };
const refundKey: Record<string, Key> = { cash: 'refundCash', khata: 'refundKhata', card: 'refundCard', jazzcash: 'jazzcash', easypaisa: 'easypaisa' };

export function returnDoc(r: ReturnSlip, lang: ReceiptLanguage): Doc {
  const t = make(lang);
  const b: Block[] = [
    ...storeHeader(r.store),
    { kind: 'center', text: t('returnTitle'), bold: true },
    { kind: 'row', label: t('returnNo'), value: r.returnNo, bold: true },
    { kind: 'row', label: t('originalReceipt'), value: r.saleReceiptNo },
    { kind: 'row', label: t('date'), value: formatDateTime(r.createdAt) },
    { kind: 'row', label: t('cashier'), value: `${r.cashierName} · ${r.tillCode}` },
    { kind: 'row', label: t('customer'), value: r.customerName ?? t('walkIn') },
    { kind: 'rule' },
  ];
  for (const line of r.lines) {
    b.push({ kind: 'text', text: line.productName, bold: true, ltr: true });
    const qty = line.sellMode === 'weight' ? formatGrams(line.qty) : `${line.qty} ${line.unitName}`;
    b.push({ kind: 'row', label: `${qty} · ${t(conditionKey[line.condition] ?? 'restock')}`, value: `-${formatAmount(line.refund)}` });
  }
  b.push({ kind: 'rule', heavy: true });
  b.push({ kind: 'row', label: t('refund'), value: formatAmount(r.refundTotal), bold: true, big: true });
  b.push({ kind: 'rule', heavy: true });
  b.push({ kind: 'row', label: t('refundedAs'), value: t(refundKey[r.refundMethod] ?? 'refundCash') });
  if (r.approvedByName) b.push({ kind: 'row', label: t('approvedBy'), value: r.approvedByName });
  if (r.khataBalance !== null) b.push({ kind: 'row', label: t('khataBalance'), value: formatAmount(r.khataBalance) });
  b.push(...fbrBlocks(r.fbrInvoiceNo, t));
  b.push({ kind: 'rule' });
  b.push({ kind: 'center', text: r.returnNo, small: true, ltr: true });
  return { ...docDir(lang), blocks: b };
}

export function zReportDoc(s: ShiftSummary, lang: ReceiptLanguage): Doc {
  const t = make(lang);
  const b: Block[] = [
    ...storeHeader(s.store),
    { kind: 'center', text: t('zTitle'), bold: true },
    { kind: 'row', label: t('till'), value: s.tillCode },
    { kind: 'row', label: t('cashier'), value: s.cashierName },
    { kind: 'row', label: t('opened'), value: formatDateTime(s.openedAt) },
  ];
  if (s.closedAt) b.push({ kind: 'row', label: t('closed'), value: formatDateTime(s.closedAt) });
  b.push({ kind: 'rule' });
  b.push({ kind: 'row', label: t('bills'), value: String(s.billCount) });
  b.push({ kind: 'row', label: t('sales'), value: formatAmount(s.salesTotal) });
  b.push({ kind: 'row', label: t('returns'), value: `${s.returnsCount} · ${formatAmount(s.refundsTotal)}` });
  b.push({ kind: 'rule' });
  b.push({ kind: 'row', label: t('openingCash'), value: formatAmount(s.openingCash) });
  b.push({ kind: 'row', label: t('cashSales'), value: formatAmount(s.cashSales) });
  if (s.cashIn) b.push({ kind: 'row', label: t('cashIn'), value: formatAmount(s.cashIn) });
  if (s.cashOut) b.push({ kind: 'row', label: t('cashOut'), value: `(${formatAmount(s.cashOut)})` });
  if (s.cashRefunds) b.push({ kind: 'row', label: t('cashRefunds'), value: `(${formatAmount(s.cashRefunds)})` });
  if (s.expectedCash !== null) b.push({ kind: 'row', label: t('expectedCash'), value: formatAmount(s.expectedCash), bold: true });
  if (s.countedCash !== null) b.push({ kind: 'row', label: t('counted'), value: formatAmount(s.countedCash) });
  if (s.difference !== null) {
    const d = s.difference;
    b.push({ kind: 'row', label: t('difference'), value: `${d > 0 ? '+' : ''}${formatAmount(d)}`, bold: true, big: true });
  }
  const others = Object.entries(s.otherPayments);
  if (others.length) {
    b.push({ kind: 'rule' });
    b.push({ kind: 'center', text: t('notInDrawer'), bold: true });
    for (const [method, amount] of others) {
      b.push({ kind: 'row', label: t(paidKey[method] ?? 'card'), value: formatAmount(amount) });
    }
  }
  if (s.note) {
    b.push({ kind: 'rule' });
    b.push({ kind: 'text', text: `${t('note')}: ${s.note}` });
  }
  b.push({ kind: 'rule' });
  b.push({ kind: 'center', text: formatDateTime(new Date().toISOString()), small: true, ltr: true });
  return { ...docDir(lang), blocks: b };
}

const payMethodKey: Record<string, Key> = { cash: 'refundCash', jazzcash: 'jazzcash', easypaisa: 'easypaisa', bank: 'bank' };

/** Slip for the customer when they pay off khata. */
export function khataPaymentDoc(p: KhataPaymentSlip, lang: ReceiptLanguage): Doc {
  const t = make(lang);
  const method = payMethodKey[p.method] ?? 'refundCash';
  // JazzCash / Easypaisa keys read "Paid — JazzCash"; show just the name.
  const methodName = method === 'jazzcash' ? 'JazzCash' : method === 'easypaisa' ? 'Easypaisa' : t(method);
  const b: Block[] = [
    ...storeHeader(p.store),
    { kind: 'center', text: t('khataPayment'), bold: true },
    { kind: 'row', label: t('customer'), value: p.customerName },
    { kind: 'row', label: t('date'), value: formatDateTime(p.createdAt) },
    { kind: 'rule' },
    { kind: 'row', label: t('balanceBefore'), value: formatAmount(p.balanceBefore) },
    { kind: 'rule', heavy: true },
    { kind: 'row', label: t('received'), value: formatAmount(p.amount), bold: true, big: true },
    { kind: 'rule', heavy: true },
    { kind: 'row', label: t('paidBy'), value: methodName },
    { kind: 'row', label: t('balanceNow'), value: formatAmount(p.balanceAfter), bold: true },
    { kind: 'row', label: t('receivedBy'), value: p.receivedBy },
    { kind: 'rule' },
    { kind: 'center', text: footer(p.store, lang), bold: true },
  ];
  return { ...docDir(lang), blocks: b };
}

/** A customer's khata on 80 mm paper: every bill and payment with the running balance. */
export function khataStatementDoc(s: KhataStatement, lang: ReceiptLanguage): Doc {
  const t = make(lang);
  const b: Block[] = [
    ...storeHeader(s.store),
    { kind: 'center', text: t('statement'), bold: true },
    { kind: 'row', label: t('customer'), value: s.account.name },
  ];
  if (s.account.phone) b.push({ kind: 'row', label: 'Ph', value: s.account.phone });
  if (s.account.creditLimit > 0) b.push({ kind: 'row', label: t('creditLimit'), value: formatAmount(s.account.creditLimit) });
  b.push({ kind: 'rule' });
  for (const l of s.lines) {
    const what =
      l.kind === 'sale'
        ? `${t('bill')} ${l.reference ?? ''}`
        : l.kind === 'return'
          ? `${t('returnLine')} ${l.reference ?? ''}`
          : l.kind === 'opening'
            ? t('oldBalance')
            : `${t('payment')} · ${l.kind === 'jazzcash' ? 'JazzCash' : l.kind === 'easypaisa' ? 'Easypaisa' : t(payMethodKey[l.kind] ?? 'refundCash')}`;
    b.push({ kind: 'row', label: `${formatDateTime(l.createdAt).slice(0, 10)}  ${what.trim()}`, value: `${l.amount > 0 ? '+' : ''}${formatAmount(l.amount)}` });
    b.push({ kind: 'row', label: '', value: `= ${formatAmount(l.balance)}` });
  }
  b.push({ kind: 'rule', heavy: true });
  b.push({ kind: 'row', label: t('balanceDue'), value: formatAmount(s.account.balance), bold: true, big: true });
  b.push({ kind: 'rule', heavy: true });
  b.push({ kind: 'center', text: `${t('printed')} ${formatDateTime(new Date().toISOString())}`, small: true });
  return { ...docDir(lang), blocks: b };
}

/** The owner's one-time recovery code, to keep somewhere safe. */
export function recoveryCodeDoc(store: StoreHeader, code: string, lang: ReceiptLanguage): Doc {
  const t = make(lang);
  return {
    ...docDir(lang),
    blocks: [
      ...storeHeader(store),
      { kind: 'center', text: t('recoveryTitle'), bold: true },
      { kind: 'rule' },
      { kind: 'title', text: code },
      { kind: 'rule' },
      { kind: 'center', text: lang === 'both' ? `${L.recoveryHelp[0]}\n${L.recoveryHelp[1]}` : t('recoveryHelp'), small: true },
      { kind: 'center', text: formatDateTime(new Date().toISOString()), small: true, ltr: true },
    ],
  };
}

/** Slip that travels with the goods (sender) or is kept (receiver). */
export function transferDoc(tr: TransferView, store: StoreHeader, lang: ReceiptLanguage): Doc {
  const t = make(lang);
  const b: Block[] = [
    ...storeHeader(store),
    { kind: 'center', text: t('transferTitle'), bold: true },
    { kind: 'row', label: '#', value: tr.transferNo, bold: true },
    { kind: 'row', label: tr.direction === 'out' ? t('transferTo') : t('transferFrom'), value: `${tr.otherName ?? ''} ${tr.otherCode}`.trim() },
    { kind: 'row', label: t('date'), value: formatDateTime(tr.createdAt) },
  ];
  if (tr.note) b.push({ kind: 'row', label: t('carriedBy'), value: tr.note });
  b.push({ kind: 'rule' });
  for (const l of tr.lines) {
    b.push({ kind: 'text', text: l.productName, bold: true, ltr: true });
    const qty = tr.direction === 'in' ? `${l.receivedQty ?? 0} / ${l.sentQty}` : String(l.sentQty);
    b.push({ kind: 'row', label: `B:${l.batchNo}${l.expiry ? ` · ${l.expiry}` : ''}`, value: `${qty} ${l.baseUnit}` });
  }
  b.push({ kind: 'rule' });
  b.push({ kind: 'text', text: `${t('signature')}: ____________` });
  return { ...docDir(lang), blocks: b };
}
