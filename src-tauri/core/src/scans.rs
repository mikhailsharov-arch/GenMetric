//! Папка сканов дела (спека 2026-10-10-skany-ryadom-s-formoj).
//!
//! Роман 05.10.2026: «Формат файлов: JPG. Сканы одного дела лежат в одной
//! папке. Одно фото (один файл) — это всегда один разворот книги. … Четкого
//! единого формата или стандарта нейминга нет». Поэтому программа не читает
//! номер страницы из имени, а только ставит файлы по порядку имён — так, как
//! их ставит проводник: «лист 2» раньше «лист 10».

use std::cmp::Ordering;
use std::path::Path;

/// Картинка скана: `.jpg`, `.jpeg`, `.png`, `.tif` или `.tiff` в любом
/// регистре, не служебная (у macOS рядом с картинкой на чужом диске лежит
/// «._имя.jpg»). PNG — с 12.10.2026 (Роман: «архивы очень часто выдают
/// цифровые копии именно в этих форматах»), TIFF — с 03.11.2026: его окно
/// само не показывает, Rust отдаёт его PNG (`tiff_png`, протокол `scan`).
pub fn is_image(name: &str) -> bool {
    let lower = name.to_lowercase();
    !name.starts_with('.') && [".jpg", ".jpeg", ".png", ".tif", ".tiff"].iter().any(|x| lower.ends_with(x))
}

/// PDF дела (с 03.11.2026): каждая страница — отдельный лист.
pub fn is_pdf(name: &str) -> bool {
    !name.starts_with('.') && name.to_lowercase().ends_with(".pdf")
}

/// Файл, который показывает блок скана: картинка или PDF.
pub fn is_scan(name: &str) -> bool {
    is_image(name) || is_pdf(name)
}

/// TIFF: окну — через Rust, а не напрямую.
pub fn is_tiff(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".tif") || lower.ends_with(".tiff")
}

/// Лист PDF: «имя.pdf#12» — файл и номер страницы с единицы. Так он и
/// хранится в `entry.scan_file` (схема базы не меняется).
pub fn sheet(file: &str, page: usize) -> String {
    format!("{file}#{page}")
}

/// Разобрать имя листа: файл и номер страницы PDF (None — не лист PDF).
/// Знак «#» в имени обычного файла номером не считается.
pub fn split_sheet(name: &str) -> (&str, Option<usize>) {
    if let Some((file, page)) = name.rsplit_once('#') {
        if is_pdf(file) && !page.is_empty() && page.bytes().all(|c| c.is_ascii_digit()) {
            if let Ok(n) = page.parse::<usize>() {
                if n >= 1 {
                    return (file, Some(n));
                }
            }
        }
    }
    (name, None)
}

/// Порядок имён «по-человечески»: цифры сравниваются как числа, буквы — без
/// учёта регистра. При равенстве решает обычное сравнение строк: порядок
/// всегда один и тот же.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (x, y): (Vec<char>, Vec<char>) = (a.to_lowercase().chars().collect(), b.to_lowercase().chars().collect());
    let (mut i, mut j) = (0, 0);
    while i < x.len() && j < y.len() {
        if x[i].is_ascii_digit() && y[j].is_ascii_digit() {
            let run = |s: &[char], from: usize| -> usize { s[from..].iter().take_while(|c| c.is_ascii_digit()).count() };
            let (n, m) = (run(&x, i), run(&y, j));
            // Числа любой длины: без ведущих нулей длиннее — больше.
            let trim = |s: &[char]| -> Vec<char> { let t: Vec<char> = s.iter().cloned().skip_while(|c| *c == '0').collect(); t };
            let (p, q) = (trim(&x[i..i + n]), trim(&y[j..j + m]));
            let by_value = p.len().cmp(&q.len()).then_with(|| p.cmp(&q));
            if by_value != Ordering::Equal {
                return by_value;
            }
            i += n;
            j += m;
        } else {
            if x[i] != y[j] {
                return x[i].cmp(&y[j]);
            }
            i += 1;
            j += 1;
        }
    }
    (x.len() - i).cmp(&(y.len() - j)).then_with(|| a.cmp(b))
}

/// Имена файлов сканов папки по порядку: картинки и PDF. Вложенные папки
/// не читаются.
pub fn list(dir: &Path) -> Result<Vec<String>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("папка сканов не читается: {e}"))?;
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| !t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| is_scan(n))
        .collect();
    names.sort_by(|a, b| natural_cmp(a, b));
    Ok(names)
}

/// Путь из адреса протокола `scan`: «%D0%B4» → байты → строка. Не «%XX» —
/// как есть, без паники.
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------------------
//  Кэш сканов: PNG из TIFF (спринт 2 от 03.11.2026)
// ---------------------------------------------------------------------------

/// Папка кэша в папке данных программы.
pub const CACHE_DIR: &str = "кэш сканов";
/// Больше — лишнее уходит, начиная с самого старого.
pub const CACHE_LIMIT: u64 = 500 << 20;

/// Имя файла кэша: отпечаток пути, размера и времени изменения (FNV-1a —
/// одинаков у всех версий программы). Файл поменяли — имя другое.
pub fn cache_name(path: &Path, ext: &str) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("файл не читается: {e}"))?;
    let modified = meta.modified().ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos()).unwrap_or(0);
    let mut h: u64 = 0xcbf29ce484222325;
    let key = format!("{}|{}|{}", path.to_string_lossy(), meta.len(), modified);
    for b in key.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    Ok(format!("{h:016x}.{ext}"))
}

/// Байты из кэша; нет — `make`, и результат кладётся в кэш. Кэш не
/// записался (диск полон, папка только для чтения) — байты всё равно
/// отдаются: показать лист важнее.
pub fn cached(cache: &Path, src: &Path, ext: &str, make: impl FnOnce() -> Result<Vec<u8>, String>) -> Result<Vec<u8>, String> {
    let file = cache.join(cache_name(src, ext)?);
    if let Ok(bytes) = std::fs::read(&file) {
        if !bytes.is_empty() {
            // Прочитанный лист — свежий: чистка кэша уберёт его последним.
            if let Ok(f) = std::fs::File::options().append(true).open(&file) {
                let _ = f.set_modified(std::time::SystemTime::now());
            }
            return Ok(bytes);
        }
    }
    let bytes = make()?;
    if std::fs::create_dir_all(cache).is_ok() {
        // Сначала во временный файл: оборванная запись не станет «готовым» листом.
        // Свой временный файл у каждого запроса: блок и окно скана просят
        // один TIFF одновременно (ревьюер 03.11.2026).
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = file.with_extension(format!("{ext}.tmp{}-{n}", std::process::id()));
        if std::fs::write(&tmp, &bytes).is_ok() && std::fs::rename(&tmp, &file).is_ok() {
            trim_cache(cache, CACHE_LIMIT);
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
    }
    Ok(bytes)
}

/// Кэш больше `limit` — удалить самые старые файлы.
pub fn trim_cache(cache: &Path, limit: u64) {
    let Ok(entries) = std::fs::read_dir(cache) else { return };
    let mut files: Vec<(std::time::SystemTime, u64, std::path::PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file().then(|| (m.modified().unwrap_or(std::time::UNIX_EPOCH), m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort();
    for (_, len, path) in files {
        if total <= limit {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_names_like_a_person() {
        let mut names = vec!["лист 10.jpg", "лист 2.JPG", "Лист 1.jpg", "IMG_0100.jpeg", "IMG_99.jpg", "a.jpg", "001об.jpg", "1.jpg", "01.jpg"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, vec!["01.jpg", "1.jpg", "001об.jpg", "a.jpg", "IMG_99.jpg", "IMG_0100.jpeg", "Лист 1.jpg", "лист 2.JPG", "лист 10.jpg"]);
        assert_eq!(natural_cmp("скан12345678901234567890123.jpg", "скан12345678901234567890124.jpg"), Ordering::Less, "числа длиннее u64");
        assert_eq!(natural_cmp("a.jpg", "a.jpg"), Ordering::Equal);
    }

    #[test]
    fn pdf_sheet_names() {
        assert_eq!(sheet("дело.pdf", 12), "дело.pdf#12");
        assert_eq!(split_sheet("дело.pdf#12"), ("дело.pdf", Some(12)));
        assert_eq!(split_sheet("Дело #3.PDF#2"), ("Дело #3.PDF", Some(2)), "«#» в имени PDF");
        assert_eq!(split_sheet("лист #2.jpg"), ("лист #2.jpg", None), "«#» в имени картинки — не страница");
        assert_eq!(split_sheet("дело.pdf#0"), ("дело.pdf#0", None));
        assert_eq!(split_sheet("дело.pdf#"), ("дело.pdf#", None));
        assert_eq!(split_sheet("дело.pdf"), ("дело.pdf", None));
        assert!(is_tiff("Лист.TIFF") && is_image("лист.tif") && !is_image("дело.pdf") && is_scan("дело.pdf"));
    }

    #[test]
    fn decodes_percent() {
        assert_eq!(percent_decode("%2FUsers%2F%D0%B4%D0%B5%D0%BB%D0%BE.pdf%2312"), "/Users/дело.pdf#12");
        assert_eq!(percent_decode("C%3A%5C%D0%B0%5Ca.tif"), "C:\\а\\a.tif");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%д"), "%д", "не цифры после «%» — как есть, без паники");
    }

    #[test]
    fn cache_reuses_and_trims() {
        let dir = std::env::temp_dir().join(format!("genmetric-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("лист.tif");
        std::fs::write(&src, b"tiff").unwrap();
        let cache = dir.join(CACHE_DIR);
        let mut made = 0;
        for _ in 0..2 {
            let got = cached(&cache, &src, "png", || { made += 1; Ok(b"png".to_vec()) }).unwrap();
            assert_eq!(got, b"png");
        }
        assert_eq!(made, 1, "второй раз — из кэша");
        // Файл изменился — делается заново.
        std::fs::write(&src, "tiff, другой".as_bytes()).unwrap();
        cached(&cache, &src, "png", || { made += 1; Ok(b"png2".to_vec()) }).unwrap();
        assert_eq!(made, 2);
        std::fs::write(cache.join("старый.png"), vec![0u8; 100]).unwrap();
        trim_cache(&cache, 10);
        assert!(std::fs::read_dir(&cache).unwrap().count() <= 1, "лишнее удалено");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lists_only_scans() {
        let dir = std::env::temp_dir().join(format!("genmetric-scans-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("вложенная.jpg")).unwrap();
        for name in ["2.jpg", "10.JPEG", "1.jpg", "._1.jpg", "заметки.txt", "обложка.png", ".DS_Store"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        assert_eq!(list(&dir).unwrap(), vec!["1.jpg", "2.jpg", "10.JPEG", "обложка.png"], "jpg, jpeg и png, без служебных и без папок");
        for name in ["дело.PDF", "лист.tif", "лист 3.TIFF", "._дело.pdf"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        assert_eq!(list(&dir).unwrap(), vec!["1.jpg", "2.jpg", "10.JPEG", "дело.PDF", "лист 3.TIFF", "лист.tif", "обложка.png"],
                   "TIFF и PDF — тоже (03.11.2026)");
        assert!(list(&dir.join("нет такой")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
