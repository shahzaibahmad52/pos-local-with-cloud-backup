import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import type { Language } from '../api/types';
import { strings, type StringKey } from './strings';

interface I18n {
  lang: Language;
  dir: 'ltr' | 'rtl';
  setLang: (lang: Language) => void;
  t: (key: StringKey, vars?: Record<string, string | number>) => string;
}

const I18nContext = createContext<I18n | null>(null);

export function translate(lang: Language, key: StringKey, vars?: Record<string, string | number>): string {
  let text = strings[lang][key] ?? strings.en[key];
  if (vars) {
    for (const [k, v] of Object.entries(vars)) text = text.replaceAll(`{${k}}`, String(v));
  }
  return text;
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLang] = useState<Language>('en');
  const value = useMemo<I18n>(
    () => ({
      lang,
      dir: lang === 'ur' ? 'rtl' : 'ltr',
      setLang,
      t: (key, vars) => translate(lang, key, vars),
    }),
    [lang],
  );
  useEffect(() => {
    document.documentElement.lang = lang;
    document.documentElement.dir = value.dir;
  }, [lang, value.dir]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18n {
  const ctx = useContext(I18nContext);
  if (!ctx) throw new Error('useI18n must be used inside I18nProvider');
  return ctx;
}
