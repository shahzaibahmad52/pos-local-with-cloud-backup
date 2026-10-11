import { useEffect, useState } from 'react';
import { api } from '../api/client';
import type { MoneySource, MonthExpenses } from '../api/types';
import { formatDate, formatMonth, monthOf, recentMonths, today } from '../domain/dates';
import { formatAmount, formatRs, parseRupees } from '../domain/money';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';
import type { StringKey } from '../i18n/strings';
import { MoneySourcePicker, SOURCES, useOpenTills } from './parts';
import { saveFile } from '../platform';

/** Categories as stored (English) with their Urdu names for the buttons. */
const CATEGORIES: [string, string][] = [
  ['Electricity', 'بجلی'],
  ['Generator fuel', 'جنریٹر فیول'],
  ['Rent', 'کرایہ'],
  ['Salaries', 'تنخواہیں'],
  ['Internet & phone', 'انٹرنیٹ اور فون'],
  ['Tea & food', 'چائے اور کھانا'],
  ['Repairs', 'مرمت'],
  ['Other', 'دیگر'],
];

export function Expenses({ tillCode }: { tillCode: string }) {
  const { t, lang } = useI18n();
  const tills = useOpenTills();
  const [month, setMonth] = useState(monthOf(today()));
  const [data, setData] = useState<MonthExpenses | null>(null);
  const [category, setCategory] = useState('Generator fuel');
  const [other, setOther] = useState('');
  const [amount, setAmount] = useState('');
  const [date, setDate] = useState(today());
  const [from, setFrom] = useState<MoneySource>('owner');
  const [till, setTill] = useState('');
  const [note, setNote] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = (m: string) =>
    api
      .expenses(m)
      .then(setData)
      .catch((e) => setError(describeError(e, lang)));

  useEffect(() => {
    void load(month);
  }, [month, lang]);

  const catName = (c: string) => {
    const pair = CATEGORIES.find(([en]) => en === c);
    return pair && lang === 'ur' ? pair[1] : c;
  };
  const sourceName = (s: MoneySource) => t(SOURCES.find(([v]) => v === s)?.[1] ?? ('srcOwner' as StringKey));

  const save = async () => {
    setError(null);
    setMessage(null);
    const paisa = parseRupees(amount);
    const cat = category === 'Other' ? other.trim() || 'Other' : category;
    if (!paisa) return setError(t('amountField'));
    setBusy(true);
    try {
      await api.addExpense({
        category: cat,
        amount: paisa,
        date,
        paidFrom: from,
        tillCode: from === 'drawer' ? till || tillCode : undefined,
        note: note.trim() || undefined,
      });
      setAmount('');
      setNote('');
      setOther('');
      setMessage(t('expenseSaved'));
      if (monthOf(date) === month) await load(month);
      else setMonth(monthOf(date));
    } catch (e) {
      setError(describeError(e, lang));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (id: string, amt: number) => {
    if (!window.confirm(t('confirmDelete', { amount: formatRs(amt) }))) return;
    try {
      await api.deleteExpense(id);
      await load(month);
    } catch (e) {
      setError(describeError(e, lang));
    }
  };

  const exportExcel = async () => {
    if (!data) return;
    const { default: writeXlsxFile } = await import('write-excel-file/browser');
    const bold = (v: string) => ({ value: v, fontWeight: 'bold' as const });
    const path = await saveFile(`expenses-${month}.xlsx`, await writeXlsxFile(
      [
        [bold('Date'), bold('Category'), bold('Note'), bold('Paid from'), bold('Amount (Rs)'), bold('Entered by')],
        ...data.entries.map((e) => [e.date, e.category, e.note ?? '', e.paidFrom + (e.tillCode ? ` ${e.tillCode}` : ''), e.amount / 100, e.userName]),
        ['', '', '', bold('Total'), { value: data.total / 100, fontWeight: 'bold' as const }, ''],
      ],
      { columns: [{ width: 12 }, { width: 18 }, { width: 30 }, { width: 14 }, { width: 12 }, { width: 14 }] },
    ).toBlob());
    if (path) setMessage(t('savedTo', { file: path }));
  };

  const max = Math.max(1, ...(data?.byCategory.map(([, v]) => v) ?? [1]));

  return (
    <>
      <div className="bo-head">
        <div>
          <h1>{t('expTitle')}</h1>
          {data && <span className="sub">{t('expSub', { month: formatMonth(month), total: formatRs(data.total) })}</span>}
        </div>
        <div className="bo-actions">
          <label className="visually-hidden" htmlFor="ex-month">{t('monthField')}</label>
          <select id="ex-month" className="select" value={month} onChange={(e) => setMonth(e.target.value)}>
            {recentMonths(12).map((m) => (
              <option key={m} value={m}>
                {formatMonth(m)}
              </option>
            ))}
          </select>
          <button type="button" className="btn" disabled={!data?.entries.length} onClick={() => void exportExcel().catch((e) => setError(String(e)))}>
            {t('exportExcel')}
          </button>
        </div>
      </div>

      <div className="cols">
        <section className="card grow" aria-label={t('addExpense')}>
          <h2>{t('addExpense')}</h2>
          <div className="field">
            <span className="label">{t('colCategory')}</span>
            <div className="pills">
              {CATEGORIES.map(([c]) => (
                <button key={c} type="button" aria-pressed={category === c} onClick={() => setCategory(c)}>
                  {catName(c)}
                </button>
              ))}
            </div>
            {category === 'Other' && (
              <input aria-label={t('otherCategoryName')} placeholder={t('otherCategoryName')} className="input" value={other} onChange={(e) => setOther(e.target.value)} />
            )}
          </div>
          <div className="form-grid">
            <div className="field">
              <label className="label" htmlFor="ex-amt">{t('amountField')}</label>
              <input id="ex-amt" className="input big ltr" inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} />
            </div>
            <div className="field">
              <label className="label" htmlFor="ex-date">{t('dateField')}</label>
              <input id="ex-date" type="date" className="input ltr" max={today()} value={date} onChange={(e) => setDate(e.target.value)} />
            </div>
            <MoneySourcePicker id="ex-from" label={t('paidFrom')} value={from} onChange={setFrom} till={till} onTill={setTill} tills={tills} />
          </div>
          <div className="field">
            <label className="label" htmlFor="ex-note">{t('expNote')}</label>
            <input id="ex-note" className="input" value={note} onChange={(e) => setNote(e.target.value)} />
          </div>
          {error && <div className="alert alert-bad" role="alert">{error}</div>}
          {message && <div className="alert alert-ok" role="status">{message}</div>}
          <button type="button" className="btn btn-primary btn-lg" disabled={busy} onClick={() => void save()}>
            {t('saveExpense')}
          </button>
        </section>

        <section className="card aside">
          <h2>{t('byCategoryTitle', { month: formatMonth(month) })}</h2>
          {data?.byCategory.length === 0 && <span className="muted">{t('noExpenses')}</span>}
          {data?.byCategory.map(([c, v]) => (
            <div key={c} style={{ display: 'flex', flexDirection: 'column', gap: 4 }} title={`${c}: ${formatRs(v)}`}>
              <div className="stat">
                <span>{catName(c)}</span>
                <span className="mono">{formatAmount(v)}</span>
              </div>
              <div className="hbar" style={{ width: `${(v / max) * 100}%` }} />
            </div>
          ))}
          {data && data.total > 0 && (
            <div className="total-row">
              <strong>{t('total')}</strong>
              <strong className="mono">{formatRs(data.total)}</strong>
            </div>
          )}
          <span className="small muted">{t('expensesToPL')}</span>
        </section>
      </div>

      <section className="card" style={{ padding: 0, overflow: 'hidden' }}>
        <h2 style={{ padding: '16px 18px 0' }}>{t('monthEntries')}</h2>
        <div className="tbl-scroll">
          <table className="tbl" style={{ minWidth: 680 }}>
            <thead>
              <tr>
                <th>{t('colDate')}</th>
                <th>{t('colCategory')}</th>
                <th>{t('colNote')}</th>
                <th>{t('colPaidFrom')}</th>
                <th className="num">{t('colAmount2')}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {data?.entries.map((e) => (
                <tr key={e.id}>
                  <td className="small ltr">{formatDate(e.date)}</td>
                  <td>{catName(e.category)}</td>
                  <td className="small">{e.note ?? ''}</td>
                  <td className="small">
                    {sourceName(e.paidFrom)}
                    {e.tillCode ? ` (${e.tillCode})` : ''}
                  </td>
                  <td className="num">{formatAmount(e.amount)}</td>
                  <td style={{ width: 110 }}>
                    {e.locked ? (
                      <span className="tiny muted">{t('lockedHint')}</span>
                    ) : (
                      <button type="button" className="btn btn-danger" style={{ height: 34, fontSize: 13 }} onClick={() => void remove(e.id, e.amount)}>
                        {t('deleteBtn')}
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {data?.entries.length === 0 && <div className="empty">{t('noExpenses')}</div>}
        </div>
      </section>
    </>
  );
}
