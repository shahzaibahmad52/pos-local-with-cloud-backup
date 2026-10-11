/** True inside the desktop app (Tauri), false in a plain browser. */
export function isDesktop(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/** Opens a wa.me link in the default browser / WhatsApp app. */
export async function openWhatsApp(url: string): Promise<void> {
  if (isDesktop()) {
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('open_whatsapp', { url });
  } else {
    window.open(url, '_blank', 'noopener');
  }
}

/**
 * Saves a file the app made (Excel sheet, transfer file). The desktop app
 * can't download like a browser, so it saves to Downloads and shows the
 * file there; a browser downloads it as usual. Returns where it was saved
 * on the desktop, or null in a browser.
 */
export async function saveFile(name: string, blob: Blob): Promise<string | null> {
  if (isDesktop()) {
    const dataUrl = await new Promise<string>((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result));
      reader.onerror = () => reject(reader.error ?? new Error('Could not read the file.'));
      reader.readAsDataURL(blob);
    });
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke<string>('save_download', { name, dataBase64: dataUrl.slice(dataUrl.indexOf(',') + 1) });
  }
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  return null;
}
