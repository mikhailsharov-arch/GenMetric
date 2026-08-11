// GenMetric — индексатор метрических книг
// Лицензия GPL-3.0-or-later, см. файл LICENSE в корне репозитория.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use tauri::path::BaseDirectory;
use tauri::{Manager, State};

struct Db {
    conn: Mutex<Connection>,
    path: String,
}

#[derive(Serialize)]
struct DbInfo {
    names: i64,
    lookups: i64,
    roles: i64,
    db_path: String,
    app_version: String,
}

#[derive(Serialize)]
struct ParsedIof {
    first_name: Option<String>,
    first_name_modern: Option<String>,
    patronymic: Option<String>,
    patronymic_modern: Option<String>,
    surname: Option<String>,
    gender: Option<String>,
    father_name: Option<String>,
    known_name: bool,
}

#[derive(Serialize)]
struct Suggestion {
    value: String,
    tier: i64,
    count: i64,
}

/// Ключ для поиска по префиксу.
///
/// Обязан совпадать с norm() в db/build_seed.py, иначе подсказки перестанут
/// находиться. Встроенный в SQLite COLLATE NOCASE кириллицу не понимает,
/// поэтому нормализуем сами.
fn normalize(input: &str) -> String {
    let lowered = input.trim().to_lowercase().replace('ё', "е");
    lowered.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Экранирование спецсимволов LIKE, чтобы введённые % и _ искались буквально.
fn like_prefix(input: &str) -> String {
    let escaped = normalize(input)
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("{escaped}%")
}

#[tauri::command]
fn db_info(app: tauri::AppHandle, db: State<Db>) -> Result<DbInfo, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let count = |sql: &str| -> Result<i64, String> {
        conn.query_row(sql, [], |r| r.get(0)).map_err(|e| e.to_string())
    };
    Ok(DbInfo {
        names: count("SELECT count(*) FROM name_dict")?,
        lookups: count("SELECT count(*) FROM lookup")?,
        roles: count("SELECT count(*) FROM role")?,
        db_path: db.path.clone(),
        app_version: app.package_info().version.to_string(),
    })
}

/// Подсказки по префиксу.
///
/// Порядок выдачи задан требованием А-1: текущее дело, затем приход, затем вся
/// база, затем словарь; внутри группы — по убыванию частоты. Дела в этой сборке
/// ещё нет, поэтому работают только уровни «вся база» и «словарь».
#[tauri::command]
fn suggest(db: State<Db>, kind: String, prefix: String, limit: Option<i64>) -> Result<Vec<Suggestion>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let pattern = like_prefix(&prefix);
    let limit = limit.unwrap_or(8).clamp(1, 50);

    // Имена лежат в отдельной таблице со словарными формами отчеств,
    // остальные перечни — в общей таблице lookup.
    let dict_source = match kind.as_str() {
        "first_name" => "SELECT form, 4, 0 FROM name_form WHERE kind IN ('name','variant') AND form_norm LIKE ?2 ESCAPE '\\'",
        "patronymic" => "SELECT form, 4, 0 FROM name_form WHERE kind LIKE 'patr%' AND form_norm LIKE ?2 ESCAPE '\\'",
        _ => "SELECT value, 4, 0 FROM lookup WHERE kind = ?1 AND value_norm LIKE ?2 ESCAPE '\\'",
    };
    let sql = format!(
        "WITH ranked AS (
             SELECT value,
                    CASE scope WHEN 'case' THEN 1 WHEN 'parish' THEN 2 ELSE 3 END AS tier,
                    count
               FROM usage_stat
              WHERE kind = ?1 AND value_norm LIKE ?2 ESCAPE '\\'
             UNION ALL
             {dict_source}
         )
         SELECT value, min(tier) AS tier, max(count) AS cnt
           FROM ranked GROUP BY value ORDER BY tier, cnt DESC, value LIMIT ?3"
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(rusqlite::params![kind, pattern, limit], |r| {
            Ok(Suggestion { value: r.get(0)?, tier: r.get(1)?, count: r.get(2)? })
        })
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// Разбор строки ИОФ на имя, отчество и фамилию.
///
/// Порядок в метрических книгах: имя, отчество, фамилия. Отчество опознаётся
/// по словарю — и старая форма «Алексеев», и современная «Алексеевич», —
/// поэтому второе слово, которого среди отчеств нет, считается фамилией.
///
/// Написание не подменяется: в полях остаётся то, что написано в книге,
/// а современный вариант выдаётся отдельно, как предложение.
///
/// Проверено на 3664 персонах из настоящей работы: разбиение верно в 99,7%
/// случаев, предложенное осовременивание совпадает с выбором индексатора
/// в 99,9%.
#[tauri::command]
fn parse_iof(db: State<Db>, text: String) -> Result<ParsedIof, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let mut out = ParsedIof {
        first_name: None, first_name_modern: None,
        patronymic: None, patronymic_modern: None,
        surname: None, gender: None, father_name: None, known_name: false,
    };
    if tokens.is_empty() {
        return Ok(out);
    }

    out.first_name = Some(tokens[0].to_string());
    // priority 0 — заголовочное написание, 1 — вариант: точное совпадение
    // с самостоятельным именем важнее совпадения с вариантом другого.
    let head: Option<(String, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT d.name, d.base_name, d.gender
               FROM name_form f JOIN name_dict d ON d.id = f.name_id
              WHERE f.kind IN ('name','variant') AND f.form_norm = ?1
              ORDER BY f.priority LIMIT 1",
            [normalize(tokens[0])],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    if let Some((name, base, gender)) = head {
        out.known_name = true;
        out.gender = gender;
        out.first_name_modern = Some(base.unwrap_or(name));
    }

    let mut rest = &tokens[1..];
    if let Some(first_rest) = rest.first() {
        let patr: Option<(String, String, String)> = conn
            .query_row(
                "SELECT d.name,
                        CASE WHEN f.kind LIKE '%_m' THEN d.patr_m ELSE d.patr_f END,
                        CASE WHEN f.kind LIKE '%_m' THEN 'М' ELSE 'Ж' END
                   FROM name_form f JOIN name_dict d ON d.id = f.name_id
                  WHERE f.kind LIKE 'patr%' AND f.form_norm = ?1
                  ORDER BY f.priority LIMIT 1",
                [normalize(first_rest)],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some((father, modern, sex)) = patr {
            out.patronymic = Some(first_rest.to_string());
            out.patronymic_modern = Some(modern);
            out.father_name = Some(father);
            if out.gender.is_none() {
                out.gender = Some(sex);
            }
            rest = &rest[1..];
        }
    }
    if !rest.is_empty() {
        out.surname = Some(rest.join(" "));
    }
    Ok(out)
}

#[tauri::command]
fn set_always_on_top(window: tauri::Window, value: bool) -> Result<(), String> {
    window.set_always_on_top(value).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // База справочников поставляется внутри установщика и при первом
            // запуске копируется в папку данных пользователя: там её можно
            // пополнять, не трогая файлы программы.
            let bundled = app
                .path()
                .resolve("resources/seed.sqlite", BaseDirectory::Resource)?;
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("genmetric.sqlite");
            if !db_path.exists() {
                std::fs::copy(&bundled, &db_path)?;
            }

            let conn = Connection::open(&db_path)?;
            // WAL включается здесь, а не в schema.sql: это настройка соединения,
            // и в файле схемы она ломает сборку на сетевых файловых системах.
            //
            // Именно execute_batch, а не pragma_update: PRAGMA journal_mode
            // возвращает строку результата, и pragma_update на этом падает
            // с «Execute returned results».
            conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;

            app.manage(Db {
                conn: Mutex::new(conn),
                path: db_path.to_string_lossy().to_string(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![db_info, suggest, parse_iof, set_always_on_top])
        .run(tauri::generate_context!())
        .expect("не удалось запустить GenMetric");
}
