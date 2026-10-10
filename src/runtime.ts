// Where this computer sends API requests, and which till it is.
// In the desktop app the answer comes from device.json (see src-tauri);
// in a plain browser (development) the Vite server proxies /api.

import { DEFAULT_PRINTER, readLocalPrinter, type PrinterSettings } from './print/printer';

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
  printer: PrinterSettings;
}

import { isDesktop } from './platform';

export { isDesktop };

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
    const info = await invoke<{ config: Partial<DeviceConfig> & { printer?: Partial<PrinterSettings> }; serverUrl: string | null }>(
      'get_runtime_info',
    );
    const config: DeviceConfig = {
      mode: info.config.mode ?? null,
      mainHost: info.config.mainHost ?? null,
      port: info.config.port ?? null,
      tillCode: info.config.tillCode ?? null,
    };
    const printer = { ...DEFAULT_PRINTER, ...info.config.printer };
    return { isDesktop: true, serverUrl: info.serverUrl, tillCode: config.tillCode ?? 'T01', config, printer };
  }
  const tillCode = readLocal('medstore.tillCode') ?? 'T01';
  return {
    isDesktop: false,
    serverUrl: '',
    tillCode,
    config: { mode: 'main', mainHost: null, port: null, tillCode },
    printer: readLocalPrinter(),
  };
}

export async function saveDeviceConfig(config: DeviceConfig): Promise<void> {
  const { invoke } = await import('@tauri-apps/api/core');
  await invoke('save_device_config', { config });
  await invoke('restart_app');
}
