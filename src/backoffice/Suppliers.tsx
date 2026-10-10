import { useEffect, useState } from 'react';
import { api } from '../api/client';
import type { MoneySource, Supplier, SupplierLedgerLine } from '../api/types';
import { formatDate } from '../domain/dates';
import { formatAmount, formatRs, parseRupees } from '../domain/money';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';
import { Modal, MoneySourcePicker, useOpenTills } from './parts';

export function Suppliers({ tillCode }: { tillCode: string }) {
  const { t, lang } = useI18n();
  const [list, setList] = useState<Supplier[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [ledger, setLedger] = useState<SupplierLedgerLine[]>([]);
  const [dialog, setDialog] = useState<'edit' | 'new' | 'pay' | 'opening' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  const load = () =>
    api
      .suppliers()
      .then((s) => {
        setList(s);
        setSelected((cur) => cur ?? s[0]?.id ?? null);
      })
      .catch((e) => setError(describeError(e, lang)));

  useEffect(() => {
    void load();
  }, [lang]);

  useEffect(() => {
    if (!selected) return setLedger([]);
    api
      .supplierLedger(selected)
      .then(setLedger)
      .catch((e) => setError(describeError(e, lang)));
  }, [selected, lang, list]);

  const sel = list?.find((s) => s.id === selected) ?? null;
  const total = list?.reduce((sum, s) => sum + Math.max(0, s.balance), 0) ?? 0;

  const updated = (s: Supplier, text?: string) => {
    setList((l) => (l ? (l.some((x) => x.id === s.id) ? l.map((x) => (x.id === s.id ? s : x)) : [...l, s]) : [s]));
    setSelected(s.id);
    setDialog(null);
    setMessage(text ?? null);
  };

  return (
    <>
      {message && <div className="alert alert-ok" role="status">{message}</div>}
      {error && <div className="alert alert-bad">{error}</div>}
      <div className="cols">
        <section className="card grow">
          <div className="card-head">
            <h2>
              {t('payablesTitle')} · <span className="mono">{formatRs(total)}</span>
            </h2>
            <button type="button" className="btn" style={{ height: 40 }} onClick={() => setDialog('new')}>
              {t('newSupplierBtn')}
            </button>
          </div>
          {list?.length === 0 && <span className="muted">{t('noSuppliers')}</span>}
          {list?.map((s) => (
            <button
              key={s.id}
              type="button"
              className="list-row"
              aria-pressed={s.id === selected}
              style={{
                border: 0,
                borderBottom: '1px solid var(--line-2)',
                background: s.id === selected ? 'var(--brand-soft)' : 'transparent',
                textAlign: 'start',
                width: '100%',
                padding: '10px 8px',
              }}
              onClick={() => setSelected(s.id)}
            >
              <div className="main">
                <strong>{s.name}</strong>
                <span className="small muted">
                  {s.termsDays ? t('termsDays', { n: s.termsDays }) : t('termsCash')}
                  {s.phone ? ` · ${s.phone}` : ''}
                </span>
              </div>
              <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-end', gap: 2 }}>
                <span className="mono">{formatRs(s.balance)}</span>
                {s.balance > 0 ? (
                  <span className={`tag ${s.overdue ? 'tag-bad' : 'tag-mute'}`}>
                    {s.overdue ? t('overdueChip') : s.nextDue ? t('dueOn', { date: formatDate(s.nextDue) }) : ''}
                  </span>
                ) : (
                  <span className="tag tag-ok">{t('settledChip')}</span>
                )}
              </div>
            </button>
          ))}
        </section>

        {sel && (
          <aside className="card aside">
            <div className="card-head">
              <h2>{sel.name}</h2>
              <button type="button" className="btn" style={{ height: 36 }} onClick={() => setDialog('edit')}>
                {t('editBtn')}
              </button>
            </div>
            <div className="stat">
              <span>{t('colBalance')}</span>
              <strong className="mono" style={{ fontSize: 20 }}>{formatRs(sel.balance)}</strong>
            </div>
            {sel.notes && <span className="small muted">{sel.notes}</span>}
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
              <button type="button" className="btn btn-primary" disabled={sel.balance <= 0} onClick={() => setDialog('pay')}>
                {t('recordPayment')}
              </button>
              <button type="button" className="btn" onClick={() => setDialog('opening')}>
                {t('oldBalance')}
              </button>
            </div>
            <h3 className="small muted" style={{ margin: '6px 0 0' }}>{t('ledgerTitle')}</h3>
            <div className="tbl-scroll">
              <table className="tbl">
                <thead>
                  <tr>
                    <th>{t('colDate')}</th>
                    <th>{t('colDetail')}</th>
                    <th className="num">{t('colOwedPlus')}</th>
                    <th className="num">{t('colPaidMinus')}</th>
                    <th className="num">{t('colBalance')}</th>
                  </tr>
                </thead>
                <tbody>
                  {ledger.map((l, i) => (
                    <tr key={i}>
                      <td className="small ltr" style={{ whiteSpace: 'nowrap' }}>{formatDate(l.createdAt)}</td>
                      <td className="small ltr">{l.detail}</td>
                      <td className="num small">{l.amount > 0 ? formatAmount(l.amount) : ''}</td>
                      <td className="num small">{l.amount < 0 ? formatAmount(-l.amount) : ''}</td>
                      <td className="num small">{formatAmount(l.balance)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </aside>
        )}
      </div>

      {(dialog === 'new' || dialog === 'edit') && (
        <SupplierDialog supplier={dialog === 'edit' ? sel : null} onClose={() => setDialog(null)} onSaved={(s) => updated(s, t('saved'))} />
      )}
      {dialog === 'pay' && sel && (
        <PayDialog supplier={sel} tillCode={tillCode} onClose={() => setDialog(null)} onSaved={(s) => updated(s, t('paymentSaved'))} />
      )}
      {dialog === 'opening' && sel && <OpeningDialog supplier={sel} onClose={() => setDialog(null)} onSaved={(s) => updated(s, t('saved'))} />}
    </>
  );
}

export function SupplierDialog({
  supplier,
  onClose,
  onSaved,
}: {
  supplier: Supplier | null;
  onClose: () => void;
  onSaved: (s: Supplier) => void;
}) {
  const { t, lang } = useI18n();
  const [name, setName] = useState(supplier?.name ?? '');
  const [phone, setPhone] = useState(supplier?.phone ?? '');
  const [notes, setNotes] = useState(supplier?.notes ?? '');
  const [days, setDays] = useState(String(supplier?.termsDays ?? 30));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const save = async () => {
    setBusy(true);
    try {
      onSaved(
        await api.saveSupplier(supplier?.id ?? null, {
          name,
          phone: phone.trim() || undefined,
          notes: notes.trim() || undefined,
          termsDays: Number(days) || 0,
        }),
      );
    } catch (e) {
      setError(describeError(e, lang));
      setBusy(false);
    }
  };

  return (
    <Modal title={supplier ? supplier.name : t('newSupplierBtn')} onClose={onClose}>
      <div className="field">
        <label className="label" htmlFor="sd-name">{t('supplierName')}</label>
        <input id="sd-name" className="input" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </div>
      <div className="field">
        <label className="label" htmlFor="sd-phone">{t('phone')}</label>
        <input id="sd-phone" className="input ltr" inputMode="tel" value={phone} onChange={(e) => setPhone(e.target.value)} />
      </div>
      <div className="field">
        <label className="label" htmlFor="sd-days">{t('creditDays')}</label>
        <input id="sd-days" className="input ltr" inputMode="numeric" value={days} onChange={(e) => setDays(e.target.value.replace(/\D/g, ''))} />
      </div>
      <div className="field">
        <label className="label" htmlFor="sd-notes">{t('notes')}</label>
        <input id="sd-notes" className="input" value={notes} onChange={(e) => setNotes(e.target.value)} />
      </div>
      {error && <div className="alert alert-bad">{error}</div>}
      <div style={{ display: 'flex', gap: 10 }}>
        <button type="button" className="btn" style={{ flex: 1 }} onClick={onClose}>
          {t('cancel')}
        </button>
        <button type="button" className="btn btn-primary" style={{ flex: 2 }} disabled={busy || !name.trim()} onClick={() => void save()}>
          {t('saveSupplier')}
        </button>
      </div>
    </Modal>
  );
}

function PayDialog({
  supplier,
  tillCode,
  onClose,
  onSaved,
}: {
  supplier: Supplier;
  tillCode: string;
  onClose: () => void;
  onSaved: (s: Supplier) => void;
}) {
  const { t, lang } = useI18n();
  const tills = useOpenTills();
  const [amount, setAmount] = useState('');
  const [from, setFrom] = useState<MoneySource>('owner');
  const [till, setTill] = useState('');
  const [note, setNote] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const paisa = parseRupees(amount);

  const save = async () => {
    if (!paisa) return;
    setBusy(true);
    try {
      onSaved(
        await api.paySupplier(supplier.id, {
          amount: paisa,
          paidFrom: from,
          tillCode: from === 'drawer' ? till || tillCode : undefined,
          note: note.trim() || undefined,
        }),
      );
    } catch (e) {
      setError(describeError(e, lang));
      setBusy(false);
    }
  };

  return (
    <Modal title={`${t('recordPayment')} · ${supplier.name}`} onClose={onClose}>
      <div className="stat">
        <span>{t('colBalance')}</span>
        <strong className="mono">{formatRs(supplier.balance)}</strong>
      </div>
      <div className="field">
        <label className="label" htmlFor="sp-amt">{t('amountField')}</label>
        <div style={{ display: 'flex', gap: 8 }}>
          <input id="sp-amt" className="input big ltr" style={{ flex: 1 }} inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} autoFocus />
          <button type="button" className="btn" onClick={() => setAmount(String(supplier.balance / 100))}>
            {t('fullBalance')}
          </button>
        </div>
      </div>
      <MoneySourcePicker id="sp-from" label={t('paidFrom')} value={from} onChange={setFrom} till={till} onTill={setTill} tills={tills} />
      <div className="field">
        <label className="label" htmlFor="sp-note">{t('noteOptional')}</label>
        <input id="sp-note" className="input" value={note} onChange={(e) => setNote(e.target.value)} />
      </div>
      {error && <div className="alert alert-bad">{error}</div>}
      <div style={{ display: 'flex', gap: 10 }}>
        <button type="button" className="btn" style={{ flex: 1 }} onClick={onClose}>
          {t('cancel')}
        </button>
        <button type="button" className="btn btn-primary" style={{ flex: 2 }} disabled={busy || !paisa} onClick={() => void save()}>
          {t('recordPayment')}
        </button>
      </div>
    </Modal>
  );
}

function OpeningDialog({ supplier, onClose, onSaved }: { supplier: Supplier; onClose: () => void; onSaved: (s: Supplier) => void }) {
  const { t, lang } = useI18n();
  const [amount, setAmount] = useState('');
  const [note, setNote] = useState('');
  const [error, setError] = useState<string | null>(null);
  const paisa = parseRupees(amount);
  return (
    <Modal title={`${t('oldBalance')} · ${supplier.name}`} onClose={onClose}>
      <span className="help">{t('oldBalanceHelp')}</span>
      <div className="field">
        <label className="label" htmlFor="so-amt">{t('amountField')}</label>
        <input id="so-amt" className="input big ltr" inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} autoFocus />
      </div>
      <div className="field">
        <label className="label" htmlFor="so-note">{t('noteOptional')}</label>
        <input id="so-note" className="input" value={note} onChange={(e) => setNote(e.target.value)} />
      </div>
      {error && <div className="alert alert-bad">{error}</div>}
      <div style={{ display: 'flex', gap: 10 }}>
        <button type="button" className="btn" style={{ flex: 1 }} onClick={onClose}>
          {t('cancel')}
        </button>
        <button
          type="button"
          className="btn btn-primary"
          style={{ flex: 2 }}
          disabled={!paisa}
          onClick={() =>
            paisa &&
            api
              .supplierOpening(supplier.id, paisa, note.trim() || undefined)
              .then(onSaved)
              .catch((e) => setError(describeError(e, lang)))
          }
        >
          {t('save')}
        </button>
      </div>
    </Modal>
  );
}
