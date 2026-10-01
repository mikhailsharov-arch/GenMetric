//! Выгрузка в Familio и в Excel (сборка #39, Роман 30.09.2026).
//!
//! Строки листов собирают запросы из db/statements.sql (блоки familio_* и
//! excel_*): каждый отдаёт колонки ровно в порядке колонок листа. Те же
//! запросы проверяет db/test_export.py. Здесь — выполнить запрос и положить
//! строки в шаблон (xlsx.rs). Запись файла на диск и команды окна — в
//! программе (src-tauri/src/export.rs).

use rusqlite::types::ValueRef;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::statement;
use crate::xlsx::{fill_template, CellValue, SheetRows};

/// Образец Familio ver.2025-12fd, присланный Романом 30.09.2026. Mike
/// 30.09.2026 разрешил держать его в репозитории.
pub const FAMILIO_TEMPLATE: &[u8] = include_bytes!("../../../db/export/familio_template.xlsx");

/// Шапка и оформление листов индексатора (МК, 1, 2, 3) — собраны из его
/// файла инструментом db/tools/make_excel_template.py, без данных.
pub const EXCEL_TEMPLATE: &[u8] = include_bytes!("../../../db/export/excel_template.xlsx");

#[derive(Serialize)]
pub struct YearCount {
    /// None — записи без года (выгружаются только со всем приходом).
    pub year: Option<i64>,
    pub births: i64,
    pub marriages: i64,
    pub deaths: i64,
}

/// Годы книги, по которым есть записи: для окна выгрузки в Familio.
pub fn years(conn: &Connection) -> Result<Vec<YearCount>, String> {
    let sql = statement("export_years")?;
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| Ok(YearCount {
        year: r.get(0)?, births: r.get(1)?, marriages: r.get(2)?, deaths: r.get(3)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Карточка «Для Familio» — строки листа about.
#[derive(Deserialize)]
pub struct About {
    pub title: String,
    pub years: String,
    pub min_year: String,
    pub description: String,
    pub author: String,
    pub profile: String,
    pub telegram: String,
    pub is_new: bool,
    pub previous: String,
}

#[derive(Serialize, Default)]
pub struct Exported {
    pub path: String,
    pub births: usize,
    pub marriages: usize,
    pub deaths: usize,
    pub places: usize,
    pub persons: usize,
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

/// Имя файла без символов, запрещённых в Windows.
pub fn safe_name(s: &str) -> String {
    s.chars().map(|c| if "\\/:*?\"<>|".contains(c) || (c as u32) < 0x20 { '_' } else { c })
        .collect::<String>().trim().trim_end_matches('.').to_string()
}

/// Название прихода для имени файла: село из дела.
pub fn parish_name(conn: &Connection) -> String {
    statement("export_parish")
        .and_then(|sql| conn.query_row(&sql, [], |r| r.get::<_, String>(0)).map_err(|e| e.to_string()))
        .unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xlsx::read_sheet;
    use std::path::PathBuf;

    /// Выгрузка на настоящей базе — её собирает db/test_export.py:
    ///   GENMETRIC_KEEP_DB=/tmp/t.sqlite python3 db/test_export.py
    ///   GENMETRIC_EXPORT_DB=/tmp/t.sqlite GENMETRIC_EXPORT_OUT=/tmp/out \
    ///     cargo test -p genmetric-core export_real -- --ignored
    /// Пишет familio.xlsx (весь приход), familio_1887.xlsx (один год) и
    /// excel.xlsx — их проверяет валидатор Open XML (scripts/xlsx-validate).
    /// Так идёт в быстрой проверке конвейера.
    #[test]
    #[ignore]
    fn export_real() {
        let db = std::env::var("GENMETRIC_EXPORT_DB").expect("GENMETRIC_EXPORT_DB");
        let out = PathBuf::from(std::env::var("GENMETRIC_EXPORT_OUT").expect("GENMETRIC_EXPORT_OUT"));
        std::fs::create_dir_all(&out).unwrap();
        let conn = Connection::open(db).unwrap();
        let about = About {
            title: "Метрические книги Борисоглебское за 1886-1887 годы".into(), years: "1886-1887".into(),
            min_year: "1886".into(), description: "".into(), author: "Роман Чистов".into(),
            profile: "".into(), telegram: "@r4istov".into(), is_new: true, previous: "".into(),
        };
        let (bytes, c) = familio_bytes(&conn, &[], &about).unwrap();
        std::fs::write(out.join("familio.xlsx"), &bytes).unwrap();
        assert!(c.births > 0, "в тестовой базе есть рождения");
        // То, что увидит человек: строк данных на листе столько, сколько записей.
        let sheet = read_sheet(&bytes, "РОЖДЕНИЕ").unwrap();
        assert_eq!(sheet.keys().filter(|r| **r >= 4).count(), c.births);
        assert_eq!(sheet[&2][&1], "№ п/п", "шапка образца на месте");

        // Один год: в файле только он.
        let (bytes, one) = familio_bytes(&conn, &[1887], &about).unwrap();
        std::fs::write(out.join("familio_1887.xlsx"), &bytes).unwrap();
        assert!(one.births < c.births && one.births > 0, "за 1887 — часть рождений");
        let sheet = read_sheet(&bytes, "РОЖДЕНИЕ").unwrap();
        assert!(sheet.iter().filter(|(r, _)| **r >= 4).all(|(_, row)| row[&15] == "#1887"), "год — 1887");

        let (bytes, e) = excel_bytes(&conn).unwrap();
        std::fs::write(out.join("excel.xlsx"), &bytes).unwrap();
        let mk = read_sheet(&bytes, "МК").unwrap();
        assert_eq!(mk.keys().filter(|r| **r >= 5).count(), e.persons);
        println!("familio: {} {} {} {}; excel: {}", c.births, c.marriages, c.deaths, c.places, e.persons);
    }
}
