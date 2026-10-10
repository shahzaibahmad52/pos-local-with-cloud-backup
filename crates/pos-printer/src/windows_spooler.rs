//! Raw printing through the Windows print spooler (no driver rendering, no
//! dialog): the ESC/POS bytes go to the printer exactly as built.

use windows::{
    core::{PCWSTR, PWSTR},
    Win32::Graphics::Printing::{
        ClosePrinter, EndDocPrinter, EndPagePrinter, EnumPrintersW, OpenPrinterW, StartDocPrinterW, StartPagePrinter,
        WritePrinter, DOC_INFO_1W, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL, PRINTER_HANDLE, PRINTER_INFO_4W,
    },
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn list_printers() -> Result<Vec<String>, String> {
    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let mut needed = 0u32;
    let mut count = 0u32;
    // First call only reports the buffer size.
    let _ = unsafe { EnumPrintersW(flags, PCWSTR::null(), 4, None, &mut needed, &mut count) };
    if needed == 0 {
        return Ok(Vec::new());
    }
    let mut buffer = vec![0u8; needed as usize];
    unsafe { EnumPrintersW(flags, PCWSTR::null(), 4, Some(&mut buffer), &mut needed, &mut count) }
        .map_err(|e| format!("Couldn't list printers: {e}"))?;
    let infos = unsafe { std::slice::from_raw_parts(buffer.as_ptr() as *const PRINTER_INFO_4W, count as usize) };
    let mut names = Vec::with_capacity(infos.len());
    for info in infos {
        if !info.pPrinterName.is_null() {
            if let Ok(name) = unsafe { info.pPrinterName.to_string() } {
                names.push(name);
            }
        }
    }
    Ok(names)
}

pub fn send_raw(printer: &str, data: &[u8]) -> Result<(), String> {
    let name = wide(printer);
    let mut handle = PRINTER_HANDLE::default();
    unsafe { OpenPrinterW(PCWSTR(name.as_ptr()), &mut handle, None) }
        .map_err(|e| format!("Couldn't open printer \"{printer}\": {e}"))?;

    let result = (|| {
        let mut doc_name = wide("Receipt");
        let mut datatype = wide("RAW");
        let info = DOC_INFO_1W {
            pDocName: PWSTR(doc_name.as_mut_ptr()),
            pOutputFile: PWSTR::null(),
            pDatatype: PWSTR(datatype.as_mut_ptr()),
        };
        let job = unsafe { StartDocPrinterW(handle, 1, &info) };
        if job == 0 {
            return Err(format!("Printer \"{printer}\" refused the job."));
        }
        let page_ok = unsafe { StartPagePrinter(handle) }.as_bool();
        let mut written = 0u32;
        let write_ok = page_ok
            && unsafe { WritePrinter(handle, data.as_ptr() as *const _, data.len() as u32, &mut written) }.as_bool();
        unsafe {
            let _ = EndPagePrinter(handle);
            let _ = EndDocPrinter(handle);
        }
        if !write_ok || written as usize != data.len() {
            return Err(format!("Printing to \"{printer}\" failed. Check it is switched on and has paper."));
        }
        Ok(())
    })();

    unsafe {
        let _ = ClosePrinter(handle);
    }
    result
}
