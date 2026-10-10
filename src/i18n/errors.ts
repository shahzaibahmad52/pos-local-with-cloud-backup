import { ApiError } from '../api/client';
import type { Language } from '../api/types';
import { formatRs } from '../domain/money';

type Params = Record<string, unknown>;
const n = (v: unknown) => Number(v ?? 0);
const s = (v: unknown) => String(v ?? '');

// Urdu versions of the server's messages, keyed by `reason`. English comes
// straight from the server.
const UR: Record<string, (p: Params) => string> = {
  'stock.short': (p) =>
    `${s(p.product)} کا اسٹاک کم ہے: ${n(p.need)} ${s(p.unit)} چاہیے، صرف ${n(p.have)} موجود ہیں۔` +
    (n(p.expired) > 0 ? ` (${n(p.expired)} ${s(p.unit)} کی میعاد ختم ہو چکی ہے، بیچے نہیں جا سکتے۔)` : ''),
  'approval.discount': (p) => `${n(p.maxPercent)}% سے زیادہ رعایت کے لیے منیجر PIN درکار ہے۔`,
  'approval.khata': (p) =>
    `${s(p.customer)} کا کھاتہ ${formatRs(n(p.balance))} ہو جائے گا (حد ${formatRs(n(p.limit))})۔ منیجر PIN درکار ہے۔`,
  'approval.refund': (p) => `${formatRs(n(p.limit))} سے زیادہ ریفنڈ کے لیے منیجر PIN درکار ہے۔`,
  'approval.action': () => 'اس کام کے لیے منیجر PIN درکار ہے۔',
  'approval.cash_out': () => 'دراز سے نقد نکالنے کے لیے منیجر PIN درکار ہے۔',
  'approval.not_allowed': (p) => `${s(p.name)} اس کی اجازت نہیں دے سکتے۔`,
  'payment.short': (p) => `ادائیگی ${formatRs(n(p.amount))} کم ہے۔`,
  'payment.change_not_cash': () => 'واپسی صرف نقد میں دی جا سکتی ہے۔',
  'khata.no_customer': () => 'پہلے کھاتہ گاہک منتخب کریں۔',
  'bill.empty': () => 'بل خالی ہے۔',
  'auth.wrong': () => 'غلط PIN یا پاس ورڈ۔',
  'auth.locked': (p) => `بہت زیادہ غلط کوششیں۔ ${n(p.minutes)} منٹ بعد دوبارہ کوشش کریں۔`,
  'return.no_sale': (p) => `رسید نمبر ${s(p.receiptNo)} کی کوئی سیل نہیں ملی۔`,
  'return.cold_chain': (p) => `${s(p.product)} فریج والی چیز ہے، واپس نہیں ہو سکتی۔`,
  'return.too_many': (p) => `اس سیل سے ${s(p.product)} کی صرف ${n(p.left)} واپسی باقی ہے۔`,
  'return.no_khata_customer': () => 'اس سیل میں کھاتہ گاہک نہیں تھا۔ نقد میں ریفنڈ کریں۔',
  'return.empty': () => 'واپس ہونے والی اشیاء منتخب کریں۔',
  'shift.note_required': (p) => `فرق (${formatRs(Math.abs(n(p.difference)))}) کی وضاحت میں نوٹ لکھیں۔`,
  'shift.recount_limit': () => 'نقد دو بار گنا جا چکا ہے۔',
  'shift.not_counted': () => 'شفٹ بند کرنے سے پہلے نقد گنیں۔',
  'shift.closed': () => 'یہ شفٹ پہلے ہی بند ہے۔',
  'cash.reason_required': () => 'لکھیں کہ نقد کس لیے ہے۔',
};

const UR_BY_CODE: Record<string, string> = {
  network: 'مین کمپیوٹر سے رابطہ نہیں ہو رہا۔ نیٹ ورک دیکھیں۔',
  unauthorized: 'دوبارہ سائن اِن کریں۔',
  internal: 'کچھ غلط ہو گیا۔ دوبارہ کوشش کریں۔',
};

/** A message for the cashier, in their language when we have a translation. */
export function describeError(error: unknown, lang: Language): string {
  if (!(error instanceof ApiError)) return String(error instanceof Error ? error.message : error);
  if (lang === 'ur') {
    const byReason = error.reason ? UR[error.reason] : undefined;
    if (byReason) return byReason(error.params);
    const byCode = UR_BY_CODE[error.code];
    if (byCode) return byCode;
  }
  return error.message;
}
