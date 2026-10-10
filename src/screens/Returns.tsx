import { useMemo, useState, type FormEvent } from 'react';
import { api, ApiError } from '../api/client';
import type { ApprovalInput, RefundMethod, ReturnCondition, ReturnSlip, SaleForReturn, StoreHeader, User } from '../api/types';
import { ApprovalDialog } from '../components/ApprovalDialog';
import { BrandMark } from '../components/common';
import { formatGrams, formatRs } from '../domain/money';
import { roundDiv } from '../domain/pricing';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';
import type { StringKey } from '../i18n/strings';
import { DocView, PrintPortal } from '../print/DocView';
import { formatDateTime, returnDoc } from '../print/doc';
import { printDoc, type PrinterSettings } from '../print/printer';

interface Choice {
  qty: number;
  condition: ReturnCondition;
  reason: StringKey;
}

const REASONS: StringKey[] = ['reasonNotNeeded', 'reasonWrongItem', 'reasonDoctor', 'reasonDamaged'];
const CONDITIONS: [ReturnCondition, StringKey][] = [
  ['restock', 'condRestock'],
  ['damaged', 'condDamaged'],
  ['expired', 'condExpired'],
];

function newRef(): string {
  return typeof crypto !== 'undefined' && 'randomUUID' in crypto ? crypto.randomUUID() : `${Date.now()}-${Math.random()}`;
}

export function Returns({
  user,
  store,
  tillCode,
  printer,
  onBack,
}: {
  user: User;
  store: StoreHeader;
  tillCode: string;
  printer: PrinterSettings;
  onBack: () => void;
}) {
  const { t, lang } = useI18n();
  const [receiptNo, setReceiptNo] = useState('');
  const [sale, setSale] = useState<SaleForReturn | null>(null);
  const [choices, setChoices] = useState<Record<string, Choice>>({});
  const [method, setMethod] = useState<RefundMethod>('cash');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [approval, setApproval] = useState<string | null>(null);
  const [slip, setSlip] = useState<ReturnSlip | null>(null);
  const [clientRef] = useState(newRef);

  const find = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    setSale(null);
    try {
      const found = await api.saleForReturn(receiptNo);
      setSale(found);
      setChoices(Object.fromEntries(found.lines.map((l) => [l.saleLineId, { qty: 0, condition: 'restock', reason: 'reasonNotNeeded' }])));
      setMethod(found.paymentMethods.includes('khata') && found.customerId ? 'khata' : 'cash');
    } catch (err) {
      setError(describeError(err, lang));
    }
  };

  // Same sums as the server: the line's share, less the bill's discount.
  const refund = useMemo(() => {
    if (!sale) return 0;
    let total = 0;
    for (const l of sale.lines) {
      const qty = choices[l.saleLineId]?.qty ?? 0;
      if (!qty) continue;
      const gross = roundDiv(l.lineTotal * qty, l.qty);
      total += sale.subtotal > 0 ? roundDiv(gross * sale.total, sale.subtotal) : 0;
    }
    return Math.min(total, sale.total - sale.refundedSoFar);
  }, [sale, choices]);

  const update = (id: string, patch: Partial<Choice>) => setChoices((c) => ({ ...c, [id]: { ...c[id], ...patch } }));

  const submit = async (withApproval?: ApprovalInput) => {
    if (!sale) return;
    const lines = sale.lines
      .filter((l) => (choices[l.saleLineId]?.qty ?? 0) > 0)
      .map((l) => {
        const c = choices[l.saleLineId];
        return { saleLineId: l.saleLineId, qty: c.qty, condition: c.condition, reason: t(c.reason) };
      });
    setBusy(true);
    setError(null);
    try {
      const result = await api.createReturn({ saleId: sale.id, tillCode, lines, refundMethod: method, clientRef, approval: withApproval });
      setApproval(null);
      setSlip(result);
      if (printer.autoPrint) {
        printDoc(returnDoc(result, store.receiptLanguage), printer, { openDrawer: result.refundMethod === 'cash' }).catch((e) =>
          setError(t('printFailed', { msg: String(e) })),
        );
      }
    } catch (e) {
      if (e instanceof ApiError && e.code === 'approval_required' && !withApproval) setApproval(describeError(e, lang));
      else if (withApproval) throw e;
      else setError(describeError(e, lang));
    } finally {
      setBusy(false);
    }
  };

  const slipDoc = slip ? returnDoc(slip, store.receiptLanguage) : null;
  const methods: [RefundMethod, StringKey][] = [
    ['cash', 'cash'],
    ...(sale?.customerId ? ([['khata', 'khataCredit']] as [RefundMethod, StringKey][]) : []),
    ['card', 'card'],
    ['jazzcash', 'jazzcash'],
    ['easypaisa', 'easypaisa'],
  ];

  return (
    <div className="till app">
      <header className="till-head">
        <div className="who">
          <BrandMark size={18} />
          <div style={{ display: 'flex', flexDirection: 'column' }}>
            <strong>{t('headReturns')} <kbd>F9</kbd></strong>
            <span className="small" style={{ color: 'var(--night-muted)' }}>
              {store.name} · <span className="ltr">{tillCode}</span> · {user.name}
            </span>
          </div>
        </div>
        <button type="button" className="head-btn" onClick={onBack}>{t('backToTill')}</button>
      </header>

      <div className="till-body">
        <section className="bill">
          <form className="box box-pad" onSubmit={find}>
            <label className="label small" htmlFor="receipt-no">{t('scanReceipt')}</label>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 10 }}>
              <div className="searchbar" style={{ flex: '1 1 280px' }}>
                <input id="receipt-no" className="ltr mono" value={receiptNo} onChange={(e) => setReceiptNo(e.target.value)} autoFocus placeholder="B1-T01-000482" />
              </div>
              <button type="submit" className="btn btn-primary" style={{ height: 56 }}>{t('find')}</button>
            </div>
          </form>

          {sale && (
            <div className="box" style={{ overflow: 'hidden' }}>
              <div className="results-head" style={{ fontSize: 14, display: 'flex', flexWrap: 'wrap', justifyContent: 'space-between', gap: 8 }}>
                <strong className="ltr">{sale.receiptNo}</strong>
                <span>
                  {t('billInfo', { date: formatDateTime(sale.createdAt), cashier: sale.cashierName, total: formatRs(sale.total) })}
                  {sale.customerName ? ` · ${sale.customerName}` : ''}
                </span>
              </div>
              <div className="cart-scroll">
                <div style={{ minWidth: 860 }}>
                  <div className="cart-row cart-head" style={{ gridTemplateColumns: 'minmax(170px,2fr) 80px 140px 170px 190px 90px' }}>
                    <span>{t('colItem')}</span>
                    <span>{t('sold')}</span>
                    <span>{t('returnQty')}</span>
                    <span>{t('reason')}</span>
                    <span>{t('condition')}</span>
                    <span className="end">{t('refund')}</span>
                  </div>
                  {sale.lines.map((l) => {
                    const c = choices[l.saleLineId];
                    const left = l.qty - l.returnedQty;
                    const isWeight = l.sellMode === 'weight';
                    const step = isWeight ? left : 1;
                    const lineRefund =
                      c && c.qty > 0 && sale.subtotal > 0 ? roundDiv(roundDiv(l.lineTotal * c.qty, l.qty) * sale.total, sale.subtotal) : 0;
                    return (
                      <div key={l.saleLineId} className="cart-row" style={{ gridTemplateColumns: 'minmax(170px,2fr) 80px 140px 170px 190px 90px' }}>
                        <div style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0 }}>
                          <span className="pname">{l.productName}</span>
                          <span className="tiny muted ltr" style={{ textAlign: 'start' }}>
                            {[l.batchNo && l.batchNo !== '-' && `${t('batch')} ${l.batchNo}`, l.batchExpiry && `${t('exp')} ${l.batchExpiry}`].filter(Boolean).join(' · ')}
                          </span>
                          {l.returnedQty > 0 && <span className="chip chip-warn" style={{ alignSelf: 'flex-start' }}>{t('alreadyReturned', { n: isWeight ? formatGrams(l.returnedQty) : l.returnedQty })}</span>}
                        </div>
                        <span className="small">{isWeight ? formatGrams(l.qty) : `${l.qty} ${l.unitName}`}</span>
                        {l.blocked ? (
                          <span className="chip chip-bad" style={{ gridColumn: 'span 3', justifySelf: 'start' }}>{t('nonReturnable')}</span>
                        ) : left <= 0 ? (
                          <span className="small muted" style={{ gridColumn: 'span 3' }}>—</span>
                        ) : (
                          <>
                            <div className="qty">
                              <button type="button" aria-label="Decrease return quantity" onClick={() => update(l.saleLineId, { qty: Math.max(0, c.qty - step) })}>−</button>
                              <span className="mono" style={{ minWidth: 48, textAlign: 'center', fontWeight: 600 }}>
                                {isWeight ? formatGrams(c.qty) : c.qty}
                              </span>
                              <button type="button" aria-label="Increase return quantity" onClick={() => update(l.saleLineId, { qty: Math.min(left, c.qty + step) })}>+</button>
                            </div>
                            <select className="select" style={{ height: 40, fontSize: 13 }} aria-label={t('reason')} value={c.reason} onChange={(e) => update(l.saleLineId, { reason: e.target.value as StringKey })}>
                              {REASONS.map((r) => (
                                <option key={r} value={r}>{t(r)}</option>
                              ))}
                            </select>
                            <select className="select" style={{ height: 40, fontSize: 13 }} aria-label={t('condition')} value={c.condition} onChange={(e) => update(l.saleLineId, { condition: e.target.value as ReturnCondition })}>
                              {CONDITIONS.map(([v, k]) => (
                                <option key={v} value={v}>{t(k)}</option>
                              ))}
                            </select>
                          </>
                        )}
                        <span className="end num" style={{ fontWeight: 600 }}>{lineRefund ? formatRs(lineRefund) : '—'}</span>
                      </div>
                    );
                  })}
                </div>
              </div>
              <div style={{ padding: '12px 16px' }} className="small muted">{t('returnRule')}</div>
            </div>
          )}
          {error && <div className="alert alert-bad" role="alert">{error}</div>}
        </section>

        {sale && (
          <aside className="side">
            <div className="box box-pad">
              <div className="total-row" style={{ borderTop: 0, paddingTop: 0 }}>
                <strong style={{ fontSize: 18 }}>{t('refund')}</strong>
                <span className="total-amount" style={{ color: 'var(--bad)' }}>{formatRs(refund)}</span>
              </div>
            </div>
            <div className="box box-pad">
              <span className="label small">{t('refundAs')}</span>
              <div className="methods">
                {methods.map(([m, k]) => (
                  <button key={m} type="button" className="choice" aria-pressed={method === m} onClick={() => setMethod(m)}>
                    {t(k)}
                  </button>
                ))}
              </div>
              <span className="small muted">
                {method === 'cash' ? t('refundNoteCash') : method === 'khata' ? t('refundNoteKhata') : t('refundNoteOther')}
              </span>
            </div>
            <button type="button" className="btn btn-primary pay-btn" disabled={refund <= 0 || busy} onClick={() => void submit()}>
              {t('refundButton')}
            </button>
          </aside>
        )}
      </div>

      {approval && <ApprovalDialog message={approval} onApprove={(a) => submit(a)} onCancel={() => setApproval(null)} />}

      {slip && slipDoc && (
        <>
          <div className="overlay" role="dialog" aria-modal="true" aria-label={t('refundDone')}>
            <div className="dialog receipt-dialog">
              <div className="receipt-preview">
                <DocView doc={slipDoc} />
              </div>
              <div className="actions">
                <h2>{t('refundDone')}</h2>
                <span className="mono" style={{ fontSize: 13 }}>{slip.returnNo}</span>
                {slip.refundMethod === 'cash' && (
                  <div className="alert alert-ok" style={{ fontSize: 22, fontWeight: 700 }}>{t('giveRefund', { amount: formatRs(slip.refundTotal) })}</div>
                )}
                {error && <div className="alert alert-bad">{error}</div>}
                <button type="button" className="btn" onClick={() => void printDoc(slipDoc, printer).catch((e) => setError(t('printFailed', { msg: String(e) })))}>
                  {t('printAgain')}
                </button>
                <button type="button" className="btn btn-primary btn-lg" onClick={onBack} autoFocus>{t('backToTill')}</button>
              </div>
            </div>
          </div>
          <PrintPortal doc={slipDoc} />
        </>
      )}
    </div>
  );
}
