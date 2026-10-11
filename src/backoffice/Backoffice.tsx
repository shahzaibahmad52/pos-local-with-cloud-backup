import { useEffect, useState, type ReactNode } from 'react';
import { api } from '../api/client';
import type { CloudPage, InventoryTab, Language, StoreHeader, User } from '../api/types';
import { BrandMark, LanguageToggle } from '../components/common';
import { useI18n } from '../i18n/i18n';
import type { StringKey } from '../i18n/strings';
import type { PrinterSettings } from '../print/printer';
import { Activity } from '../screens/Activity';
import { Customers } from './Customers';
import { Dashboard } from './Dashboard';
import { Expenses } from './Expenses';
import { HeadOffice } from './HeadOffice';
import { ImportProducts } from './ImportProducts';
import { Inventory } from './Inventory';
import { ProductForm } from './ProductForm';
import { Purchases } from './Purchases';
import { Settings } from './Settings';

export type Page =
  | { name: 'dashboard' }
  | { name: 'inventory'; tab?: InventoryTab; selected?: string }
  | { name: 'product'; productId: string | null }
  | { name: 'import' }
  | { name: 'purchases'; tab?: 'new' | 'history' | 'suppliers' }
  | { name: 'expenses' }
  | { name: 'khata' }
  | { name: 'activity' }
  | { name: 'stores' }
  | { name: 'settings' };

const ICONS: Record<string, ReactNode> = {
  dashboard: (
    <>
      <rect x="3" y="3" width="7" height="7" rx="1" />
      <rect x="14" y="3" width="7" height="7" rx="1" />
      <rect x="3" y="14" width="7" height="7" rx="1" />
      <rect x="14" y="14" width="7" height="7" rx="1" />
    </>
  ),
  inventory: (
    <>
      <path d="M21 8 12 3 3 8v8l9 5 9-5z" />
      <path d="m3 8 9 5 9-5M12 13v8" />
    </>
  ),
  purchases: (
    <>
      <path d="M3 6h11v10H3zM14 9h4l3 3v4h-7" />
      <circle cx="7" cy="18" r="2" />
      <circle cx="17" cy="18" r="2" />
    </>
  ),
  expenses: (
    <>
      <rect x="3" y="6" width="18" height="13" rx="2" />
      <path d="M3 10h18M16 15h2" />
    </>
  ),
  khata: (
    <>
      <circle cx="9" cy="8" r="4" />
      <path d="M2 21a7 7 0 0 1 14 0M17 4a4 4 0 0 1 0 8M22 21a6 6 0 0 0-4-5.7" />
    </>
  ),
  activity: (
    <>
      <path d="M12 3 4 6v6c0 5 3.5 8 8 9 4.5-1 8-4 8-9V6z" />
      <path d="m9 12 2 2 4-4" />
    </>
  ),
  stores: (
    <>
      <path d="M3 9 5 4h14l2 5M3 9h18v11H3zM9 20v-6h6v6" />
    </>
  ),
  settings: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M12 2v3M12 19v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M2 12h3M19 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1" />
    </>
  ),
};

const NAV: [Page['name'], StringKey][] = [
  ['dashboard', 'navDashboard'],
  ['inventory', 'navInventory'],
  ['purchases', 'navPurchases'],
  ['expenses', 'navExpenses'],
  ['khata', 'navKhata'],
  ['activity', 'navActivity'],
  ['stores', 'navStores'],
  ['settings', 'navSettings'],
];

/** Which nav item a page belongs under. */
function section(page: Page): Page['name'] {
  if (page.name === 'product' || page.name === 'import') return 'inventory';
  return page.name;
}

export function Backoffice({
  user,
  store,
  printer,
  tillCode,
  onStoreChange,
  onOpenTill,
  onSignOut,
}: {
  user: User;
  store: StoreHeader;
  printer: PrinterSettings;
  tillCode: string;
  onStoreChange: (store: StoreHeader) => void;
  onOpenTill: () => void;
  onSignOut: () => void;
}) {
  const { t, lang, setLang } = useI18n();
  const [page, setPage] = useState<Page>({ name: 'dashboard' });
  const [cloud, setCloud] = useState<CloudPage | null>(null);

  // Cloud backup status for the menu, refreshed every minute.
  useEffect(() => {
    const load = () =>
      api
        .cloud()
        .then(setCloud)
        .catch(() => undefined);
    void load();
    const timer = setInterval(load, 60_000);
    return () => clearInterval(timer);
  }, [page.name]);

  const changeLang = (l: Language) => {
    setLang(l);
    void api.setLanguage(l).catch(() => undefined);
  };

  if (page.name === 'activity') {
    return <Activity store={store} onBack={() => setPage({ name: 'dashboard' })} backLabel={t('boTitle')} />;
  }

  const go = (p: Page) => {
    setPage(p);
    window.scrollTo(0, 0);
  };

  let content: ReactNode;
  switch (page.name) {
    case 'dashboard':
      content = <Dashboard store={store} go={go} />;
      break;
    case 'inventory':
      content = <Inventory store={store} go={go} initialTab={page.tab} initialSelected={page.selected} />;
      break;
    case 'product':
      content = <ProductForm key={page.productId ?? 'new'} productId={page.productId} go={go} />;
      break;
    case 'import':
      content = <ImportProducts go={go} />;
      break;
    case 'purchases':
      content = <Purchases tillCode={tillCode} initialTab={page.tab} />;
      break;
    case 'expenses':
      content = <Expenses tillCode={tillCode} />;
      break;
    case 'khata':
      content = <Customers store={store} printer={printer} tillCode={tillCode} />;
      break;
    case 'stores':
      content = <HeadOffice />;
      break;
    case 'settings':
      content = <Settings user={user} printer={printer} onStoreChange={onStoreChange} onCloudChange={setCloud} />;
      break;
  }

  return (
    <div className="bo app">
      <nav className="bo-nav" aria-label={t('boTitle')}>
        <div className="who">
          <BrandMark size={18} />
          <div style={{ display: 'flex', flexDirection: 'column', minWidth: 0 }}>
            <strong>{t('boTitle')}</strong>
            <span className="small" style={{ color: 'var(--night-muted)' }}>
              {user.name} · {store.name}
            </span>
          </div>
        </div>
        <div className="bo-links">
          {NAV.map(([name, label]) => (
            <button
              key={name}
              type="button"
              className="bo-link"
              aria-current={section(page) === name ? 'page' : undefined}
              onClick={() => go({ name } as Page)}
            >
              <svg aria-hidden="true" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                {ICONS[name]}
              </svg>
              {t(label)}
            </button>
          ))}
        </div>
        <div className="foot">
          <SyncStatus cloud={cloud} onClick={() => go({ name: 'settings' })} />
          <button type="button" className="btn btn-primary" onClick={onOpenTill}>
            {t('openTill')}
          </button>
          <LanguageToggle value={lang} onChange={changeLang} onNight />
          <button type="button" className="head-btn" onClick={onSignOut}>
            {t('signOut')}
          </button>
        </div>
      </nav>
      <main className="bo-main">{content}</main>
    </div>
  );
}

function SyncStatus({ cloud, onClick }: { cloud: CloudPage | null; onClick: () => void }) {
  const { t } = useI18n();
  if (!cloud) return null;
  let text: string;
  let colour = '#4fbf98';
  if (!cloud.enabled) {
    text = t('syncOff');
    colour = '#a9bdb4';
  } else if (cloud.lastError && cloud.pending > 0) {
    text = t('syncWaiting', { n: cloud.pending });
    colour = '#f0a640';
  } else if (!cloud.lastSyncAt) {
    text = t('syncNever');
    colour = '#f0a640';
  } else {
    const d = new Date(cloud.lastSyncAt);
    const p = (n: number) => String(n).padStart(2, '0');
    text = t('syncOk', { time: `\u2066${p(d.getHours())}:${p(d.getMinutes())}\u2069` });
  }
  return (
    <button
      type="button"
      onClick={onClick}
      style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 13, color: '#cfe6dc', background: 'none', border: 0, padding: 0, textAlign: 'start' }}
    >
      <span aria-hidden="true" style={{ width: 8, height: 8, borderRadius: '50%', background: colour, flex: 'none' }} />
      {text}
    </button>
  );
}
