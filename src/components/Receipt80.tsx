import type { Receipt, ReceiptLanguage } from '../api/types';
import { formatAmount, formatGrams } from '../domain/money';

const LABELS = {
  receiptNo: ['Receipt #', 'رسید نمبر'],
  date: ['Date', 'تاریخ'],
  cashier: ['Cashier', 'کیشیئر'],
  till: ['Till', 'ٹِل'],
  customer: ['Customer', 'گاہک'],
  walkIn: ['Walk-in', 'عام گاہک'],
  item: ['ITEM', 'آئٹم'],
  amount: ['AMOUNT', 'رقم'],
  items: ['Items', 'کل اشیاء'],
  subtotal: ['Subtotal', 'ذیلی کل'],
  discount: ['Discount', 'رعایت'],
  tax: ['Sales tax (incl.)', 'سیلز ٹیکس (شامل)'],
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
  grams: ['g', 'گرام'],
  perKg: ['/ kg', '/ کلو'],
} as const;

type Key = keyof typeof LABELS;

function label(lang: ReceiptLanguage, key: Key): string {
  const [en, ur] = LABELS[key];
  return lang === 'ur' ? ur : lang === 'both' ? `${en} · ${ur}` : en;
}

function receiptDate(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const p = (n: number) => String(n).padStart(2, '0');
  return `${p(d.getDate())}-${p(d.getMonth() + 1)}-${d.getFullYear()}  ${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** The 80 mm receipt, as previewed on screen and printed. */
export function Receipt80({ receipt, language }: { receipt: Receipt; language: ReceiptLanguage }) {
  const L = (key: Key) => label(language, key);
  const s = receipt.store;
  const policyText = language === 'both' ? `${LABELS.policy[0]}\n${LABELS.policy[1]}` : L('policy');
  return (
    <div className={`receipt${language === 'en' ? '' : ' has-ur'}`} dir={language === 'ur' ? 'rtl' : 'ltr'} lang={language === 'ur' ? 'ur' : 'en'}>
      <div className="center">
        <span style={{ fontSize: 16, fontWeight: 700 }}>{s.name}</span>
        {s.address && <span>{s.address}</span>}
        {s.phone && <span className="ltr">Ph: {s.phone}</span>}
        {(s.ntn || s.drugLicense) && (
          <span className="ltr">
            {s.ntn ? `NTN: ${s.ntn}` : ''}
            {s.ntn && s.drugLicense ? ' · ' : ''}
            {s.drugLicense ? `Drug Lic: ${s.drugLicense}` : ''}
          </span>
        )}
      </div>
      <div className="rule" />
      <div>
        <div className="rrow"><span>{L('receiptNo')}</span><span className="bold ltr">{receipt.receiptNo}</span></div>
        <div className="rrow"><span>{L('date')}</span><span className="ltr">{receiptDate(receipt.createdAt)}</span></div>
        <div className="rrow"><span>{L('cashier')}</span><span>{receipt.cashierName} · {receipt.tillCode}</span></div>
        <div className="rrow"><span>{L('customer')}</span><span>{receipt.customerName ?? L('walkIn')}</span></div>
      </div>
      <div className="rule" />
      <div className="rrow bold"><span>{L('item')}</span><span>{L('amount')}</span></div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 5 }}>
        {receipt.lines.map((line, i) => (
          <div key={i}>
            <div className="pn">
              {line.productName}
              {line.taxIncluded > 0 ? '  *' : ''}
            </div>
            <div className="rrow">
              <span>
                {line.sellMode === 'weight' ? (
                  <>
                    <span className="ltr">{formatGrams(line.qty)}</span> @ <span className="ltr">{formatAmount(line.unitPrice)}</span> {L('perKg')}
                  </>
                ) : (
                  <>
                    <span className="ltr">{line.qty}</span> {line.unitName} × <span className="ltr">{formatAmount(line.unitPrice)}</span>
                    {line.batchNo && line.batchNo !== '-' ? <span className="ltr">{`  B:${line.batchNo}`}</span> : null}
                  </>
                )}
              </span>
              <span className="ltr">{formatAmount(line.lineTotal)}</span>
            </div>
          </div>
        ))}
      </div>
      <div className="rule" />
      <div>
        <div className="rrow"><span>{L('items')}</span><span className="ltr">{receipt.itemCount}</span></div>
        <div className="rrow"><span>{L('subtotal')}</span><span className="ltr">{formatAmount(receipt.subtotal)}</span></div>
        {receipt.discount > 0 && (
          <div className="rrow"><span>{L('discount')}</span><span className="ltr">-{formatAmount(receipt.discount)}</span></div>
        )}
        {receipt.taxIncluded > 0 && (
          <div className="rrow"><span>* {L('tax')}</span><span className="ltr">{formatAmount(receipt.taxIncluded)}</span></div>
        )}
      </div>
      <div className="rtotal"><span>{L('total')}</span><span className="ltr">{formatAmount(receipt.total)}</span></div>
      <div>
        {receipt.payments.map((p, i) => (
          <div className="rrow" key={i}>
            <span>{L(p.method)}</span>
            <span className="ltr">{formatAmount(p.method === 'cash' ? p.amount + receipt.changeDue : p.amount)}</span>
          </div>
        ))}
        {receipt.changeDue > 0 && (
          <div className="rrow bold"><span>{L('change')}</span><span className="ltr">{formatAmount(receipt.changeDue)}</span></div>
        )}
        {receipt.khataBalance !== null && (
          <div className="rrow"><span>{L('khataBalance')}</span><span className="ltr">{formatAmount(receipt.khataBalance)}</span></div>
        )}
      </div>
      <div className="rule" />
      <div className="center" style={{ fontSize: 10.5 }}>
        <span style={{ whiteSpace: 'pre-line' }}>{policyText}</span>
        <span className="bold" style={{ fontSize: 12, marginTop: 4 }}>{s.receiptFooter && language === 'en' ? s.receiptFooter : L('thanks')}</span>
      </div>
      <div className="center ltr" style={{ fontSize: 10 }}>{receipt.receiptNo}</div>
    </div>
  );
}
