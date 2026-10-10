import { useEffect, useState } from 'react';
import { api } from '../api/client';
import type { KhataAccount, KhataList, KhataPayMethod, KhataPaymentSlip, KhataStatement, ReceiptLanguage, StoreHeader } from '../api/types';
import { SearchIcon } from '../components/common';
import { formatDate } from '../domain/dates';
import { formatAmount, formatRs, ltr, parseRupees } from '../domain/money';
import { waLink } from '../domain/whatsapp';
import { describeError } from '../i18n/errors';
import { translate, useI18n } from '../i18n/i18n';
import type { StringKey } from '../i18n/strings';
import { PrintPortal } from '../print/DocView';
import { khataPaymentDoc, khataStatementDoc, type Doc } from '../print/doc';
import { printDoc, type PrinterSettings } from '../print/printer';
import { openWhatsApp } from '../platform';
import { Modal, useOpenTills } from './parts';

type Tab = 'all' | 'balance' | 'overdue';

const METHODS: [KhataPayMethod, StringKey][] = [
  ['cash', 'cash'],
  ['jazzcash', 'jazzcash'],
  ['easypaisa', 'easypaisa'],
  ['bank', 'methodBank'],
];

export function Customers({ store, printer, tillCode }: { store: StoreHeader; printer: PrinterSettings; tillCode: string }) {
  const { t, lang } = useI18n();
  const [tab, setTab] = useState<Tab>('balance');
  const [query, setQuery] = useState('');
  const [list, setList] = useState<KhataList | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [statement, setStatement] = useState<KhataStatement | null>(null);
  const [dialog, setDialog] = useState<'pay' | 'edit' | 'new' | 'opening' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [printing, setPrinting] = useState<Doc | null>(null);

  const load = () =>
    api
      .khata({ q: query, tab })
      .then((l) => {
        setList(l);
        setSelected((cur) => cur ?? l.accounts[0]?.id ?? null);
      })
      .catch((e) => setError(describeError(e, lang)));

  useEffect(() => {
    const timer = setTimeout(() => void load(), query ? 250 : 0);
    return () => clearTimeout(timer);
  }, [query, tab, lang]);

  const loadStatement = (id: string) =>
    api
      .khataStatement(id)
      .then(setStatement)
      .catch((e) => setError(describeError(e, lang)));

  useEffect(() => {
    if (selected) void loadStatement(selected);
    else setStatement(null);
  }, [selected, lang]);

  const acc = statement?.account;
  const receiptLang = (a: KhataAccount): ReceiptLanguage => a.receiptLanguage ?? store.receiptLanguage;

  const print = async (doc: Doc) => {
    setPrinting(doc);
    try {
      await printDoc(doc, printer);
    } catch (e) {
      setError(t('printFailed', { msg: String(e) }));
    }
  };

  const remind = async (a: KhataAccount) => {
    const textLang = receiptLang(a) === 'ur' ? 'ur' : 'en';
    const text = translate(textLang, 'waText', { name: a.name, store: store.name, amount: formatAmount(a.balance) });
    const url = a.phone ? waLink(a.phone, text) : null;
    if (!url) return setError(t('needPhone'));
    try {
      await openWhatsApp(url);
    } catch (e) {
      setError(String(e));
    }
  };

  const refresh = async (id: string, text?: string) => {
    setDialog(null);
    setMessage(text ?? null);
    setSelected(id);
    await Promise.all([load(), loadStatement(id)]);
  };

  const kindText = (k: KhataStatement['lines'][number]) => {
    switch (k.kind) {
      case 'sale':
        return t('kindSale', { ref: k.reference ?? '' });
      case 'return':
        return t('kindReturn', { ref: k.reference ?? '' });
      case 'opening':
        return k.note ?? t('kindOpening');
      default:
        return t('kindPayment', { method: t(METHODS.find(([m]) => m === k.kind)?.[1] ?? 'cash') }) + (k.note ? ` · ${k.note}` : '');
    }
  };

  const s = list?.summary;
  const pct = acc && acc.creditLimit > 0 ? Math.min(100, Math.round((Math.max(0, acc.balance) / acc.creditLimit) * 100)) : 0;

  return (
    <>
      <div className="bo-head">
        <div>
          <h1>{t('khataTitle')}</h1>
          <span className="sub">
            {store.name} · {t('khataSub')}
          </span>
        </div>
        <button type="button" className="btn btn-primary" onClick={() => setDialog('new')}>
          {t('addCustomer')}
        </button>
      </div>

      {s && (
        <div className="kpis">
          <div className="kpi">
            <span className="label">{t('totalCollect')}</span>
            <span className="value">{formatRs(s.totalDue)}</span>
            <span className="sub">{t('owingN', { n: s.owingCount })}</span>
          </div>
          <div className="kpi">
            <span className="label">{t('overdue30')}</span>
            <span className="value" style={{ color: s.overdueTotal ? 'var(--bad)' : undefined }}>{formatRs(s.overdueTotal)}</span>
            <span className="sub">{t('customersN', { n: s.overdueCount })}</span>
          </div>
          <div className="kpi">
            <span className="label">{t('collectedMonth')}</span>
            <span className="value">{formatRs(s.collectedThisMonth)}</span>
            <span className="sub">{t('collectedSub')}</span>
          </div>
          <div className="kpi">
            <span className="label">{t('nearLimit')}</span>
            <span className="value">{s.nearLimitCount}</span>
            <span className="sub">{t('nearLimitSub')}</span>
          </div>
        </div>
      )}

      {message && <div className="alert alert-ok" role="status">{message}</div>}
      {error && (
        <div className="alert alert-bad" role="alert" onClick={() => setError(null)}>
          {error}
        </div>
      )}

      <div className="cols">
        <section className="card grow" aria-label={t('khataTitle')} style={{ flexBasis: 360 }}>
          <div className="pills" role="group">
            {(
              [
                ['all', 'tabAllCustomers'],
                ['balance', 'tabHasBalance'],
                ['overdue', 'tabOverdue'],
              ] as [Tab, StringKey][]
            ).map(([k, label]) => (
              <button key={k} type="button" aria-pressed={tab === k} onClick={() => setTab(k)}>
                {t(label)}
              </button>
            ))}
          </div>
          <div className="searchbar" style={{ height: 46 }}>
            <SearchIcon />
            <label className="visually-hidden" htmlFor="kh-q">{t('searchCustomers')}</label>
            <input id="kh-q" placeholder={t('searchCustomers')} value={query} onChange={(e) => setQuery(e.target.value)} />
          </div>
          {list?.accounts.length === 0 && <span className="muted">{t('noCustomers')}</span>}
          <div>
            {list?.accounts.map((a) => (
              <button
                key={a.id}
                type="button"
                className="list-row"
                aria-pressed={a.id === selected}
                style={{
                  border: 0,
                  borderBottom: '1px solid var(--line-2)',
                  background: a.id === selected ? 'var(--brand-soft)' : 'transparent',
                  textAlign: 'start',
                  width: '100%',
                  padding: '10px 8px',
                }}
                onClick={() => {
                  setSelected(a.id);
                  setMessage(null);
                }}
              >
                <div className="main">
                  <strong>
                    {a.name} {!a.active && <span className="tag tag-mute">{t('inactiveChip')}</span>}
                  </strong>
                  <span className="small muted">
                    {a.phone ? <span className="ltr">{a.phone}</span> : null}
                    {a.phone ? ' · ' : ''}
                    {a.lastPaid ? t('lastPaidOn', { date: ltr(formatDate(a.lastPaid)) }) : t('neverPaid')}
                  </span>
                </div>
                <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-end', gap: 2 }}>
                  <span className="mono">{formatRs(a.balance)}</span>
                  {a.balance > 0 ? (
                    <span className={`tag ${a.overdue ? 'tag-bad' : 'tag-mute'}`}>{t('daysN', { n: a.ageDays })}</span>
                  ) : (
                    <span className="tag tag-ok">{t('settledChip')}</span>
                  )}
                </div>
              </button>
            ))}
          </div>
        </section>

        <section className="card aside" aria-label={t('ledgerTitle')} style={{ flex: '3 1 480px' }}>
          {!acc && <span className="muted">{t('pickCustomer')}</span>}
          {acc && statement && (
            <>
              <div className="card-head">
                <div style={{ display: 'flex', flexDirection: 'column' }}>
                  <h2 style={{ fontSize: 20 }}>{acc.name}</h2>
                  <span className="small muted">
                    {acc.phone && <span className="ltr">{acc.phone}</span>}
                    {acc.notes ? ` · ${acc.notes}` : ''}
                  </span>
                </div>
                <div style={{ textAlign: 'end' }}>
                  <span className="small muted">{t('balanceDue')}</span>
                  <div className="total-amount" style={{ fontSize: 28, color: acc.balance > 0 ? 'var(--bad)' : 'var(--brand)' }}>
                    {formatRs(acc.balance)}
                  </div>
                </div>
              </div>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                <div className="stat">
                  <span>{t('creditLimit')}</span>
                  <span>{acc.creditLimit > 0 ? t('limitUsed', { used: ltr(formatRs(Math.max(0, acc.balance))), limit: ltr(formatRs(acc.creditLimit)) }) : t('noKhataAllowed')}</span>
                </div>
                {acc.creditLimit > 0 && (
                  <div className={`meter${pct >= 80 ? ' warn' : ''}`} role="img" aria-label={`${pct}%`}>
                    <span style={{ width: `${pct}%` }} />
                  </div>
                )}
              </div>
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
                <button type="button" className="btn btn-primary" disabled={acc.balance <= 0} onClick={() => setDialog('pay')}>
                  {t('receivePayment')}
                </button>
                <button type="button" className="btn" disabled={acc.balance <= 0} onClick={() => void remind(acc)} title={acc.phone ? undefined : t('needPhone')}>
                  {t('waReminder')}
                </button>
                <button type="button" className="btn" onClick={() => void print(khataStatementDoc(statement, receiptLang(acc)))}>
                  {t('printStatement')}
                </button>
                <button type="button" className="btn" onClick={() => setDialog('edit')}>
                  {t('editBtn')}
                </button>
                <button type="button" className="btn" onClick={() => setDialog('opening')}>
                  {t('oldBalance')}
                </button>
              </div>
              <span className="tiny muted">{t('waNote')}</span>
              <div className="tbl-scroll">
                <table className="tbl" style={{ minWidth: 520 }}>
                  <thead>
                    <tr>
                      <th>{t('colDate')}</th>
                      <th>{t('colDetail')}</th>
                      <th className="num">{t('colUdhaarPlus')}</th>
                      <th className="num">{t('colPaidMinus')}</th>
                      <th className="num">{t('colBalance')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {statement.lines.map((l, i) => (
                      <tr key={i}>
                        <td className="small ltr" style={{ whiteSpace: 'nowrap' }}>{formatDate(l.createdAt)}</td>
                        <td className="small">{kindText(l)}</td>
                        <td className="num small">{l.amount > 0 ? formatAmount(l.amount) : ''}</td>
                        <td className="num small">{l.amount < 0 ? formatAmount(-l.amount) : ''}</td>
                        <td className="num small">{formatAmount(l.balance)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <span className="tiny muted">{t('khataAuto')}</span>
            </>
          )}
        </section>
      </div>

      {dialog === 'pay' && acc && (
        <PayDialog
          account={acc}
          tillCode={tillCode}
          onClose={() => setDialog(null)}
          onSaved={(slip) => {
            void refresh(acc.id, t('paymentReceived', { amount: formatRs(slip.amount), name: slip.customerName }));
            void print(khataPaymentDoc(slip, receiptLang(acc)));
          }}
        />
      )}
      {(dialog === 'edit' || dialog === 'new') && (
        <CustomerDialog account={dialog === 'edit' ? acc ?? null : null} onClose={() => setDialog(null)} onSaved={(id) => void refresh(id, t('customerSaved'))} />
      )}
      {dialog === 'opening' && acc && <OpeningDialog account={acc} onClose={() => setDialog(null)} onSaved={() => void refresh(acc.id, t('saved'))} />}
      <PrintPortal doc={printing} />
    </>
  );
}

function PayDialog({
  account,
  tillCode,
  onClose,
  onSaved,
}: {
  account: KhataAccount;
  tillCode: string;
  onClose: () => void;
  onSaved: (slip: KhataPaymentSlip) => void;
}) {
  const { t, lang } = useI18n();
  const tills = useOpenTills();
  const [amount, setAmount] = useState('');
  const [method, setMethod] = useState<KhataPayMethod>('cash');
  const [till, setTill] = useState<string>('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const paisa = parseRupees(amount);
  const drawer = method === 'cash' ? till || (tills.some((x) => x.tillCode === tillCode) ? tillCode : '') : '';

  const save = async () => {
    if (!paisa) return;
    setBusy(true);
    try {
      onSaved(await api.khataPayment(account.id, { amount: paisa, method, tillCode: drawer || undefined }));
    } catch (e) {
      setError(describeError(e, lang));
      setBusy(false);
    }
  };

  return (
    <Modal title={`${t('receivePayment')} · ${account.name}`} onClose={onClose}>
      <div className="stat">
        <span>{t('balanceDue')}</span>
        <strong className="mono">{formatRs(account.balance)}</strong>
      </div>
      <div className="field">
        <label className="label" htmlFor="kp-amt">{t('amountReceived')}</label>
        <div style={{ display: 'flex', gap: 8 }}>
          <input id="kp-amt" className="input big ltr" style={{ flex: 1 }} inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} autoFocus />
          <button type="button" className="btn" onClick={() => setAmount(String(account.balance / 100))}>
            {t('fullBalance')}
          </button>
        </div>
        {paisa ? <span className="help">{t('balanceAfter', { amount: formatRs(account.balance - paisa) })}</span> : null}
      </div>
      <div className="field">
        <span className="label">{t('paidBy')}</span>
        <div className="methods" style={{ gridTemplateColumns: 'repeat(4, minmax(0, 1fr))' }}>
          {METHODS.map(([m, k]) => (
            <button key={m} type="button" className="choice" aria-pressed={method === m} onClick={() => setMethod(m)}>
              {t(k)}
            </button>
          ))}
        </div>
      </div>
      {method === 'cash' && (
        <div className="field">
          <label className="label" htmlFor="kp-till">{t('cashGoesTo')}</label>
          <select id="kp-till" className="select" value={drawer} onChange={(e) => setTill(e.target.value)}>
            <option value="">{t('ownerKeeps')}</option>
            {tills.map((x) => (
              <option key={x.tillCode} value={x.tillCode}>
                {t('drawerOf', { till: x.tillCode, name: x.cashierName })}
              </option>
            ))}
          </select>
        </div>
      )}
      {error && <div className="alert alert-bad">{error}</div>}
      <div style={{ display: 'flex', gap: 10 }}>
        <button type="button" className="btn" style={{ flex: 1 }} onClick={onClose}>
          {t('cancel')}
        </button>
        <button type="button" className="btn btn-primary" style={{ flex: 2 }} disabled={busy || !paisa} onClick={() => void save()}>
          {t('savePrint')}
        </button>
      </div>
    </Modal>
  );
}

function CustomerDialog({ account, onClose, onSaved }: { account: KhataAccount | null; onClose: () => void; onSaved: (id: string) => void }) {
  const { t, lang } = useI18n();
  const [name, setName] = useState(account?.name ?? '');
  const [phone, setPhone] = useState(account?.phone ?? '');
  const [notes, setNotes] = useState(account?.notes ?? '');
  const [limit, setLimit] = useState(account ? String(account.creditLimit / 100) : '0');
  const [receiptLanguage, setReceiptLanguage] = useState<ReceiptLanguage | ''>(account?.receiptLanguage ?? '');
  const [active, setActive] = useState(account?.active ?? true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const save = async () => {
    const creditLimit = parseRupees(limit || '0');
    if (creditLimit === null) return setError(t('creditLimit'));
    setBusy(true);
    try {
      const input = {
        name,
        phone: phone.trim() || undefined,
        notes: notes.trim() || undefined,
        creditLimit,
        receiptLanguage: receiptLanguage || null,
        active,
      };
      if (account) {
        onSaved((await api.updateCustomer(account.id, input)).id);
      } else {
        const created = await api.createCustomerFull(input);
        if (notes.trim()) await api.updateCustomer(created.id, input);
        onSaved(created.id);
      }
    } catch (e) {
      setError(describeError(e, lang));
      setBusy(false);
    }
  };

  return (
    <Modal title={account ? account.name : t('addCustomer')} onClose={onClose}>
      <div className="field">
        <label className="label" htmlFor="cu-name">{t('userName')}</label>
        <input id="cu-name" className="input" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </div>
      <div className="field">
        <label className="label" htmlFor="cu-phone">{t('phone')}</label>
        <input id="cu-phone" className="input ltr" inputMode="tel" placeholder="03xx-xxxxxxx" value={phone} onChange={(e) => setPhone(e.target.value)} />
      </div>
      <div className="form-grid">
        <div className="field">
          <label className="label" htmlFor="cu-limit">{t('creditLimit')} (Rs)</label>
          <input id="cu-limit" className="input ltr" inputMode="decimal" value={limit} onChange={(e) => setLimit(e.target.value)} />
        </div>
        <div className="field">
          <label className="label" htmlFor="cu-lang">{t('receiptLanguage')}</label>
          <select id="cu-lang" className="select" value={receiptLanguage} onChange={(e) => setReceiptLanguage(e.target.value as ReceiptLanguage | '')}>
            <option value="">{t('langDefault')}</option>
            <option value="en">{t('langEnglish')}</option>
            <option value="ur">{t('langUrdu')}</option>
            <option value="both">{t('langBoth')}</option>
          </select>
        </div>
      </div>
      <div className="field">
        <label className="label" htmlFor="cu-notes">{t('notes')}</label>
        <input id="cu-notes" className="input" value={notes} onChange={(e) => setNotes(e.target.value)} />
      </div>
      {account && (
        <label className="check">
          <input type="checkbox" checked={active} onChange={(e) => setActive(e.target.checked)} />
          {t('activeCustomer')}
        </label>
      )}
      {error && <div className="alert alert-bad">{error}</div>}
      <div style={{ display: 'flex', gap: 10 }}>
        <button type="button" className="btn" style={{ flex: 1 }} onClick={onClose}>
          {t('cancel')}
        </button>
        <button type="button" className="btn btn-primary" style={{ flex: 2 }} disabled={busy || !name.trim()} onClick={() => void save()}>
          {t('saveCustomer')}
        </button>
      </div>
    </Modal>
  );
}

function OpeningDialog({ account, onClose, onSaved }: { account: KhataAccount; onClose: () => void; onSaved: () => void }) {
  const { t, lang } = useI18n();
  const [amount, setAmount] = useState('');
  const [note, setNote] = useState('');
  const [error, setError] = useState<string | null>(null);
  const paisa = parseRupees(amount);
  return (
    <Modal title={`${t('oldBalance')} · ${account.name}`} onClose={onClose}>
      <span className="help">{t('oldBalanceHelp')}</span>
      <div className="field">
        <label className="label" htmlFor="ko-amt">{t('amountField')}</label>
        <input id="ko-amt" className="input big ltr" inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} autoFocus />
      </div>
      <div className="field">
        <label className="label" htmlFor="ko-note">{t('noteOptional')}</label>
        <input id="ko-note" className="input" value={note} onChange={(e) => setNote(e.target.value)} />
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
              .khataOpening(account.id, paisa, note.trim() || undefined)
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
