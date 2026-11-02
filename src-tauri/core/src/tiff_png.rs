//! TIFF для окна (спека 2026-10-20-tiff-i-pdf, спринт 2 от 03.11.2026).
//!
//! Окно программы (WebView2, WebKit) TIFF не показывает. Файл Романа
//! («0118 Федор Иванович Дорохов.tif») — 5264×4763, одна страница, 1 бит на
//! точку, LZW. Перекодируем в PNG той же глубины: чёрно-белый остаётся
//! в 1 бит — файл мал и пишется быстро. Многостраничных TIFF у Романа нет
//! (ответ 6.3) — берётся первая страница.

use std::path::Path;
use tiff::decoder::{Decoder, DecodingResult, Limits};
use tiff::ColorType;

/// PNG первой страницы TIFF.
pub fn to_png(path: &Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("файл не открывается: {e}"))?;
    let mut dec = Decoder::new(std::io::BufReader::new(file))
        .map_err(|e| format!("это не TIFF: {e}"))?
        // Скан листа бывает большим: 10 000 × 7 000 в цвете — 210 МБ.
        .with_limits({
            let mut l = Limits::default();
            l.decoding_buffer_size = 512 << 20;
            l.intermediate_buffer_size = 512 << 20;
            l
        });
    let (w, h) = dec.dimensions().map_err(|e| format!("TIFF не читается: {e}"))?;
    let color = dec.colortype().map_err(|e| format!("TIFF не читается: {e}"))?;
    let img = dec.read_image().map_err(|e| format!("TIFF не читается: {e}"))?;
    // Белый — ноль (WhiteIsZero) декодер уже перевернул: дальше ноль — чёрный.
    let (kind, depth, bytes) = match (color, img) {
        (ColorType::Gray(b @ (1 | 2 | 4 | 8)), DecodingResult::U8(v)) => (png::ColorType::Grayscale, b, v),
        (ColorType::Gray(16), DecodingResult::U16(v)) => (png::ColorType::Grayscale, 8, high_bytes(&v)),
        (ColorType::GrayA(8), DecodingResult::U8(v)) => (png::ColorType::GrayscaleAlpha, 8, v),
        (ColorType::RGB(8), DecodingResult::U8(v)) => (png::ColorType::Rgb, 8, v),
        (ColorType::RGB(16), DecodingResult::U16(v)) => (png::ColorType::Rgb, 8, high_bytes(&v)),
        (ColorType::RGBA(8), DecodingResult::U8(v)) => (png::ColorType::Rgba, 8, v),
        (ColorType::RGBA(16), DecodingResult::U16(v)) => (png::ColorType::Rgba, 8, high_bytes(&v)),
        (ColorType::CMYK(8), DecodingResult::U8(v)) => (png::ColorType::Rgb, 8, cmyk_to_rgb(&v)),
        (c, _) => return Err(format!("такой TIFF программа показать не может ({c:?})")),
    };
    let depth = match depth {
        1 => png::BitDepth::One,
        2 => png::BitDepth::Two,
        4 => png::BitDepth::Four,
        _ => png::BitDepth::Eight,
    };
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(kind);
        enc.set_depth(depth);
        enc.set_compression(png::Compression::Fast);
        let mut writer = enc.write_header().map_err(|e| format!("PNG не записался: {e}"))?;
        writer.write_image_data(&bytes).map_err(|e| format!("PNG не записался: {e}"))?;
    }
    Ok(out)
}

fn high_bytes(v: &[u16]) -> Vec<u8> {
    v.iter().map(|x| (x >> 8) as u8).collect()
}

fn cmyk_to_rgb(v: &[u8]) -> Vec<u8> {
    v.chunks_exact(4)
        .flat_map(|p| {
            let k = 255 - p[3] as u16;
            [p[0], p[1], p[2]].map(|c| ((255 - c as u16) * k / 255) as u8)
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// TIFF в 1 бит без сжатия, 10×2: первая строка — чёрная слева,
    /// вторая — белая; белый — ноль (WhiteIsZero), как у сканеров.
    pub fn bilevel_tiff() -> Vec<u8> {
        let (w, h) = (10u32, 2u32);
        let rows: [u8; 4] = [0xF0, 0x00, 0x00, 0x00]; // 2 байта на строку
        let mut t = b"II*\0".to_vec();
        t.extend_from_slice(&8u32.to_le_bytes());
        let tags: Vec<(u16, u16, u32, u32)> = vec![
            (256, 4, 1, w),
            (257, 4, 1, h),
            (258, 3, 1, 1),
            (259, 3, 1, 1),
            (262, 3, 1, 0),
            (273, 4, 1, 0), // место данных — ниже
            (277, 3, 1, 1),
            (278, 4, 1, h),
            (279, 4, 1, rows.len() as u32),
        ];
        let data_at = 8 + 2 + tags.len() as u32 * 12 + 4;
        t.extend_from_slice(&(tags.len() as u16).to_le_bytes());
        for (tag, typ, n, v) in tags {
            t.extend_from_slice(&tag.to_le_bytes());
            t.extend_from_slice(&typ.to_le_bytes());
            t.extend_from_slice(&n.to_le_bytes());
            let v = if tag == 273 { data_at } else { v };
            if typ == 3 {
                t.extend_from_slice(&(v as u16).to_le_bytes());
                t.extend_from_slice(&[0, 0]);
            } else {
                t.extend_from_slice(&v.to_le_bytes());
            }
        }
        t.extend_from_slice(&0u32.to_le_bytes());
        t.extend_from_slice(&rows);
        t
    }

    #[test]
    fn bilevel_tiff_to_one_bit_png() {
        let path = std::env::temp_dir().join(format!("genmetric-tiff-{}.tif", std::process::id()));
        std::fs::write(&path, bilevel_tiff()).unwrap();
        let png_bytes = to_png(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let dec = png::Decoder::new(std::io::Cursor::new(png_bytes));
        let mut r = dec.read_info().unwrap();
        let info = r.info();
        assert_eq!((info.width, info.height, info.bit_depth, info.color_type), (10, 2, png::BitDepth::One, png::ColorType::Grayscale));
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        r.next_frame(&mut buf).unwrap();
        // WhiteIsZero перевёрнут: единицы TIFF (чёрное) — нули PNG.
        assert_eq!(&buf[..2], &[0x0F, 0xFF], "первая строка: 4 чёрных точки, дальше белое");
        assert_eq!(&buf[2..4], &[0xFF, 0xFF], "вторая строка — белая");
    }

    #[test]
    fn not_a_tiff_is_an_error() {
        let path = std::env::temp_dir().join(format!("genmetric-nottiff-{}.tif", std::process::id()));
        std::fs::write(&path, b"JFIF").unwrap();
        assert!(to_png(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }

    /// TIFF Романа: 5264×4763, 1 бит, LZW. Только у разработчика:
    /// `GENMETRIC_SCANS_DIR=… cargo test -p genmetric-core scans_roman -- --ignored`.
    #[test]
    #[ignore]
    fn scans_roman_tiff() {
        let Ok(dir) = std::env::var("GENMETRIC_SCANS_DIR") else { return };
        let path = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path())
            .find(|p| p.extension().map_or(false, |x| x.eq_ignore_ascii_case("tif"))).expect("в папке нет TIFF");
        let t = std::time::Instant::now();
        let png_bytes = to_png(&path).unwrap();
        eprintln!("PNG {} КБ за {:?}", png_bytes.len() / 1024, t.elapsed());
        let r = png::Decoder::new(std::io::Cursor::new(png_bytes)).read_info().unwrap();
        assert_eq!((r.info().width, r.info().height, r.info().bit_depth), (5264, 4763, png::BitDepth::One));
    }
}
