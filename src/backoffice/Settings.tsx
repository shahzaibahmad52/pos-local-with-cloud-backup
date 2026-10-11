import { useEffect, useState } from 'react';
import { api } from '../api/client';
import type { CloudPage, Language, ReceiptLanguage, StoreHeader, StoreSettings, User, UserRow } from '../api/types';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';
import { PrintPortal } from '../print/DocView';
import { DEFAULT_POLICY, recoveryCodeDoc, type Doc } from '../print/doc';
import { printDoc, type PrinterSettings } from '../print/printer';
import { CloudBackup } from './Cloud';
import { FbrCard } from './Fbr';
import { Modal } from './parts';

export function Settings({
  user,
  printer,
  onStoreChange,
  onCloudChange,
}: {
  user: User;
  printer: PrinterSettings;
  onStoreChange: (s: StoreHeader) => void;
  onCloudChange?: (page: CloudPage) => void;
}) {
  const { t, lang } = useI18n();
  const [settings, setSettings] = useState<StoreSettings | null>(null);
  const [users, setUsers] = useState<UserRow[]>([]);
  const [editing, setEditing] = useState<UserRow | 'new' | null>(null);
  const [code, setCode] = useState<string | null>(null);
  const [printing, setPrinting] = useState<Doc | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    Promise.all([api.settings(), api.users()])
      .then(([s, u]) => {
        setSettings(s);
        setUsers(u);
      })
      .catch((e) => setError(describeError(e, lang)));
  }, [lang]);

  const makeCode = async () => {
    try {
      const { code } = await api.newRecoveryCode();
      setCode(code);
      setSettings((s) => (s ? { ...s, hasRecoveryCode: true } : s));
    } catch (e) {
      setError(describeError(e, lang));
    }
  };

  const printCode = async () => {
    if (!code || !settings) return;
    const doc = recoveryCodeDoc(settings.store, code, settings.store.receiptLanguage);
    setPrinting(doc);
    try {
      await printDoc(doc, printer);
    } catch (e) {
      setError(t('printFailed', { msg: String(e) }));
    }
  };

  if (!settings) return <div className="muted">{error ?? t('loading')}</div>;

  return (
    <>
      <div className="bo-head">
        <h1>{t('setTitle')}</h1>
        <span className="small muted">{t('appVersion', { v: settings.version })}</span>
      </div>
      {error && <div className="alert alert-bad">{error}</div>}

      <div className="cols">
        <div className="grow" style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
          <StoreForm
            store={settings.store}
            onSaved={(store) => {
              setSettings({ ...settings, store });
              onStoreChange(store);
            }}
          />

          <section className="card">
            <div className="card-head">
              <h2>{t('usersTitle')}</h2>
              <button type="button" className="btn" style={{ height: 40 }} onClick={() => setEditing('new')}>
                {t('addUser')}
              </button>
            </div>
            <div className="tbl-scroll">
              <table className="tbl" style={{ minWidth: 560 }}>
                <thead>
                  <tr>
                    <th>{t('userName')}</th>
                    <th>{t('userRole')}</th>
                    <th>{t('userLanguage')}</th>
                    <th>{t('userActive')}</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {users.map((u) => (
                    <tr key={u.id}>
                      <td>
                        <strong>{u.name}</strong>
                        {u.id === user.id && <span className="small muted"> ·  ✓</span>}
                      </td>
                      <td className="small">
                        {u.role === 'admin' ? t('roleAdmin') : t('roleCashier')}
                        {u.role === 'cashier' && u.canApprove && <span className="tag tag-ok" style={{ marginInlineStart: 6 }}>PIN ✓</span>}
                      </td>
                      <td className="small">{u.language === 'ur' ? 'اردو' : 'English'}</td>
                      <td>{u.active ? <span className="tag tag-ok">✓</span> : <span className="tag tag-mute">{t('inactiveChip')}</span>}</td>
                      <td style={{ width: 100 }}>
                        <button type="button" className="btn" style={{ height: 34 }} onClick={() => setEditing(u)}>
                          {t('editBtn')}
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>

          <section className="card" aria-label={t('addonsTitle')}>
            <h2>{t('addonsTitle')}</h2>
            <span className="small muted">{t('addonsHelp')}</span>
            {(
              [
                ['addonSms', 'addonSmsFree'],
                ['addonWa', 'addonWaFree'],
              ] as const
            ).map(([title, free]) => (
              <div key={title} className="box box-pad" style={{ gap: 6 }}>
                <div className="row">
                  <strong>{t(title)}</strong>
                  <button type="button" role="switch" aria-checked="false" aria-label={t(title)} className="btn" style={{ height: 34 }} disabled>
                    {t('offLabel')}
                  </button>
                </div>
                <span className="small">{t(free)}</span>
              </div>
            ))}
            <span className="tiny muted">{t('addonLater')}</span>
          </section>
          <FbrCard />
        </div>

        <div className="aside" style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
          <section className="card">
            <h2>{t('recoveryTitle')}</h2>
            <span className="small">{t('recoveryHelp')}</span>
            <span className={`tag ${settings.hasRecoveryCode ? 'tag-ok' : 'tag-warn'}`} style={{ alignSelf: 'flex-start' }}>
              {settings.hasRecoveryCode ? t('recoveryHave') : t('recoveryNone')}
            </span>
            {code && (
              <>
                <div className="code-box" aria-live="polite">{code}</div>
                <div className="alert alert-warn small">{t('recoveryShow')}</div>
                <button type="button" className="btn" onClick={() => void printCode()}>
                  {t('printCode')}
                </button>
              </>
            )}
            <button type="button" className="btn btn-primary" onClick={() => void makeCode()}>
              {t('recoveryNew')}
            </button>
          </section>
          <CloudBackup onChange={onCloudChange} />
        </div>
      </div>

      {editing && (
        <UserDialog
          row={editing === 'new' ? null : editing}
          onClose={() => setEditing(null)}
          onSaved={(rows) => {
            setUsers(rows);
            setEditing(null);
          }}
        />
      )}
      <PrintPortal doc={printing} />
    </>
  );
}

function StoreForm({ store, onSaved }: { store: StoreHeader; onSaved: (s: StoreHeader) => void }) {
  const { t, lang } = useI18n();
  const [f, setF] = useState({
    name: store.name,
    address: store.address ?? '',
    phone: store.phone ?? '',
    ntn: store.ntn ?? '',
    drugLicense: store.drugLicense ?? '',
    receiptFooter: store.receiptFooter ?? '',
    receiptFooterUr: store.receiptFooterUr ?? '',
    // Until the store writes its own, the standard wording is what prints.
    receiptPolicy: store.receiptPolicy ?? DEFAULT_POLICY.en,
    receiptPolicyUr: store.receiptPolicyUr ?? DEFAULT_POLICY.ur,
    receiptLanguage: store.receiptLanguage,
  });
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const set = (p: Partial<typeof f>) => setF({ ...f, ...p });

  const save = async () => {
    try {
      onSaved(await api.updateStore(f));
      setMessage({ ok: true, text: t('saved') });
    } catch (e) {
      setMessage({ ok: false, text: describeError(e, lang) });
    }
  };

  const text = (key: keyof typeof f, label: string, ltr = false) => (
    <div className="field">
      <label className="label" htmlFor={`st-${key}`}>{label}</label>
      <input id={`st-${key}`} className={`input${ltr ? ' ltr' : ''}`} value={f[key]} onChange={(e) => set({ [key]: e.target.value })} />
    </div>
  );

  return (
    <section className="card">
      <h2>{t('storeDetails')}</h2>
      <span className="small muted">{t('storeCodeFixed', { code: store.code })}</span>
      <div className="form-grid">
        {text('name', t('storeName'))}
        {text('phone', t('phone'), true)}
        {text('ntn', t('ntn'), true)}
        {text('drugLicense', t('drugLicense'), true)}
      </div>
      {text('address', t('address'))}
      <h3 style={{ margin: '8px 0 0', fontSize: 16 }}>{t('receiptTitle')}</h3>
      <div className="form-grid">
        {text('receiptFooter', t('footerEn'))}
        <div className="field">
          <label className="label" htmlFor="st-footer-ur">{t('footerUr')}</label>
          <input id="st-footer-ur" className="input" lang="ur" dir="rtl" value={f.receiptFooterUr} onChange={(e) => set({ receiptFooterUr: e.target.value })} />
        </div>
        <div className="field" style={{ gridColumn: '1 / -1' }}>
          <label className="label" htmlFor="st-policy">{t('policyEn')}</label>
          <textarea id="st-policy" className="input" rows={2} style={{ height: 'auto', padding: 10 }} value={f.receiptPolicy} onChange={(e) => set({ receiptPolicy: e.target.value })} />
        </div>
        <div className="field" style={{ gridColumn: '1 / -1' }}>
          <label className="label" htmlFor="st-policy-ur">{t('policyUr')}</label>
          <textarea id="st-policy-ur" className="input" lang="ur" dir="rtl" rows={2} style={{ height: 'auto', padding: 10 }} value={f.receiptPolicyUr} onChange={(e) => set({ receiptPolicyUr: e.target.value })} />
          <span className="help">{t('policyHelp')}</span>
        </div>
        <div className="field">
          <label className="label" htmlFor="st-rlang">{t('defaultReceiptLang')}</label>
          <select id="st-rlang" className="select" value={f.receiptLanguage} onChange={(e) => set({ receiptLanguage: e.target.value as ReceiptLanguage })}>
            <option value="en">{t('langEnglish')}</option>
            <option value="ur">{t('langUrdu')}</option>
            <option value="both">{t('langBoth')}</option>
          </select>
          <span className="help">{t('customerOverrides')}</span>
        </div>
      </div>
      {message && <div className={`alert ${message.ok ? 'alert-ok' : 'alert-bad'}`} role="status">{message.text}</div>}
      <button type="button" className="btn btn-primary" style={{ alignSelf: 'flex-start' }} disabled={!f.name.trim()} onClick={() => void save()}>
        {t('save')}
      </button>
    </section>
  );
}

function UserDialog({ row, onClose, onSaved }: { row: UserRow | null; onClose: () => void; onSaved: (rows: UserRow[]) => void }) {
  const { t, lang } = useI18n();
  const [name, setName] = useState(row?.name ?? '');
  const [role, setRole] = useState<'admin' | 'cashier'>(row?.role ?? 'cashier');
  const [canApprove, setCanApprove] = useState(row?.canApprove ?? false);
  const [language, setLanguage] = useState<Language>(row?.language ?? 'en');
  const [active, setActive] = useState(row?.active ?? true);
  const [secret, setSecret] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const save = async () => {
    setBusy(true);
    try {
      if (row) await api.updateUser(row.id, { name, canApprove, language, active, secret: secret || undefined });
      else await api.createUser({ name, role, secret, canApprove, language });
      onSaved(await api.users());
    } catch (e) {
      setError(describeError(e, lang));
      setBusy(false);
    }
  };

  const isCashier = (row?.role ?? role) === 'cashier';
  return (
    <Modal title={row ? row.name : t('addUser')} onClose={onClose}>
      <div className="field">
        <label className="label" htmlFor="us-name">{t('userName')}</label>
        <input id="us-name" className="input" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </div>
      {!row && (
        <div className="seg" role="group" aria-label={t('userRole')}>
          <button type="button" aria-pressed={role === 'cashier'} onClick={() => setRole('cashier')}>
            {t('roleCashier')}
          </button>
          <button type="button" aria-pressed={role === 'admin'} onClick={() => setRole('admin')}>
            {t('roleAdmin')}
          </button>
        </div>
      )}
      <div className="field">
        <label className="label" htmlFor="us-secret">{isCashier ? t('newPin') : t('newPassword')}</label>
        <input
          id="us-secret"
          className="input ltr"
          type="password"
          inputMode={isCashier ? 'numeric' : undefined}
          maxLength={isCashier ? 4 : undefined}
          autoComplete="new-password"
          value={secret}
          onChange={(e) => setSecret(isCashier ? e.target.value.replace(/\D/g, '') : e.target.value)}
        />
        {row && <span className="help">{t('pinKeep')}</span>}
      </div>
      <div className="field">
        <label className="label" htmlFor="us-lang">{t('userLanguage')}</label>
        <select id="us-lang" className="select" value={language} onChange={(e) => setLanguage(e.target.value as Language)}>
          <option value="en">English</option>
          <option value="ur">اردو</option>
        </select>
      </div>
      {isCashier && (
        <label className="check">
          <input type="checkbox" checked={canApprove} onChange={(e) => setCanApprove(e.target.checked)} />
          {t('canApprove')}
        </label>
      )}
      {row && (
        <label className="check">
          <input type="checkbox" checked={active} onChange={(e) => setActive(e.target.checked)} />
          {t('userActive')}
        </label>
      )}
      {error && <div className="alert alert-bad">{error}</div>}
      <div style={{ display: 'flex', gap: 10 }}>
        <button type="button" className="btn" style={{ flex: 1 }} onClick={onClose}>
          {t('cancel')}
        </button>
        <button type="button" className="btn btn-primary" style={{ flex: 2 }} disabled={busy || !name.trim() || (!row && !secret)} onClick={() => void save()}>
          {t('saveUser')}
        </button>
      </div>
    </Modal>
  );
}
