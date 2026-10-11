//! ESC/POS commands. Only the widely supported subset is used so cheap
//! 80 mm printers work: initialise, raster image (`GS v 0`), feed, cut and
//! the cash-drawer pulse.

const ESC: u8 = 0x1B;
const GS: u8 = 0x1D;

/// The tallest image accepted, in rows.
pub const MAX_HEIGHT: usize = 200_000;

/// Rows per `GS v 0` command. Some printers have small buffers, so a tall
/// receipt is sent as several bands.
const BAND_ROWS: usize = 255;

#[derive(Debug, Clone, Copy, Default)]
pub struct JobOptions {
    pub cut: bool,
    pub open_drawer: bool,
}

/// A 1-bit image: each row is `width / 8` bytes, most significant bit first,
/// 1 = black dot.
#[derive(Debug, Clone)]
pub struct Raster<'a> {
    pub width: usize,
    pub height: usize,
    pub bits: &'a [u8],
}

impl Raster<'_> {
    pub fn bytes_per_row(&self) -> usize {
        self.width.div_ceil(8)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.width == 0 || self.width > 2048 {
            return Err(format!("Image width {} is not supported.", self.width));
        }
        // 200,000 rows is 25 m of paper at 8 dots/mm: any real bill or statement.
        if self.height == 0 || self.height > MAX_HEIGHT {
            return Err(format!("Image height {} is not supported.", self.height));
        }
        let expected = self.bytes_per_row() * self.height;
        if self.bits.len() != expected {
            return Err(format!("Image data is {} bytes, expected {expected}.", self.bits.len()));
        }
        Ok(())
    }
}

/// Pulse on drawer pin 2 (the usual RJ11 cable from printer to drawer).
pub fn drawer_pulse() -> Vec<u8> {
    vec![ESC, b'p', 0, 25, 250]
}

/// Everything needed to print one receipt image.
pub fn raster_job(image: &Raster, options: JobOptions) -> Result<Vec<u8>, String> {
    image.validate()?;
    let row_bytes = image.bytes_per_row();
    let mut out = Vec::with_capacity(image.bits.len() + 64);
    out.extend_from_slice(&[ESC, b'@']); // initialise
    out.extend_from_slice(&[ESC, b'a', 0]); // left aligned
    if options.open_drawer {
        // Open the drawer first so the cashier can take cash while it prints.
        out.extend(drawer_pulse());
    }
    for band in image.bits.chunks(row_bytes * BAND_ROWS) {
        let rows = band.len() / row_bytes;
        out.extend_from_slice(&[
            GS,
            b'v',
            b'0',
            0, // normal density
            (row_bytes & 0xFF) as u8,
            (row_bytes >> 8) as u8,
            (rows & 0xFF) as u8,
            (rows >> 8) as u8,
        ]);
        out.extend_from_slice(band);
    }
    out.extend_from_slice(&[ESC, b'd', 4]); // feed so the end clears the cutter
    if options.cut {
        out.extend_from_slice(&[GS, b'V', 66, 0]); // feed to cut position and partial cut
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_image_job() {
        // 16 x 2 image: two bytes per row.
        let bits = [0xFF, 0x00, 0x0F, 0xF0];
        let job =
            raster_job(&Raster { width: 16, height: 2, bits: &bits }, JobOptions { cut: true, open_drawer: false })
                .unwrap();
        assert_eq!(&job[..5], &[ESC, b'@', ESC, b'a', 0]);
        assert_eq!(&job[5..13], &[GS, b'v', b'0', 0, 2, 0, 2, 0]);
        assert_eq!(&job[13..17], &bits);
        assert_eq!(&job[17..], &[ESC, b'd', 4, GS, b'V', 66, 0]);
    }

    #[test]
    fn tall_images_are_sent_in_bands() {
        let width = 576;
        let height = 600;
        let bits = vec![0u8; width / 8 * height];
        let job = raster_job(&Raster { width, height, bits: &bits }, JobOptions::default()).unwrap();
        let headers = job.windows(3).filter(|w| w == &[GS, b'v', b'0']).count();
        assert_eq!(headers, 3); // 255 + 255 + 90 rows
        assert_eq!(job.len(), 5 + 3 * 8 + bits.len() + 3);
    }

    #[test]
    fn drawer_pulse_comes_first() {
        let bits = [0u8; 1];
        let job =
            raster_job(&Raster { width: 8, height: 1, bits: &bits }, JobOptions { cut: false, open_drawer: true })
                .unwrap();
        assert_eq!(&job[5..10], &drawer_pulse()[..]);
    }

    #[test]
    fn rejects_wrong_sizes() {
        let bits = [0u8; 3];
        assert!(raster_job(&Raster { width: 16, height: 2, bits: &bits }, JobOptions::default()).is_err());
        assert!(raster_job(&Raster { width: 0, height: 1, bits: &[] }, JobOptions::default()).is_err());
        // A long bill (about 700 lines) goes through, in bands.
        let tall = vec![0u8; 72 * 40_000];
        assert!(raster_job(&Raster { width: 576, height: 40_000, bits: &tall }, JobOptions::default()).is_ok());
    }
}
