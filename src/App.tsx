import { useCallback, useEffect, useState } from 'react';
import { api, ApiError, configureApi, setToken } from './api/client';
import type { Shift, StoreHeader, User } from './api/types';
import { BrandMark } from './components/common';
import { useI18n } from './i18n/i18n';
import type { PrinterSettings } from './print/printer';
import { loadRuntime, type Runtime } from './runtime';
import { Backoffice } from './backoffice/Backoffice';
import { Activity } from './screens/Activity';
import { DeviceSetup } from './screens/DeviceSetup';
import { Login } from './screens/Login';
import { OpenShift } from './screens/OpenShift';
import { Returns } from './screens/Returns';
import { ShiftClose } from './screens/ShiftClose';
import { StoreSetup } from './screens/StoreSetup';
import { Till } from './screens/Till';

type Stage =
  | { name: 'boot' }
  | { name: 'device-setup' }
  | { name: 'unreachable'; message: string }
  | { name: 'store-setup' }
  | { name: 'login'; store: StoreHeader }
  | { name: 'backoffice'; store: StoreHeader; user: User }
  | { name: 'open-shift'; store: StoreHeader; user: User }
  | { name: 'till'; store: StoreHeader; user: User; shift: Shift; over: 'returns' | 'close' | 'activity' | null };

export function App() {
  const { t, setLang } = useI18n();
  const [runtime, setRuntime] = useState<Runtime | null>(null);
  const [stage, setStage] = useState<Stage>({ name: 'boot' });
  const [printer, setPrinter] = useState<PrinterSettings | null>(null);

  const connect = useCallback(async () => {
    setStage({ name: 'boot' });
    let rt: Runtime;
    try {
      rt = await loadRuntime();
    } catch (e) {
      setStage({ name: 'unreachable', message: String(e) });
      return;
    }
    setRuntime(rt);
    setPrinter(rt.printer);
    if (rt.serverUrl === null) {
      setStage({ name: 'device-setup' });
      return;
    }
    configureApi(rt.serverUrl);
    // The main PC's own server may still be starting; retry briefly.
    for (let attempt = 0; attempt < 10; attempt++) {
      try {
        const health = await api.health();
        if (!health.setUp || !health.store) setStage({ name: 'store-setup' });
        else setStage({ name: 'login', store: health.store });
        return;
      } catch (e) {
        if (attempt === 9) {
          setStage({ name: 'unreachable', message: e instanceof ApiError ? e.message : String(e) });
          return;
        }
        await new Promise((r) => setTimeout(r, 500));
      }
    }
  }, []);

  useEffect(() => {
    void connect();
  }, [connect]);

  const afterSignIn = async (store: StoreHeader, user: User) => {
    setLang(user.language);
    // The owner starts in the backoffice; cashiers go straight to the till.
    if (user.role === 'admin') {
      setStage({ name: 'backoffice', store, user });
      return;
    }
    await openTill(store, user);
  };

  const openTill = async (store: StoreHeader, user: User) => {
    const tillCode = runtime?.tillCode ?? 'T01';
    try {
      const shift = await api.currentShift(tillCode);
      if (shift) setStage({ name: 'till', store, user, shift, over: null });
      else setStage({ name: 'open-shift', store, user });
    } catch (e) {
      setStage({ name: 'unreachable', message: e instanceof ApiError ? e.message : String(e) });
    }
  };

  const tillCode = runtime?.tillCode ?? 'T01';

  switch (stage.name) {
    case 'boot':
      return (
        <div className="split">
          <main className="split-main">
            <div className="panel" style={{ alignItems: 'center' }}>
              <BrandMark />
              <span className="muted">{t('loading')}</span>
            </div>
          </main>
        </div>
      );
    case 'device-setup':
      return <DeviceSetup />;
    case 'unreachable':
      return (
        <div className="split">
          <main className="split-main">
            <div className="panel">
              <h1>{t('unreachableTitle')}</h1>
              <p>{t('unreachableHelp')}</p>
              <div className="alert alert-warn small ltr">{stage.message}</div>
              <button type="button" className="btn btn-primary btn-lg" onClick={() => void connect()}>{t('retry')}</button>
              {runtime?.isDesktop && (
                <button type="button" className="btn" onClick={() => setStage({ name: 'device-setup' })}>{t('changeSetup')}</button>
              )}
            </div>
          </main>
        </div>
      );
    case 'store-setup':
      return (
        <StoreSetup
          onRestored={() => void connect()}
          onDone={async (user) => {
            const health = await api.health();
            if (health.store) await afterSignIn(health.store, user);
          }}
        />
      );
    case 'login':
      return <Login store={stage.store} tillCode={tillCode} onSignedIn={(user) => void afterSignIn(stage.store, user)} />;
    case 'backoffice': {
      const signOut = async () => {
        await api.logout().catch(() => undefined);
        setToken(null);
        setStage({ name: 'login', store: stage.store });
      };
      return (
        <Backoffice
          user={stage.user}
          store={stage.store}
          printer={printer ?? runtime!.printer}
          tillCode={tillCode}
          onStoreChange={(store) => setStage({ ...stage, store })}
          onOpenTill={() => void openTill(stage.store, stage.user)}
          onSignOut={() => void signOut()}
        />
      );
    }
    case 'open-shift':
      return (
        <OpenShift
          user={stage.user}
          tillCode={tillCode}
          onOpened={(shift) => setStage({ name: 'till', store: stage.store, user: stage.user, shift, over: null })}
          onBack={() =>
            setStage(stage.user.role === 'admin' ? { name: 'backoffice', store: stage.store, user: stage.user } : { name: 'login', store: stage.store })
          }
        />
      );
    case 'till': {
      const show = (over: 'returns' | 'close' | 'activity' | null) => setStage({ ...stage, over });
      const printerSettings = printer ?? runtime!.printer;
      const signOut = async () => {
        await api.logout().catch(() => undefined);
        setToken(null);
        setStage({ name: 'login', store: stage.store });
      };
      return (
        <>
          <Till
            user={stage.user}
            shift={stage.shift}
            store={stage.store}
            tillCode={tillCode}
            printer={printerSettings}
            onPrinterChange={setPrinter}
            active={stage.over === null}
            onSignOut={() => setStage({ name: 'login', store: stage.store })}
            onOpenReturns={() => show('returns')}
            onCloseShift={() => show('close')}
            onOpenActivity={() => show('activity')}
            onOpenBackoffice={
              stage.user.role === 'admin' ? () => setStage({ name: 'backoffice', store: stage.store, user: stage.user }) : undefined
            }
          />
          {stage.over && (
            <div className="layer">
              {stage.over === 'returns' && (
                <Returns user={stage.user} store={stage.store} tillCode={tillCode} printer={printerSettings} onBack={() => show(null)} />
              )}
              {stage.over === 'close' && (
                <ShiftClose shift={stage.shift} store={stage.store} printer={printerSettings} onBack={() => show(null)} onClosed={() => void signOut()} />
              )}
              {stage.over === 'activity' && <Activity store={stage.store} onBack={() => show(null)} />}
            </div>
          )}
        </>
      );
    }
  }
}
