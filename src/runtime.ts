// Where this computer sends API requests, and which till it is.
// In the desktop app the answer comes from device.json (see src-tauri);
// in a plain browser (development) the Vite server proxies /api.

export interface DeviceConfig {
  mode: 'main' | 'till' | null;
  mainHost: string | null;
  port: number | null;
  tillCode: string | null;
}

export interface Runtime {
  isDesktop: boolean;
  /** null until this computer has been set up. */
  serverUrl: string | null;
  tillCode: string;
  config: DeviceConfig;
}

export function isDesktop(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

function readLocal(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

export async function loadRuntime(): Promise<Runtime> {
  if (isDesktop()) {
    const { invoke } = await import('@tauri-apps/api/core');
    const info = await invoke<{ config: Partial<DeviceConfig>; serverUrl: string | null }>('get_runtime_info');
    const config: DeviceConfig = {
      mode: info.config.mode ?? null,
      mainHost: info.config.mainHost ?? null,
      port: info.config.port ?? null,
      tillCode: info.config.tillCode ?? null,
    };
    return { isDesktop: true, serverUrl: info.serverUrl, tillCode: config.tillCode ?? 'T01', config };
  }
  const tillCode = readLocal('medstore.tillCode') ?? 'T01';
  return {
    isDesktop: false,
    serverUrl: '',
    tillCode,
    config: { mode: 'main', mainHost: null, port: null, tillCode },
  };
}

export async function saveDeviceConfig(config: DeviceConfig): Promise<void> {
  const { invoke } = await import('@tauri-apps/api/core');
  await invoke('save_device_config', { config });
  await invoke('restart_app');
}
