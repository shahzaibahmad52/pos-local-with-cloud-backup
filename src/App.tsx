import { useCallback, useEffect, useState } from 'react';
import { api, ApiError, configureApi } from './api/client';
import type { Shift, StoreHeader, User } from './api/types';
import { BrandMark } from './components/common';
import { useI18n } from './i18n/i18n';
import { loadRuntime, type Runtime } from './runtime';
import { DeviceSetup } from './screens/DeviceSetup';
import { Login } from './screens/Login';
import { OpenShift } from './screens/OpenShift';
import { StoreSetup } from './screens/StoreSetup';
import { Till } from './screens/Till';

type Stage =
  | { name: 'boot' }
  | { name: 'device-setup' }
  | { name: 'unreachable'; message: string }
  | { name: 'store-setup' }
  | { name: 'login'; store: StoreHeader }
  | { name: 'open-shift'; store: StoreHeader; user: User }
  | { name: 'till'; store: StoreHeader; user: User; shift: Shift };

export function App() {
  const { t, setLang } = useI18n();
  const [runtime, setRuntime] = useState<Runtime | null>(null);
  const [stage, setStage] = useState<Stage>({ name: 'boot' });

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
    const tillCode = runtime?.tillCode ?? 'T01';
    try {
      const shift = await api.currentShift(tillCode);
      if (shift) setStage({ name: 'till', store, user, shift });
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
          onDone={async (user) => {
            const health = await api.health();
            if (health.store) await afterSignIn(health.store, user);
          }}
        />
      );
    case 'login':
      return <Login store={stage.store} tillCode={tillCode} onSignedIn={(user) => void afterSignIn(stage.store, user)} />;
    case 'open-shift':
      return (
        <OpenShift
          user={stage.user}
          tillCode={tillCode}
          onOpened={(shift) => setStage({ name: 'till', store: stage.store, user: stage.user, shift })}
          onBack={() => setStage({ name: 'login', store: stage.store })}
        />
      );
    case 'till':
      return (
        <Till
          user={stage.user}
          shift={stage.shift}
          store={stage.store}
          tillCode={tillCode}
          onSignOut={() => setStage({ name: 'login', store: stage.store })}
        />
      );
  }
}
