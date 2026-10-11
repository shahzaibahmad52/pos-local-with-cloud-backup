import { useEffect, useState } from 'react';
import { api } from '../api/client';
import type { ActivityEntry, ApprovalRules, Language, StoreHeader } from '../api/types';
import { BrandMark } from '../components/common';
import { formatRs, parseRupees } from '../domain/money';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';
import type { StringKey } from '../i18n/strings';

const ACTIONS: Record<string, [string, string]> = {
  'sale.approved': ['Sale approved (discount / khata)', 'سیل کی اجازت (رعایت / کھاتہ)'],
  'sale.offline': ['Offline sale received from a till', 'ٹل سے آف لائن سیل وصول'],
  'return.create': ['Refund', 'ریفنڈ'],
  'shift.open': ['Shift opened', 'شفٹ شروع'],
  'shift.count': ['Cash counted', 'نقد گنا گیا'],
  'shift.close': ['Shift closed', 'شفٹ بند'],
  'cash.in': ['Cash put in drawer', 'دراز میں نقد رکھا'],
  'cash.out': ['Cash taken out', 'دراز سے نقد نکالا'],
  'till.remove_item': ['Item removed from bill', 'بل سے آئٹم ہٹایا'],
  'till.cancel_bill': ['Bill cancelled', 'بل منسوخ'],
  'till.no_sale_drawer': ['Drawer opened, no sale', 'بغیر سیل دراز کھولی'],
  'auth.locked': ['Locked after wrong PINs', 'غلط PIN کے بعد لاک'],
  'product.create': ['Product added', 'پروڈکٹ شامل'],
  'rules.update': ['Approval rules changed', 'اصول تبدیل'],
  'store.setup': ['Store set up', 'اسٹور سیٹ اپ'],
  'user.create': ['User added', 'صارف شامل'],
  'user.update': ['User changed', 'صارف میں تبدیلی'],
  'product.update': ['Product changed', 'پروڈکٹ میں تبدیلی'],
  'product.import': ['Products imported from Excel', 'Excel سے پروڈکٹس'],
  'stock.adjust': ['Stock corrected', 'اسٹاک درست کیا'],
  'purchase.receive': ['Purchase received', 'خریداری وصول'],
  'supplier.create': ['Supplier added', 'سپلائر شامل'],
  'supplier.update': ['Supplier changed', 'سپلائر میں تبدیلی'],
  'supplier.pay': ['Supplier paid', 'سپلائر کو ادائیگی'],
  'supplier.opening': ['Supplier old balance', 'سپلائر کا پرانا بقایا'],
  'expense.add': ['Expense', 'خرچ'],
  'expense.delete': ['Expense deleted', 'خرچ حذف'],
  'khata.payment': ['Khata payment received', 'کھاتے کی وصولی'],
  'khata.opening': ['Khata old balance added', 'کھاتے کا پرانا بقایا'],
  'customer.update': ['Customer changed', 'گاہک میں تبدیلی'],
  'store.update': ['Store details changed', 'اسٹور کی معلومات تبدیل'],
  'count.start': ['Stock count started', 'اسٹاک گنتی شروع'],
  'count.apply': ['Stock count applied', 'اسٹاک گنتی لاگو'],
  'count.cancel': ['Stock count cancelled', 'اسٹاک گنتی منسوخ'],
  'transfer.send': ['Stock sent to branch', 'برانچ کو اسٹاک بھیجا'],
  'transfer.receive': ['Stock received from branch', 'برانچ سے اسٹاک وصول'],
  'fbr.settings': ['FBR settings changed', 'FBR سیٹنگز تبدیل'],
  'recovery.new': ['New recovery code made', 'نیا ریکوری کوڈ'],
  'recovery.used': ['Password reset with recovery code', 'ریکوری کوڈ سے پاس ورڈ ری سیٹ'],
};

const MONEY_KEYS = ['amount', 'refund', 'counted', 'expected', 'difference', 'openingCash', 'khata', 'net', 'paidNow', 'costChange', 'short', 'extra', 'total'];

function actionLabel(action: string, lang: Language): string {
  const pair = ACTIONS[action];
  return pair ? pair[lang === 'ur' ? 1 : 0] : action;
}

function detailText(details: Record<string, unknown> | null): string {
  if (!details) return '';
  return Object.entries(details)
    .filter(([k, v]) => v !== null && v !== undefined && v !== '' && !/Id$/.test(k))
    .map(([k, v]) => `${k}: ${MONEY_KEYS.includes(k) && typeof v === 'number' ? formatRs(v) : String(v)}`)
    .join(' · ');
}

function today(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

const FILTERS: [string, StringKey][] = [
  ['all', 'filterAll'],
  ['attention', 'filterAttention'],
  ['approvals', 'filterApprovals'],
  ['money', 'filterMoney'],
  ['stock', 'filterStock'],
];

export function Activity({ store, onBack, backLabel }: { store: StoreHeader; onBack: () => void; backLabel?: string }) {
  const { t, lang } = useI18n();
  const [date, setDate] = useState(today);
  const [filter, setFilter] = useState('all');
  const [entries, setEntries] = useState<ActivityEntry[] | null>(null);
  const [all, setAll] = useState<ActivityEntry[]>([]);
  const [rules, setRules] = useState<ApprovalRules | null>(null);
  const [refundText, setRefundText] = useState('');
  const [discountText, setDiscountText] = useState('');
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    setEntries(null);
    Promise.all([api.activity(date, filter), api.activity(date, 'all')])
      .then(([shown, everything]) => {
        setEntries(shown);
        setAll(everything);
      })
      .catch((e) => setMessage({ ok: false, text: describeError(e, lang) }));
  }, [date, filter, lang]);

  useEffect(() => {
    api
      .rules()
      .then((r) => {
        setRules(r);
        setRefundText(String(r.refundApprovalAbove / 100));
        setDiscountText(String(r.cashierMaxDiscountBp / 100));
      })
      .catch((e) => setMessage({ ok: false, text: describeError(e, lang) }));
  }, [lang]);

  const saveRules = async () => {
    if (!rules) return;
    const refund = parseRupees(refundText);
    if (refund === null) return setMessage({ ok: false, text: t('ruleRefund') });
    // Percent with up to two decimals: 2.5 → 250 basis points.
    const pct = Number(discountText.trim());
    if (discountText.trim() === '' || !Number.isFinite(pct) || pct < 0 || pct > 100) {
      return setMessage({ ok: false, text: t('ruleDiscount') });
    }
    try {
      setRules(await api.saveRules({ ...rules, refundApprovalAbove: refund, cashierMaxDiscountBp: Math.round(pct * 100) }));
      setMessage({ ok: true, text: t('saved') });
    } catch (e) {
      setMessage({ ok: false, text: describeError(e, lang) });
    }
  };

  const approvals = all.filter((e) => e.approvedByName).length;
  const cancelled = all.filter((e) => e.action === 'till.cancel_bill').length;
  const difference = all
    .filter((e) => e.action === 'shift.close')
    .reduce((sum, e) => sum + Number((e.details as { difference?: number } | null)?.difference ?? 0), 0);
  const attention = all.filter((e) => e.flagged).length;

  const toggles: [keyof ApprovalRules, StringKey][] = [
    ['removeItem', 'ruleRemoveItem'],
    ['cancelBill', 'ruleCancelBill'],
    ['cashOut', 'ruleCashOut'],
    ['noSaleDrawer', 'ruleNoSale'],
  ];

  return (
    <div className="till app">
      <header className="till-head">
        <div className="who">
          <BrandMark size={18} />
          <div style={{ display: 'flex', flexDirection: 'column' }}>
            <strong>{t('activityTitle')}</strong>
            <span className="small" style={{ color: 'var(--night-muted)' }}>{store.name} · {t('activityHelp')}</span>
          </div>
        </div>
        <button type="button" className="head-btn" onClick={onBack}>{backLabel ?? t('backToTill')}</button>
      </header>

      <div className="till-body">
        <section className="bill">
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(170px, 1fr))', gap: 12 }}>
            {(
              [
                ['kpiApprovals', String(approvals), 'var(--ink)'],
                ['kpiCancelled', String(cancelled), 'var(--ink)'],
                ['kpiDifference', formatRs(difference), difference < 0 ? 'var(--bad)' : 'var(--ink)'],
                ['kpiAttention', String(attention), attention ? 'var(--warn)' : 'var(--ink)'],
              ] as [StringKey, string, string][]
            ).map(([k, v, color]) => (
              <div key={k} className="box box-pad" style={{ gap: 4 }}>
                <span className="small muted">{t(k)}</span>
                <span className="num" style={{ fontSize: 24, fontWeight: 600, color }}>{v}</span>
              </div>
            ))}
          </div>

          <div className="box" style={{ overflow: 'hidden' }}>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, padding: '12px 14px', borderBottom: '1px solid var(--line)', alignItems: 'center' }}>
              {FILTERS.map(([f, k]) => (
                <button
                  key={f}
                  type="button"
                  aria-pressed={filter === f}
                  onClick={() => setFilter(f)}
                  className="btn"
                  style={{ height: 36, borderRadius: 999, ...(filter === f ? { background: 'var(--night)', color: '#fff', borderColor: 'var(--night)' } : {}) }}
                >
                  {t(k)}
                </button>
              ))}
              <label className="visually-hidden" htmlFor="activity-date">Date</label>
              <input id="activity-date" type="date" className="input ltr" style={{ height: 40, marginInlineStart: 'auto' }} value={date} onChange={(e) => setDate(e.target.value)} />
            </div>
            <div className="cart-scroll">
              <div style={{ minWidth: 760 }}>
                <div className="cart-row cart-head" style={{ gridTemplateColumns: '70px 90px 100px minmax(240px, 1fr) 140px' }}>
                  <span>{t('colTime')}</span>
                  <span>{t('colWhere')}</span>
                  <span>{t('colUser')}</span>
                  <span>{t('colWhat')}</span>
                  <span>{t('colApprovedBy')}</span>
                </div>
                {entries === null && <div className="empty">{t('loading')}</div>}
                {entries?.length === 0 && <div className="empty">{t('noActivity')}</div>}
                {entries?.map((e) => (
                  <div key={e.id} className="cart-row" style={{ gridTemplateColumns: '70px 90px 100px minmax(240px, 1fr) 140px', alignItems: 'start' }}>
                    <span className="mono small muted">{e.createdAt.slice(11, 16)}</span>
                    <span className="small muted ltr">{e.tillCode ?? '—'}</span>
                    <strong className="small">{e.userName ?? '—'}</strong>
                    <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                      <span style={{ fontWeight: 600 }}>
                        {actionLabel(e.action, lang)} {e.flagged && <span className="chip chip-warn">{t('check')}</span>}
                      </span>
                      <span className="small muted ltr" style={{ textAlign: 'start' }}>{detailText(e.details)}</span>
                    </div>
                    <span className="small">{e.approvedByName ?? '—'}</span>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </section>

        <aside className="side">
          {rules && (
            <div className="box box-pad">
              <h2>{t('rulesTitle')}</h2>
              <span className="small muted">{t('rulesHelp')}</span>
              <div className="row">
                <label htmlFor="rule-discount">{t('ruleDiscount')}</label>
                <input
                  id="rule-discount"
                  className="input ltr mono"
                  style={{ width: 80, height: 40, textAlign: 'center' }}
                  inputMode="decimal"
                  value={discountText}
                  onChange={(e) => setDiscountText(e.target.value)}
                />
              </div>
              <div className="row">
                <label htmlFor="rule-refund">{t('ruleRefund')}</label>
                <input id="rule-refund" className="input ltr mono" style={{ width: 100, height: 40, textAlign: 'center' }} inputMode="numeric" value={refundText} onChange={(e) => setRefundText(e.target.value)} />
              </div>
              {toggles.map(([k, label]) => (
                <label key={k} className="row" style={{ justifyContent: 'flex-start', gap: 10, minHeight: 36 }}>
                  <input type="checkbox" checked={Boolean(rules[k])} onChange={(e) => setRules({ ...rules, [k]: e.target.checked })} />
                  {t(label)}
                </label>
              ))}
              {message && <div className={`alert ${message.ok ? 'alert-ok' : 'alert-bad'}`} role="status">{message.text}</div>}
              <button type="button" className="btn btn-primary" onClick={() => void saveRules()}>{t('saveRules')}</button>
            </div>
          )}
        </aside>
      </div>
    </div>
  );
}
