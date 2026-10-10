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
