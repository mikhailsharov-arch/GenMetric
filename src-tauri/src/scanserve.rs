//! Листы PDF и TIFF для окна — протокол `scan` (спека 2026-10-20-tiff-i-pdf,
//! спринт 2 от 03.11.2026).
//!
//! JPG и PNG окно берёт с диска само, протоколом `asset`. TIFF оно не
//! показывает, а страницы PDF — не файлы, поэтому их байты отдаёт Rust: окно
//! просит `scan://…/путь/имя.pdf#12` (на Windows — `http://scan.localhost/…`),
//! Rust достаёт JPEG страницы (`genmetric_core::pdf`) или PNG из TIFF
//! (`genmetric_core::tiff_png`, с кэшем в папке данных). Лист готовится,
//! только когда его открыли: просмотрщик получает адреса всех листов сразу.
//!
//! Протокол отдаёт только файлы папок, которые программа разрешила окну
//! (`allow`), без вложенных, — то же правило, что у `asset`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use genmetric_core::pdf::{self, Page};
use genmetric_core::scans;

/// Индекс страниц PDF и разрешённые папки.
pub struct Sheets {
    dirs: Mutex<HashSet<PathBuf>>,
    /// Страницы PDF по пути: размер и время файла, при которых прочитаны.
    pdfs: Mutex<HashMap<PathBuf, (u64, Option<SystemTime>, Arc<Vec<Page>>)>>,
    /// Папка кэша PNG из TIFF.
    cache: PathBuf,
}

/// Путь без лишних разделителей — для сравнения папок.
fn norm(p: &Path) -> PathBuf {
    p.components().collect()
}

impl Sheets {
    pub fn new(data_dir: &Path) -> Self {
        Sheets { dirs: Mutex::default(), pdfs: Mutex::default(), cache: data_dir.join(scans::CACHE_DIR) }
    }

    /// Окно видит файлы этой папки (без вложенных).
    pub fn allow(&self, dir: &Path) {
        self.dirs.lock().unwrap_or_else(|e| e.into_inner()).insert(norm(dir));
    }

    /// Страницы PDF: из памяти, пока файл не изменился, иначе — разбор файла.
    pub fn pdf_pages(&self, path: &Path) -> Result<Arc<Vec<Page>>, String> {
        let meta = std::fs::metadata(path).map_err(|e| format!("файл не читается: {e}"))?;
        let (len, modified) = (meta.len(), meta.modified().ok());
        if let Some((l, m, pages)) = self.pdfs.lock().unwrap_or_else(|e| e.into_inner()).get(path) {
            if *l == len && *m == modified {
                return Ok(pages.clone());
            }
        }
        let path_owned = path.to_path_buf();
        let pages = Arc::new(genmetric_core::guarded("Разбор PDF", move || pdf::index_file(&path_owned))?);
        let mut all = self.pdfs.lock().unwrap_or_else(|e| e.into_inner());
        // Память — на несколько дел: больше 64 файлов — забывается один, самый
        // старый по времени файла (не всё разом: папка из 17 PDF перечитывалась
        // бы при каждом открытии, ревьюер 03.11.2026).
        if all.len() >= 64 {
            if let Some(old) = all.iter().min_by_key(|(_, v)| v.1).map(|(k, _)| k.clone()) {
                all.remove(&old);
            }
        }
        all.insert(path.to_path_buf(), (len, modified, pages.clone()));
        Ok(pages)
    }

    /// Байты листа по полному пути из адреса: тип и тело.
    fn serve(&self, full: &str) -> Result<(&'static str, Vec<u8>), (u16, String)> {
        let (file, page) = scans::split_sheet(full);
        let path = Path::new(file);
        let dir = path.parent().ok_or((404, "у файла нет папки".to_string()))?;
        if !self.dirs.lock().unwrap_or_else(|e| e.into_inner()).contains(&norm(dir)) {
            return Err((403, "папка не открыта в программе".into()));
        }
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if let Some(n) = page {
            let pages = self.pdf_pages(path).map_err(|e| (404, e))?;
            return match pages.get(n - 1) {
                Some(Page::Jpeg { offset, len }) => pdf::read_jpeg(path, *offset, *len).map(|b| ("image/jpeg", b)).map_err(|e| (404, e)),
                Some(Page::Unsupported(why)) => Err((415, why.clone())),
                None => Err((404, format!("в файле нет страницы {n}"))),
            };
        }
        if scans::is_tiff(&name) {
            let src = path.to_path_buf();
            return scans::cached(&self.cache, path, "png", move || {
                genmetric_core::guarded("TIFF", move || genmetric_core::tiff_png::to_png(&src))
            })
            .map(|b| ("image/png", b))
            .map_err(|e| (415, e));
        }
        Err((404, "этот файл протокол сканов не отдаёт".into()))
    }
}

/// Ответ протоколу `scan`. В отдельном потоке: разбор PDF и перекодирование
/// TIFF не должны держать окно.
pub fn handle(app: tauri::AppHandle, request: tauri::http::Request<Vec<u8>>, responder: tauri::UriSchemeResponder) {
    use tauri::Manager;
    std::thread::spawn(move || {
        let full = scans::percent_decode(request.uri().path().trim_start_matches('/'));
        // Паника при разборе — тоже ответ: иначе лист «висит» без пометки.
        let got = match app.try_state::<Sheets>() {
            Some(sheets) => genmetric_core::caught(|| sheets.serve(&full))
                .unwrap_or_else(|why| Err((500, format!("внутренняя ошибка программы ({why})")))),
            None => Err((503, "программа ещё не готова".into())),
        };
        let builder = tauri::http::Response::builder()
            // Без разрешения CORS просмотрщик в режиме WebGL картинку не возьмёт.
            .header("Access-Control-Allow-Origin", "*")
            .header("Cache-Control", "no-cache");
        let response = match got {
            Ok((kind, body)) => builder.header("Content-Type", kind).body(body),
            Err((status, why)) => {
                if status != 415 {
                    if let Some(app_state) = app.try_state::<crate::App>() {
                        crate::write_log(&app_state.log_path, &format!("Лист скана «{full}» не отдан: {why}"));
                    }
                }
                builder.status(status).header("Content-Type", "text/plain; charset=utf-8").body(why.into_bytes())
            }
        };
        match response {
            Ok(r) => responder.respond(r),
            Err(e) => responder.respond(tauri::http::Response::new(e.to_string().into_bytes())),
        }
    });
}
