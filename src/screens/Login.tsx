import { useEffect, useState, type FormEvent } from 'react';
import { api, setToken } from '../api/client';
import type { Language, LoginTile, StoreHeader, User } from '../api/types';
import { BrandMark, LanguageToggle } from '../components/common';
import { PinPad } from '../components/PinPad';
import { describeError } from '../i18n/errors';
import { useI18n } from '../i18n/i18n';

export function Login({
  store,
  tillCode,
  onSignedIn,
}: {
  store: StoreHeader;
  tillCode: string;
  onSignedIn: (user: User) => void;
}) {
  const { t, lang, setLang } = useI18n();
  const [tiles, setTiles] = useState<LoginTile[] | null>(null);
  const [selected, setSelected] = useState<LoginTile | null>(null);
  const [pin, setPin] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [recovering, setRecovering] = useState(false);
  const [code, setCode] = useState('');
  const [newPassword, setNewPassword] = useState('');

  useEffect(() => {
    api
      .loginTiles()
      .then((list) => {
        setTiles(list);
        const first = list.find((u) => u.role === 'cashier') ?? list[0] ?? null;
        setSelected(first);
        if (first) setLang(first.language);
      })
      .catch((e) => setError(describeError(e, lang)));
  }, [setLang]);

  const pick = (tile: LoginTile) => {
    setSelected(tile);
    setRecovering(false);
    setPin('');
    setPassword('');
    setError(null);
    setLang(tile.language);
  };

  const changeLanguage = (next: Language) => {
    setLang(next);
    if (selected) setSelected({ ...selected, language: next });
  };

  const signIn = async (secret: string) => {
    if (!selected || busy) return;
    setBusy(true);
    setError(null);
    try {
      const res = await api.login(selected.id, secret);
      setToken(res.token);
      let user = res.user;
      if (user.language !== lang) {
        await api.setLanguage(lang);
        user = { ...user, language: lang };
      }
      onSignedIn(user);
    } catch (e) {
      setError(describeError(e, lang));
      setPin('');
      setBusy(false);
    }
  };

  const submitPassword = (e: FormEvent) => {
    e.preventDefault();
    void signIn(password);
  };

  const isAdmin = selected?.role === 'admin';

  const recover = async (e: FormEvent) => {
    e.preventDefault();
    if (!selected || busy) return;
    setBusy(true);
    setError(null);
    try {
      const res = await api.recover({ userId: selected.id, code, newPassword });
      setToken(res.token);
      window.alert(t('recoverDone'));
      onSignedIn(res.user);
    } catch (err) {
      setError(describeError(err, lang));
      setBusy(false);
    }
  };

  return (
    <div className="split">
      <aside className="split-side">
        <div className="brand">
          <BrandMark />
          <div style={{ display: 'flex', flexDirection: 'column' }}>
            <span className="brand-name">{store.name}</span>
            <span className="side-muted">{t('tillCode')}: <span className="ltr">{tillCode}</span></span>
          </div>
        </div>
        <p className="side-muted">{t('langNote')}</p>
      </aside>
      <main className="split-main">
        <div className="panel">
          <div className="row" style={{ flexWrap: 'wrap' }}>
            <h1>{t('signIn')}</h1>
            <LanguageToggle value={lang} onChange={changeLanguage} />
          </div>
          <div className="field">
            <span className="label">{t('who')}</span>
            {!tiles && !error && <span className="muted">{t('loading')}</span>}
            <div className="tiles">
              {tiles?.map((tile) => (
                <button key={tile.id} type="button" className="tile" aria-pressed={selected?.id === tile.id} onClick={() => pick(tile)}>
                  <span className="tile-name">{tile.name}</span>
                  <span className="tile-sub">
                    {tile.role === 'admin' ? t('owner') : t('cashier')} · {tile.language === 'ur' ? 'اردو' : 'English'}
                  </span>
                </button>
              ))}
            </div>
          </div>

          {selected && !isAdmin && (
            <div className="field">
              <span className="label">{t('enterPin')}</span>
              <PinPad value={pin} onChange={setPin} onComplete={(p) => void signIn(p)} />
            </div>
          )}

          {selected && isAdmin && !recovering && (
            <form className="field" onSubmit={submitPassword}>
              <label className="label" htmlFor="pw">{t('enterPassword')}</label>
              <input id="pw" type="password" className="input ltr" value={password} onChange={(e) => setPassword(e.target.value)} autoFocus autoComplete="current-password" />
              <button type="submit" className="btn btn-dark btn-lg" disabled={busy || password.length === 0}>{t('signIn')}</button>
              <button type="button" className="btn" style={{ border: 0, background: 'none', color: 'var(--brand)', height: 36 }} onClick={() => { setRecovering(true); setError(null); }}>
                {t('forgotPassword')}
              </button>
            </form>
          )}

          {selected && isAdmin && recovering && (
            <form className="field" onSubmit={(e) => void recover(e)}>
              <h2>{t('recoverTitle')}</h2>
              <span className="help">{t('recoverHelp')}</span>
              <label className="label" htmlFor="rc-code">{t('recoveryCode')}</label>
              <input id="rc-code" className="input mono ltr" placeholder="XXXX-XXXX-XXXX" value={code} onChange={(e) => setCode(e.target.value)} autoFocus autoComplete="off" />
              <label className="label" htmlFor="rc-pw">{t('newPassword')}</label>
              <input id="rc-pw" type="password" className="input ltr" value={newPassword} onChange={(e) => setNewPassword(e.target.value)} autoComplete="new-password" />
              <button type="submit" className="btn btn-dark btn-lg" disabled={busy || !code.trim() || newPassword.length < 8}>{t('setNewPassword')}</button>
              <button type="button" className="btn" onClick={() => setRecovering(false)}>{t('back')}</button>
            </form>
          )}

          {error && <div className="alert alert-bad" role="alert">{error}</div>}
        </div>
      </main>
    </div>
  );
}
