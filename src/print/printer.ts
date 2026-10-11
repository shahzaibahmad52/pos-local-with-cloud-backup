// Printing a document on this till: straight to the thermal printer in the
// desktop app (no dialog, can open the drawer), or through the browser's
// print dialog otherwise.

import { isDesktop } from '../platform';
import type { Doc } from './doc';
import { rasterDoc, toBase64 } from './raster';

export interface PrinterSettings {
  /** Windows printer name; null = use the print dialog. */
  name: string | null;
  /** 576 for 80 mm paper, 384 for 58 mm. */
  widthDots: number;
  autoPrint: boolean;
  openDrawerOnCash: boolean;
  cut: boolean;
}

export const DEFAULT_PRINTER: PrinterSettings = {
  name: null,
  widthDots: 576,
  autoPrint: true,
  openDrawerOnCash: true,
  cut: true,
};

const LOCAL_KEY = 'medstore.printer';

export function readLocalPrinter(): PrinterSettings {
  try {
    const raw = window.localStorage.getItem(LOCAL_KEY);
    return raw ? { ...DEFAULT_PRINTER, ...JSON.parse(raw), name: null } : DEFAULT_PRINTER;
  } catch {
    return DEFAULT_PRINTER;
  }
}

export function canPrintDirect(settings: PrinterSettings): boolean {
  return isDesktop() && !!settings.name;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const core = await import('@tauri-apps/api/core');
  return core.invoke<T>(cmd, args);
}

/**
 * Prints `doc`. Without a direct printer the caller must have the doc in a
 * PrintPortal, which is what the dialog prints.
 */
export async function printDoc(doc: Doc, settings: PrinterSettings, options: { openDrawer?: boolean } = {}): Promise<void> {
  if (canPrintDirect(settings)) {
    const image = await rasterDoc(doc, settings.widthDots);
    await invoke('print_raster', {
      printer: settings.name,
      width: image.width,
      height: image.height,
      bitsBase64: toBase64(image.bits),
      cut: settings.cut,
      openDrawer: !!options.openDrawer,
    });
    return;
  }
  await new Promise((r) => setTimeout(r, 60));
  window.print();
}

export async function openDrawer(settings: PrinterSettings): Promise<void> {
  if (!canPrintDirect(settings)) throw new Error('No receipt printer is set up on this till.');
  await invoke('open_cash_drawer', { printer: settings.name });
}

export async function listPrinters(): Promise<string[]> {
  return isDesktop() ? invoke<string[]>('list_printers') : [];
}

export async function savePrinterSettings(settings: PrinterSettings): Promise<void> {
  if (isDesktop()) {
    await invoke('save_printer_settings', { printer: settings });
    return;
  }
  try {
    window.localStorage.setItem(LOCAL_KEY, JSON.stringify(settings));
  } catch {
    // Private window: settings last until the page is closed.
  }
}
