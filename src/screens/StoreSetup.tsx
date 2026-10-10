import { useState, type FormEvent } from 'react';
import { api, ApiError, setToken } from '../api/client';
import type { User } from '../api/types';
import { BrandMark, LanguageToggle } from '../components/common';
import { useI18n } from '../i18n/i18n';

/** Shown once, when the main PC's database is empty. */
export function StoreSetup({ onDone }: { onDone: (user: User) => void }) {
  const { t, lang, setLang } = useI18n();
  const [form, setForm] = useState({ storeCode: 'B1', storeName: '', address: '', phone: '', ownerName: '', password: '', password2: '' });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const set = (k: keyof typeof form) => (e: { target: { value: string } }) => setForm({ ...form, [k]: e.target.value });

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    if (form.password !== form.password2) return setError(t('passwordMismatch'));
    setBusy(true);
    try {
      const res = await api.setup({
        storeCode: form.storeCode,
        storeName: form.storeName,
        address: form.address || undefined,
        phone: form.phone || undefined,
        ownerName: form.ownerName,
        ownerPassword: form.password,
      });
      setToken(res.token);
      onDone(res.user);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : String(err));
      setBusy(false);
    }
  };

  return (
    <div className="split">
      <aside className="split-side">
        <div className="brand">
          <BrandMark />
          <span className="brand-name">{t('appName')}</span>
        </div>
        <p className="side-muted">{t('setupIntro')}</p>
      </aside>
      <main className="split-main">
        <form className="panel wide" onSubmit={submit}>
          <div className="row" style={{ flexWrap: 'wrap' }}>
            <h1>{t('setupTitle')}</h1>
            <LanguageToggle value={lang} onChange={setLang} />
          </div>
          <div className="grid-2">
            <div className="field">
              <label className="label" htmlFor="code">{t('storeCode')}</label>
              <input id="code" className="input ltr" value={form.storeCode} onChange={set('storeCode')} maxLength={6} required />
              <span className="help">{t('storeCodeHelp')}</span>
            </div>
            <div className="field">
              <label className="label" htmlFor="sname">{t('storeName')}</label>
              <input id="sname" className="input" value={form.storeName} onChange={set('storeName')} required />
            </div>
            <div className="field">
              <label className="label" htmlFor="addr">{t('address')}</label>
              <input id="addr" className="input" value={form.address} onChange={set('address')} />
            </div>
            <div className="field">
              <label className="label" htmlFor="phone">{t('phone')}</label>
              <input id="phone" className="input ltr" value={form.phone} onChange={set('phone')} inputMode="tel" />
            </div>
            <div className="field">
              <label className="label" htmlFor="owner">{t('ownerName')}</label>
              <input id="owner" className="input" value={form.ownerName} onChange={set('ownerName')} required />
            </div>
          </div>
          <div className="grid-2">
            <div className="field">
              <label className="label" htmlFor="pw">{t('password')}</label>
              <input id="pw" type="password" className="input ltr" value={form.password} onChange={set('password')} minLength={8} required autoComplete="new-password" />
              <span className="help">{t('passwordHelp')}</span>
            </div>
            <div className="field">
              <label className="label" htmlFor="pw2">{t('passwordAgain')}</label>
              <input id="pw2" type="password" className="input ltr" value={form.password2} onChange={set('password2')} minLength={8} required autoComplete="new-password" />
            </div>
          </div>
          {error && <div className="alert alert-bad" role="alert">{error}</div>}
          <button type="submit" className="btn btn-primary btn-lg" disabled={busy}>{t('createStore')}</button>
        </form>
      </main>
    </div>
  );
}
