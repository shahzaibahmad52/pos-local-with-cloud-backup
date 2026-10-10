//! Receipt printing for 80 mm / 58 mm thermal printers.
//!
//! The till draws each receipt as a black-and-white image, so Urdu (and any
//! other script) prints on every ESC/POS printer, even ones with no Urdu font.
//! This crate turns that image into printer commands and, on Windows, sends
//! them straight to the printer with no print dialog.

pub mod escpos;

#[cfg(windows)]
mod windows_spooler;

/// Printer names installed on this computer.
pub fn list_printers() -> Result<Vec<String>, String> {
    #[cfg(windows)]
    {
        windows_spooler::list_printers()
    }
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}

/// Sends raw ESC/POS bytes to a printer.
pub fn send_raw(printer: &str, data: &[u8]) -> Result<(), String> {
    #[cfg(windows)]
    {
        windows_spooler::send_raw(printer, data)
    }
    #[cfg(not(windows))]
    {
        let _ = (printer, data);
        Err("Direct printing is only available on Windows.".into())
    }
}
