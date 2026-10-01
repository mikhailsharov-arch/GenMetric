//! Команды окна для выгрузки в Familio и в Excel. Сама выгрузка — в крейте
//! genmetric-core (export.rs, xlsx.rs): он без Tauri, и его тесты идут в
//! быстрой проверке конвейера. Здесь — папка, имя файла, запись на диск.
//!
//! Файл пишется в «Документы/GenMetric»: без окна выбора места — так его
//! находит и сквозная проверка на Windows (scripts/e2e/windows.py).

use std::path::PathBuf;

use genmetric_core::export::{excel_bytes, familio_bytes, parish_name, safe_name, About, Exported, YearCount};
use tauri::{Manager, State};

use crate::{with_conn, App};

/// Годы книги, по которым есть записи: для окна выгрузки в Familio.
#[tauri::command]
pub fn export_years(app: State<App>) -> Result<Vec<YearCount>, String> {
    with_conn(&app, "Годы для выгрузки", genmetric_core::export::years)
}

/// Папка выгрузки: «Документы/GenMetric».
fn export_dir(handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = handle.path().document_dir().map_err(|e| format!("не найдена папка «Документы»: {e}"))?
        .join("GenMetric");
    std::fs::create_dir_all(&dir).map_err(|e| format!("не удалось создать папку {}: {e}", dir.display()))?;
    Ok(dir)
}

/// «30.09.2026 14:05» → «2026-09-30_14-05-37» для имени файла. С секундами:
/// вторая выгрузка в ту же минуту не должна молча затирать первую (ревьюер #39).
fn file_stamp() -> String {
    let t = crate::timestamp();
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() % 60).unwrap_or(0);
    let (date, time) = t.split_once(' ').unwrap_or((&t, ""));
    let parts: Vec<&str> = date.split('.').collect();
    if parts.len() == 3 {
        format!("{}-{}-{}_{}-{secs:02}", parts[2], parts[1], parts[0], time.replace(':', "-"))
    } else {
        safe_name(&t)
    }
}

fn write_file(dir: PathBuf, name: &str, bytes: &[u8]) -> Result<String, String> {
    let path = dir.join(name);
    std::fs::write(&path, bytes).map_err(|e| {
        format!("не удалось записать {} ({e}); если файл открыт в Excel — закройте его", path.display())
    })?;
    Ok(path.to_string_lossy().to_string())
}

/// Выгрузка в Familio: выбранные годы (пусто — весь приход) и карточка about.
/// async — не в главном потоке: на 20 000 записей выгрузка идёт секунды, и
/// синхронная команда держала бы окно «не отвечает» (проверяющий #39).
#[tauri::command]
pub async fn export_familio(handle: tauri::AppHandle, app: State<'_, App>, years: Vec<i64>, about: About)
    -> Result<Exported, String>
{
    let dir = export_dir(&handle)?;
    with_conn(&app, "Выгрузка в Familio", |conn| {
        let (bytes, counts) = familio_bytes(conn, &years, &about)?;
        let name = format!("Familio_{}_{}_{}.xlsx", safe_name(&parish_name(conn)),
                           safe_name(&about.years), file_stamp());
        Ok(Exported { path: write_file(dir, &name, &bytes)?, ..counts })
    })
}

/// Выгрузка в Excel для себя: весь приход, без окон.
#[tauri::command]
pub async fn export_excel(handle: tauri::AppHandle, app: State<'_, App>) -> Result<Exported, String> {
    let dir = export_dir(&handle)?;
    with_conn(&app, "Выгрузка в Excel", |conn| {
        let (bytes, counts) = excel_bytes(conn)?;
        let name = format!("GenMetric_{}_{}.xlsx", safe_name(&parish_name(conn)), file_stamp());
        Ok(Exported { path: write_file(dir, &name, &bytes)?, ..counts })
    })
}

/// «Показать в папке»: проводник Windows или Finder с выделенным файлом.
#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), String> {
    #[cfg(windows)]
    let result = {
        // Как есть, без кавычек вокруг всего аргумента: с пробелом в пути
        // («Мои документы») Rust взял бы в кавычки и «/select,», и проводник
        // открыл бы не ту папку (ревьюер #39).
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer").raw_arg(format!("/select,\"{path}\"")).spawn()
    };
    #[cfg(not(windows))]
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg("-R").arg(&path).spawn()
    } else {
        let dir = std::path::Path::new(&path).parent().map(|p| p.to_path_buf()).unwrap_or_default();
        std::process::Command::new("xdg-open").arg(dir).spawn()
    };
    result.map(|_| ()).map_err(|e| format!("не удалось открыть папку: {e}"))
}

