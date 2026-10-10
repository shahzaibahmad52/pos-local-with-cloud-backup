import { useEffect, useState } from 'react';
import { api } from '../api/client';
import type { Shift, ShiftSummary, StoreHeader } from '../api/types';
import { BrandMark } from '../components/common';
import { formatAmount, formatRs } from '../domain/money';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';
import type { StringKey } from '../i18n/strings';
import { DocView, PrintPortal } from '../print/DocView';
import { zReportDoc } from '../print/doc';
import { printDoc, type PrinterSettings } from '../print/printer';

const NOTES = [5000, 1000, 500, 100, 50, 20, 10];
/** Coins are typed as a rupee total, not counted one by one. */
const COINS = 'coins';

export function ShiftClose({
  shift,
  store,
  printer,
  onBack,
  onClosed,
}: {
  shift: Shift;
  store: StoreHeader;
  printer: PrinterSettings;
  onBack: () => void;
  onClosed: () => void;
}) {
  const { t, lang } = useI18n();
  const [summary, setSummary] = useState<ShiftSummary | null>(null);
  const [counts, setCounts] = useState<Record<string, string>>({});
  const [note, setNote] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [closed, setClosed] = useState<ShiftSummary | null>(null);

  useEffect(() => {
    api.shiftSummary(shift.id).then(setSummary).catch((e) => setError(describeError(e, lang)));
  }, [shift.id, lang]);

  const num = (k: string) => {
    const n = parseInt((counts[k] ?? '').replace(/\D/g, ''), 10);
    return Number.isNaN(n) ? 0 : n;
  };
  const counted = NOTES.reduce((sum, v) => sum + v * num(String(v)), 0) * 100 + num(COINS) * 100;
  const counting = !summary || summary.countAttempts === 0 || summary.expectedCash === null;
  const canRecount = summary && summary.countAttempts === 1;
  const [recounting, setRecounting] = useState(false);
  const showCountForm = counting || recounting;

  const submitCount = async () => {
    // Coins count as Rs 1 coins so the server's note list stays the source of truth.
    const denominations: Record<string, number> = {};
    for (const v of NOTES) if (num(String(v))) denominations[String(v)] = num(String(v));
    if (num(COINS)) denominations['1'] = num(COINS);
    setBusy(true);
    setError(null);
    try {
      setSummary(await api.countCash(shift.id, denominations));
      setRecounting(false);
    } catch (e) {
      setError(describeError(e, lang));
    } finally {
      setBusy(false);
    }
  };

  const close = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await api.closeShift(shift.id, note.trim() || null);
      setClosed(result);
      printDoc(zReportDoc(result, store.receiptLanguage), printer).catch((e) => setError(t('printFailed', { msg: String(e) })));
    } catch (e) {
      setError(describeError(e, lang));
    } finally {
      setBusy(false);
    }
  };

  const diff = summary?.difference ?? null;
  const row = (label: StringKey, value: string, strong = false) => (
    <div className="row" style={{ padding: '6px 0', borderBottom: '1px solid var(--line-2)', fontWeight: strong ? 700 : undefined }}>
      <span>{t(label)}</span>
      <span className="num">{value}</span>
    </div>
  );

  return (
    <div className="till app">
      <header className="till-head">
        <div className="who">
          <BrandMark size={18} />
          <div style={{ display: 'flex', flexDirection: 'column' }}>
            <strong>{t('headCloseShift')}</strong>
            <span className="small" style={{ color: 'var(--night-muted)' }}>
              {store.name} · <span className="ltr">{shift.tillCode}</span> · {shift.userName}
              {summary ? ` · ${t('shiftStats', { bills: summary.billCount, sales: formatRs(summary.salesTotal), returns: summary.returnsCount })}` : ''}
            </span>
          </div>
        </div>
        <button type="button" className="head-btn" onClick={onBack}>{t('backToTill')}</button>
      </header>

      <div className="till-body">
        <section className="bill box box-pad" aria-label={t('countTitle')}>
          <div className="row" style={{ flexWrap: 'wrap' }}>
            <h1 style={{ fontSize: 22 }}>{t('countTitle')}</h1>
            <span className="small muted">{t('countHelp')}</span>
          </div>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(250px, 1fr))', gap: 10 }}>
            {[...NOTES.map(String), COINS].map((k) => (
              <div key={k} style={{ display: 'grid', gridTemplateColumns: '96px 1fr 100px', gap: 10, alignItems: 'center', border: '1px solid var(--line)', borderRadius: 10, padding: '8px 12px' }}>
                <label htmlFor={`n${k}`} className={k === COINS ? 'small' : 'mono'} style={{ fontWeight: 600, fontSize: k === COINS ? 13 : 16 }}>
                  {k === COINS ? t('coins') : k}
                </label>
                <input
                  id={`n${k}`}
                  className="input ltr mono"
                  style={{ height: 44, textAlign: 'center', fontSize: 18 }}
                  inputMode="numeric"
                  disabled={!showCountForm}
                  value={counts[k] ?? ''}
                  onChange={(e) => setCounts({ ...counts, [k]: e.target.value })}
                />
                <span className="num small muted" style={{ textAlign: 'end' }}>
                  {formatAmount((k === COINS ? num(k) : Number(k) * num(k)) * 100)}
                </span>
              </div>
            ))}
          </div>
          <div className="total-row">
            <strong style={{ fontSize: 18 }}>{t('cashCounted')}</strong>
            <span className="total-amount" style={{ color: 'var(--ink)' }}>{formatRs(counted)}</span>
          </div>
          {showCountForm && (
            <button type="button" className="btn btn-primary btn-lg" disabled={busy || !summary} onClick={() => void submitCount()}>
              {t('submitCount')}
            </button>
          )}
          {error && <div className="alert alert-bad" role="alert">{error}</div>}
        </section>

        <aside className="side">
          {summary && counting && !recounting && (
            <div className="box box-pad" style={{ alignItems: 'center', textAlign: 'center', padding: 24 }}>
              <strong style={{ fontSize: 16 }}>{t('blindTitle')}</strong>
              <span className="small muted">{t('blindHelp')}</span>
            </div>
          )}
          {summary && !counting && summary.expectedCash !== null && (
            <>
              <div className="box box-pad">
                <h2>{t('expectedTitle')}</h2>
                {row('rowOpening', formatAmount(summary.openingCash))}
                {row('rowCashSales', formatAmount(summary.cashSales))}
                {summary.cashIn > 0 && row('rowCashIn', formatAmount(summary.cashIn))}
                {summary.cashOut > 0 && row('rowCashOut', `(${formatAmount(summary.cashOut)})`)}
                {summary.cashRefunds > 0 && row('rowCashRefunds', `(${formatAmount(summary.cashRefunds)})`)}
                {row('rowExpected', formatAmount(summary.expectedCash), true)}
                {summary.countedCash !== null && row('rowCounted', formatAmount(summary.countedCash))}
                {diff !== null && (
                  <div className={`alert ${diff === 0 ? 'alert-ok' : diff < 0 ? 'alert-bad' : 'alert-warn'}`} style={{ display: 'flex', justifyContent: 'space-between', fontWeight: 700 }}>
                    <span>{diff === 0 ? t('exactLabel') : diff < 0 ? t('shortLabel') : t('overLabel')}</span>
                    <span className="num">{diff === 0 ? '0' : `${diff < 0 ? '−' : '+'} ${formatAmount(Math.abs(diff))}`}</span>
                  </div>
                )}
              </div>
              {Object.keys(summary.otherPayments).length > 0 && (
                <div className="box box-pad small">
                  <span className="label small">{t('notInDrawer')}</span>
                  {Object.entries(summary.otherPayments).map(([m, amount]) => (
                    <div key={m} className="row">
                      <span>{t((m === 'khata' ? 'khata' : m) as StringKey)}</span>
                      <span className="num">{formatAmount(amount)}</span>
                    </div>
                  ))}
                </div>
              )}
              <div className="box box-pad">
                <label className="label small" htmlFor="close-note">{t('closeNote')}</label>
                <textarea id="close-note" className="input" style={{ height: 'auto', padding: 10 }} rows={2} value={note} onChange={(e) => setNote(e.target.value)} />
                <button type="button" className="btn btn-primary btn-lg" disabled={busy} onClick={() => void close()}>
                  {t('closeAndPrint')}
                </button>
                {canRecount && !recounting && (
                  <button type="button" className="btn" onClick={() => setRecounting(true)}>{t('recount')}</button>
                )}
              </div>
            </>
          )}
        </aside>
      </div>

      {closed && (
        <>
          <div className="overlay" role="dialog" aria-modal="true" aria-label={t('shiftClosed')}>
            <div className="dialog receipt-dialog">
              <div className="receipt-preview">
                <DocView doc={zReportDoc(closed, store.receiptLanguage)} />
              </div>
              <div className="actions">
                <h2>{t('shiftClosed')}</h2>
                {error && <div className="alert alert-bad">{error}</div>}
                <button type="button" className="btn" onClick={() => void printDoc(zReportDoc(closed, store.receiptLanguage), printer)}>{t('printAgain')}</button>
                <button type="button" className="btn btn-primary btn-lg" onClick={onClosed} autoFocus>{t('done')}</button>
              </div>
            </div>
          </div>
          <PrintPortal doc={zReportDoc(closed, store.receiptLanguage)} />
        </>
      )}
    </div>
  );
}
