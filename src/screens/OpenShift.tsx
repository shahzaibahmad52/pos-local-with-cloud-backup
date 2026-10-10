import { useState, type FormEvent } from 'react';
import { api, ApiError } from '../api/client';
import type { Shift, User } from '../api/types';
import { BrandMark } from '../components/common';
import { parseRupees } from '../domain/money';
import { useI18n } from '../i18n/i18n';

export function OpenShift({ user, tillCode, onOpened, onBack }: { user: User; tillCode: string; onOpened: (s: Shift) => void; onBack: () => void }) {
  const { t } = useI18n();
  const [cash, setCash] = useState('5,000');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const amount = parseRupees(cash);
    if (amount === null) return setError(t('openingCash'));
    setBusy(true);
    try {
      onOpened(await api.openShift(tillCode, amount));
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
          <span className="brand-name">{user.name}</span>
        </div>
      </aside>
      <main className="split-main">
        <form className="panel" onSubmit={submit}>
          <h1>{t('openShiftTitle')}</h1>
          <div className="field">
            <label className="label" htmlFor="cash">{t('openingCash')}</label>
            <input id="cash" className="input big ltr" inputMode="decimal" value={cash} onChange={(e) => setCash(e.target.value)} autoFocus />
          </div>
          {error && <div className="alert alert-bad" role="alert">{error}</div>}
          <button type="submit" className="btn btn-primary btn-lg" disabled={busy}>{t('startShift')}</button>
          <button type="button" className="btn" onClick={onBack}>{t('back')}</button>
        </form>
      </main>
    </div>
  );
}
