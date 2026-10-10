// Small pieces shared by backoffice screens.

import { useEffect, useState } from 'react';
import { api } from '../api/client';
import type { MoneySource, OpenTill } from '../api/types';
import { useI18n } from '../i18n/i18n';
import type { StringKey } from '../i18n/strings';

export const SOURCES: [MoneySource, StringKey][] = [
  ['drawer', 'srcDrawer'],
  ['owner', 'srcOwner'],
  ['bank', 'srcBank'],
  ['wallet', 'srcWallet'],
];

/** Tills with an open shift, refreshed when the component mounts. */
export function useOpenTills(): OpenTill[] {
  const [tills, setTills] = useState<OpenTill[]>([]);
  useEffect(() => {
    api
      .openTills()
      .then(setTills)
      .catch(() => setTills([]));
  }, []);
  return tills;
}

/**
 * Where money comes from (or goes to). Choosing the drawer also asks which
 * till, because the cash must be counted in that till's shift.
 */
export function MoneySourcePicker({
  id,
  label,
  value,
  onChange,
  till,
  onTill,
  tills,
}: {
  id: string;
  label: string;
  value: MoneySource;
  onChange: (v: MoneySource) => void;
  till: string;
  onTill: (v: string) => void;
  tills: OpenTill[];
}) {
  const { t } = useI18n();
  return (
    <div className="field">
      <label className="label" htmlFor={id}>
        {label}
      </label>
      <select
        id={id}
        className="select"
        value={value}
        onChange={(e) => {
          const v = e.target.value as MoneySource;
          onChange(v);
          if (v === 'drawer' && !till && tills[0]) onTill(tills[0].tillCode);
        }}
      >
        {SOURCES.map(([v, k]) => (
          <option key={v} value={v}>
            {t(k)}
          </option>
        ))}
      </select>
      {value === 'drawer' &&
        (tills.length ? (
          <select aria-label={t('whichTill')} className="select sm" value={till} onChange={(e) => onTill(e.target.value)}>
            {tills.map((x) => (
              <option key={x.tillCode} value={x.tillCode}>
                {t('drawerOf', { till: x.tillCode, name: x.cashierName })}
              </option>
            ))}
          </select>
        ) : (
          <span className="help" style={{ color: 'var(--warn)' }}>
            {t('noOpenTill')}
          </span>
        ))}
    </div>
  );
}

/** A small modal with a title, used for quick backoffice forms. */
export function Modal({ title, onClose, children, wide = false }: { title: string; onClose: () => void; children: React.ReactNode; wide?: boolean }) {
  return (
    <div className="overlay" role="dialog" aria-modal="true" aria-label={title} onKeyDown={(e) => e.key === 'Escape' && onClose()}>
      <div className="dialog" style={wide ? { maxWidth: 620 } : undefined}>
        <h2>{title}</h2>
        {children}
      </div>
    </div>
  );
}

/** Reads and writes a value in this browser's storage, ignoring failures. */
export const local = {
  get<T>(key: string): T | null {
    try {
      const v = localStorage.getItem(key);
      return v ? (JSON.parse(v) as T) : null;
    } catch {
      return null;
    }
  },
  set(key: string, value: unknown): void {
    try {
      if (value === null) localStorage.removeItem(key);
      else localStorage.setItem(key, JSON.stringify(value));
    } catch {
      // storage full or blocked: drafts just aren't kept
    }
  },
};

export function newClientRef(): string {
  return typeof crypto !== 'undefined' && 'randomUUID' in crypto
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}
