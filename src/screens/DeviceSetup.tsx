import { useState } from 'react';
import { BrandMark, LanguageToggle } from '../components/common';
import { useI18n } from '../i18n/i18n';
import { saveDeviceConfig } from '../runtime';

/** First start of the desktop app: is this the main PC or an extra till? */
export function DeviceSetup() {
  const { t, lang, setLang } = useI18n();
  const [mode, setMode] = useState<'main' | 'till'>('main');
  const [mainHost, setMainHost] = useState('');
  const [tillCode, setTillCode] = useState('T01');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const save = async () => {
    setError(null);
    const code = tillCode.trim().toUpperCase();
    if (!/^[A-Z0-9-]{1,12}$/.test(code)) return setError(t('tillCodeHelp'));
    if (mode === 'till' && !mainHost.trim()) return setError(t('mainAddressHelp'));
    setBusy(true);
    try {
      await saveDeviceConfig({ mode, mainHost: mode === 'till' ? mainHost.trim() : null, port: null, tillCode: code });
    } catch (e) {
      setError(String(e));
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
        <p className="side-muted">{t('deviceIntro')}</p>
      </aside>
      <main className="split-main">
        <div className="panel wide">
          <div className="row" style={{ flexWrap: 'wrap' }}>
            <h1>{t('deviceTitle')}</h1>
            <LanguageToggle value={lang} onChange={setLang} />
          </div>
          <div className="grid-2">
            <button type="button" className="option-card" aria-pressed={mode === 'main'} onClick={() => setMode('main')}>
              <strong>{t('mainPc')}</strong>
              <span className="small muted">{t('mainPcHelp')}</span>
            </button>
            <button type="button" className="option-card" aria-pressed={mode === 'till'} onClick={() => setMode('till')}>
              <strong>{t('extraTill')}</strong>
              <span className="small muted">{t('extraTillHelp')}</span>
            </button>
          </div>
          {mode === 'till' && (
            <div className="field">
              <label className="label" htmlFor="host">{t('mainAddress')}</label>
              <input id="host" className="input ltr" value={mainHost} onChange={(e) => setMainHost(e.target.value)} placeholder="192.168.1.10" />
              <span className="help">{t('mainAddressHelp')}</span>
            </div>
          )}
          <div className="field">
            <label className="label" htmlFor="till">{t('tillCode')}</label>
            <input id="till" className="input ltr" value={tillCode} onChange={(e) => setTillCode(e.target.value)} maxLength={12} />
            <span className="help">{t('tillCodeHelp')}</span>
          </div>
          {error && <div className="alert alert-bad" role="alert">{error}</div>}
          <button type="button" className="btn btn-primary btn-lg" disabled={busy} onClick={save}>
            {t('saveRestart')}
          </button>
        </div>
      </main>
    </div>
  );
}
