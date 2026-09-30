//! Выгрузка в Familio и в Excel (сборка #39, Роман 30.09.2026).
//!
//! Строки листов собирают запросы из db/statements.sql (блоки familio_* и
//! excel_*): каждый отдаёт колонки ровно в порядке колонок листа. Те же
//! запросы проверяет db/test_export.py — выгрузка проверяется без Rust.
//! Здесь только выполнить запрос и положить строки в шаблон (xlsx.rs).
//!
//! Файл пишется в «Документы/GenMetric»: без окна выбора места — так его
//! находит и сквозная проверка на Windows (scripts/e2e/windows.py).

use std::path::PathBuf;

use rusqlite::types::ValueRef;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::xlsx::{fill_template, CellValue, SheetRows};
use crate::{statement, with_conn, App};

/// Образец Familio ver.2025-12fd, присланный Романом 30.09.2026. Mike
/// 30.09.2026 разрешил держать его в репозитории.
const FAMILIO_TEMPLATE: &[u8] = include_bytes!("../../db/export/familio_template.xlsx");

/// Шапка листов индексатора (МК, 1, 2, 3) — собрана из его файла
/// инструментом db/tools/make_excel_template.py, только шапка, без данных.
const EXCEL_TEMPLATE: &[u8] = include_bytes!("../../db/export/excel_template.xlsx");

#[derive(Serialize)]
pub struct YearCount {
    /// None — записи без года (выгружаются только со всем приходом).
    year: Option<i64>,
    births: i64,
    marriages: i64,
    deaths: i64,
}

/// Годы книги, по которым есть записи: для окна выгрузки в Familio.
#[tauri::command]
pub fn export_years(app: State<App>) -> Result<Vec<YearCount>, String> {
    with_conn(&app, "Годы для выгрузки", |conn| {
        let sql = statement("export_years")?;
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| Ok(YearCount {
            year: r.get(0)?, births: r.get(1)?, marriages: r.get(2)?, deaths: r.get(3)?,
        })).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    })
}

/// Карточка «Для Familio» — строки листа about.
#[derive(Deserialize)]
pub struct About {
    title: String,
    years: String,
    min_year: String,
    description: String,
    author: String,
    profile: String,
    telegram: String,
    is_new: bool,
    previous: String,
}

#[derive(Serialize, Default)]
pub struct Exported {
    path: String,
    births: usize,
    marriages: usize,
    deaths: usize,
    places: usize,
    persons: usize,
}

/// Временные таблицы выгрузки (x_*): считаются один раз на выгрузку.
fn prepare(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(&statement("export_prepare")?).map_err(|e| format!("подготовка выгрузки: {e}"))
}

/// Все колонки листа: число без ведущих нулей пишется числом (№, счёт,
/// день, месяц, год, возраст), остальное — текстом.
fn all_columns() -> Vec<u32> {
    (1..=120).collect()
}

/// Строки запроса: каждая колонка — текстом, пустое — None.
pub fn query_rows(conn: &Connection, sql: &str, years_json: &str) -> Result<Vec<Vec<Option<String>>>, String> {
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let n = stmt.column_count();
    let takes_years = stmt.parameter_index(":years").map_err(|e| e.to_string())?.is_some();
    let mut rows = if takes_years {
        stmt.query(rusqlite::named_params! { ":years": years_json })
    } else {
        stmt.query([])
    }.map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let mut vals = Vec::with_capacity(n);
        for i in 0..n {
            let v = match row.get_ref(i).map_err(|e| e.to_string())? {
                ValueRef::Null => None,
                ValueRef::Integer(x) => Some(x.to_string()),
                ValueRef::Real(x) => Some(x.to_string()),
                ValueRef::Text(t) => Some(String::from_utf8_lossy(t).trim().to_string()),
                ValueRef::Blob(_) => None,
            };
            vals.push(v.filter(|s| !s.is_empty()));
        }
        out.push(vals);
    }
    Ok(out)
}

/// Папка выгрузки: «Документы/GenMetric».
fn export_dir(handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = handle.path().document_dir().map_err(|e| format!("не найдена папка «Документы»: {e}"))?
        .join("GenMetric");
    std::fs::create_dir_all(&dir).map_err(|e| format!("не удалось создать папку {}: {e}", dir.display()))?;
    Ok(dir)
}

/// Имя файла без символов, запрещённых в Windows.
fn safe_name(s: &str) -> String {
    s.chars().map(|c| if "\\/:*?\"<>|".contains(c) || (c as u32) < 0x20 { '_' } else { c })
        .collect::<String>().trim().trim_end_matches('.').to_string()
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

/// Название прихода для имени файла: село из дела.
fn parish_name(conn: &Connection) -> String {
    statement("export_parish")
        .and_then(|sql| conn.query_row(&sql, [], |r| r.get::<_, String>(0)).map_err(|e| e.to_string()))
        .unwrap_or_default()
}

fn write_file(dir: PathBuf, name: &str, bytes: &[u8]) -> Result<String, String> {
    let path = dir.join(name);
    std::fs::write(&path, bytes).map_err(|e| {
        format!("не удалось записать {} ({e}); если файл открыт в Excel — закройте его", path.display())
    })?;
    Ok(path.to_string_lossy().to_string())
}

/// Файл для Familio: строки листов и карточка about. Отдельно от команды —
/// её проверяет тест на настоящей базе (export_real ниже).
pub fn familio_bytes(conn: &Connection, years: &[i64], about: &About)
    -> Result<(Vec<u8>, Exported), String>
{
    let years_json = serde_json::to_string(years).map_err(|e| e.to_string())?;
    prepare(conn)?;
    // Имя блока — литералом: так его видит проверка в test_incidents.py.
    let births = query_rows(conn, &statement("familio_birth")?, &years_json)?;
    let marriages = query_rows(conn, &statement("familio_marriage")?, &years_json)?;
    let deaths = query_rows(conn, &statement("familio_death")?, &years_json)?;
    let places = query_rows(conn, &statement("familio_location")?, &years_json)?;
    let counts = Exported {
        births: births.len(), marriages: marriages.len(), deaths: deaths.len(), places: places.len(),
        ..Default::default()
    };
    let b = |cell: &str, value: &str| CellValue {
        sheet: "about".into(), cell: cell.into(), value: value.trim().to_string(),
    };
    let cells = vec![
        b("B4", &about.title), b("B5", &about.years), b("B6", &about.min_year),
        b("B7", &about.description), b("B9", &about.author), b("B10", &about.profile),
        b("B11", &about.telegram),
        // Отчества в основных полях — современные (Роман 30.09.2026).
        b("B13", "современный"),
        b("B19", if about.is_new { "новый справочник" } else { "обновление начатого ранее" }),
        b("B20", if about.is_new { "" } else { &about.previous }),
    ];
    let bytes = fill_template(FAMILIO_TEMPLATE, &[
        SheetRows { sheet: "location".into(), first_row: 3, rows: places },
        SheetRows { sheet: "РОЖДЕНИЕ".into(), first_row: 4, rows: births },
        SheetRows { sheet: "БРАК".into(), first_row: 4, rows: marriages },
        SheetRows { sheet: "СМЕРТЬ".into(), first_row: 4, rows: deaths },
    ], &cells, &[("location", all_columns()), ("РОЖДЕНИЕ", all_columns()),
                 ("БРАК", all_columns()), ("СМЕРТЬ", all_columns())])?;
    Ok((bytes, counts))
}

/// Файл Excel «для себя»: весь приход, листы как в индексаторе.
pub fn excel_bytes(conn: &Connection) -> Result<(Vec<u8>, Exported), String> {
    let all = "[]";
    prepare(conn)?;
    let mk = query_rows(conn, &statement("excel_mk")?, all)?;
    let births = query_rows(conn, &statement("excel_births")?, all)?;
    let marriages = query_rows(conn, &statement("excel_marriages")?, all)?;
    let deaths = query_rows(conn, &statement("excel_deaths")?, all)?;
    let counts = Exported {
        births: births.len(), marriages: marriages.len(), deaths: deaths.len(), persons: mk.len(),
        ..Default::default()
    };
    let bytes = fill_template(EXCEL_TEMPLATE, &[
        SheetRows { sheet: "МК".into(), first_row: 5, rows: mk },
        SheetRows { sheet: "Рождения".into(), first_row: 3, rows: births },
        SheetRows { sheet: "Браки".into(), first_row: 3, rows: marriages },
        SheetRows { sheet: "Смерти".into(), first_row: 3, rows: deaths },
    ], &[], &[("МК", all_columns()), ("Рождения", all_columns()),
              ("Браки", all_columns()), ("Смерти", all_columns())])?;
    Ok((bytes, counts))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Выгрузка на настоящей базе: GENMETRIC_EXPORT_DB — путь к базе,
    /// GENMETRIC_EXPORT_OUT — папка для двух файлов. Запуск:
    ///   cargo test --bin genmetric export_real -- --ignored
    #[test]
    #[ignore]
    fn export_real() {
        let db = std::env::var("GENMETRIC_EXPORT_DB").expect("GENMETRIC_EXPORT_DB");
        let out = PathBuf::from(std::env::var("GENMETRIC_EXPORT_OUT").expect("GENMETRIC_EXPORT_OUT"));
        let conn = Connection::open(db).unwrap();
        let about = About {
            title: "Метрические книги Борисоглебское за 1886-1887 годы".into(), years: "1886-1887".into(),
            min_year: "1886".into(), description: "".into(), author: "Роман Чистов".into(),
            profile: "".into(), telegram: "@r4istov".into(), is_new: true, previous: "".into(),
        };
        let (bytes, c) = familio_bytes(&conn, &[], &about).unwrap();
        std::fs::write(out.join("familio.xlsx"), bytes).unwrap();
        let (bytes, e) = excel_bytes(&conn).unwrap();
        std::fs::write(out.join("excel.xlsx"), bytes).unwrap();
        println!("familio: {} {} {} {}; excel: {}", c.births, c.marriages, c.deaths, c.places, e.persons);
    }
}
