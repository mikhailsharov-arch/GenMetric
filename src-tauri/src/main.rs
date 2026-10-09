// GenMetric — индексатор метрических книг
// Лицензия GPL-3.0-or-later, см. файл LICENSE в корне репозитория.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::path::BaseDirectory;
use tauri::{Manager, State};

mod export;

use genmetric_core::parish;
use genmetric_core::records::{extend_lookup, parse_iof_in, save_case, save_entry, Case, CaseSaved, EntryInput, ParsedIof, Saved};
use genmetric_core::statement;
use genmetric_core::text::{normalize, normalize_name, normalize_words};



/// Слияние архива подсказок из Excel. Тот же файл прогоняет db/test_archive.py.
const IMPORT_ARCHIVE_SQL: &str = include_str!("../../db/import_archive.sql");

/// Состояние приложения.
///
/// Соединение хранится в Option: если база не открылась, программа всё равно
/// должна запуститься и объяснить человеку, что случилось, а не молча не
/// показать окно. Причина в этом случае лежит в startup_error.
struct App {
    conn: Mutex<Option<Connection>>,
    /// Файл открытого прихода. С 02.10.2026 приходов несколько, и открытый
    /// меняется на ходу (parish_open) — поэтому под замком.
    db_path: Mutex<String>,
    /// Открытый приход: номер в перечне и название.
    parish: Mutex<(i64, String)>,
    /// Не поломка, но сказать надо: справочники не сверены с общими, или
    /// открыт не тот приход, что в прошлый раз.
    warning: Mutex<Option<String>>,
    /// Файл Excel для импорта: приходит из окна частями (он 7 МБ; байты —
    /// обычными аргументами, инцидент 13.09.2026) и ждёт здесь команды.
    import_file: Mutex<Vec<u8>>,
    data_dir: PathBuf,
    /// База поставки внутри программы: из неё создаётся и обновляется каждый приход.
    bundled: Result<PathBuf, String>,
    log_path: PathBuf,
    startup_error: Option<String>,
}

#[derive(Serialize)]
struct DbInfo {
    names: i64,
    name_forms: i64,
    lookups: i64,
    places: i64,
    roles: i64,
    db_path: String,
    log_path: String,
    app_version: String,
    schema_version: i64,
    seed_stamp: String,
    /// Сколько записей починено при обновлении (номер девочек в женскую
    /// колонку). Человек должен видеть, что с его данными что-то сделали.
    repaired_entries: i64,
    /// Записи до 13.09 с ребёнком без пола и номером в мужской колонке —
    /// их программа не чинит, человек правит сам.
    unknown_sex_entries: i64,
    /// Записи с причтом без имени (сборки 21–22.09 после перезапуска).
    clergy_noname_entries: i64,
}


#[derive(Serialize)]
struct Suggestion {
    value: String,
    tier: i64,
    count: i64,
}

#[derive(Serialize)]
struct LookupSize {
    kind: String,
    title: String,
    count: i64,
}

#[derive(Serialize)]
struct Startup {
    error: Option<String>,
    db_path: String,
    log_path: String,
    parish_id: i64,
    parish_name: String,
    warning: Option<String>,
}

// ============================================================================
//  Журнал
//
//  13.08.2026 половина сборки молча не работала: интерфейс глушил ошибку
//  пустым перехватом, и поломка выглядела как «просто ничего не происходит».
//  Это стоило тестировщику целого цикла проверки. Теперь каждая неудача
//  и называется вслух на экране, и остаётся в файле — журнал можно прислать
//  целиком, не пересказывая.
// ============================================================================

fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Без внешних библиотек: простой пересчёт секунд в дату по григорианскому
    // календарю. Точности до минуты для журнала достаточно.
    let days = secs / 86_400;
    let rest = secs % 86_400;
    let (h, m) = (rest / 3600, (rest % 3600) / 60);
    let mut year = 1970_i64;
    let mut d = days as i64;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let len = if leap { 366 } else { 365 };
        if d < len {
            break;
        }
        d -= len;
        year += 1;
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let months = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 0usize;
    while month < 12 && d >= months[month] {
        d -= months[month];
        month += 1;
    }
    format!("{:02}.{:02}.{} {:02}:{:02}", d + 1, month + 1, year, h, m)
}

fn write_log(path: &Path, text: &str) {
    // Журнал не должен ронять программу: если записать не удалось — молчим,
    // на экране сообщение всё равно появится.
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{}  {}", timestamp(), text);
    }
}

/// Оборачивает работу с базой: логирует неудачу и возвращает её наверх.
fn with_conn<T>(
    app: &State<App>,
    what: &str,
    body: impl FnOnce(&Connection) -> Result<T, String>,
) -> Result<T, String> {
    let guard = match app.conn.lock() {
        Ok(g) => g,
        Err(e) => {
            let msg = format!("{what}: не удалось получить доступ к базе ({e})");
            write_log(&app.log_path, &msg);
            return Err(msg);
        }
    };
    let conn = match guard.as_ref() {
        Some(c) => c,
        None => {
            let msg = format!("{what}: база не открыта");
            write_log(&app.log_path, &msg);
            return Err(msg);
        }
    };
    body(conn).map_err(|e| {
        let msg = format!("{what}: {e}");
        write_log(&app.log_path, &msg);
        msg
    })
}

/// То же, и сразу — сверка справочников с общим файлом: карточка пункта,
/// новое звание, решение окна сверки доезжают до других приходов без
/// перезапуска (спека 2026-10-02, п. 2.2). Сбой сверки сохранению не мешает:
/// сохранённое уже в приходе, а сверка повторится при следующем сохранении
/// и при открытии.
fn with_conn_shared<T>(
    app: &State<App>,
    what: &str,
    body: impl FnOnce(&Connection) -> Result<T, String>,
) -> Result<T, String> {
    with_conn(app, what, |conn| {
        let out = body(conn)?;
        if parish::has_common(conn) {
            if let Err(e) = parish::sync(conn) {
                write_log(&app.log_path, &format!("{what}: справочники не сверены с общими ({e})"));
            }
        }
        Ok(out)
    })
}

// ============================================================================
//  Поиск и разбор
// ============================================================================

/// Расстояние Дамерау — Левенштейна по символам (не байтам: кириллица).
/// Для поиска похожих имён и названий: «Букарина» ↔ «Бухарино» = 2.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for i in 0..=n { d[i][0] = i; }
    for j in 0..=m { d[0][j] = j; }
    for i in 1..=n {
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            let mut v = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = v;
        }
    }
    d[n][m]
}

/// Похожие из перечня (name_norm, значение, пол): сначала ближние по
/// расстоянию, при равенстве — с более длинным общим началом, потом по
/// алфавиту. Дальше max_dist не показываем: список из всего словаря никому
/// не нужен. Порог задаёт вызывающий: имена — треть длины, не меньше 3
/// (пример Романа «Пискарь» → «Кесарь» = 3); места — четверть, не меньше 2
/// («Букарина» → «Бухарино» = 2, а «Неверовка» к «Новодеревенька» уже нет:
/// в карточке Enter выбирает первое похожее, и ложное похожее опасно).
fn rank_similar(query: &str, items: Vec<(String, String, Option<String>)>, limit: usize,
                max_dist: usize) -> Vec<Similar>
{
    let q = query;
    let mut scored: Vec<(usize, usize, String, Option<String>)> = items
        .into_iter()
        .filter_map(|(norm, value, gender)| {
            let dist = edit_distance(q, &norm);
            if dist > max_dist { return None; }
            let prefix = q.chars().zip(norm.chars()).take_while(|(x, y)| x == y).count();
            Some((dist, prefix, value, gender))
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    scored.dedup_by(|a, b| a.2 == b.2);
    scored.into_iter().take(limit)
        .map(|(dist, _, value, gender)| Similar { value, gender, distance: dist as i64 })
        .collect()
}

#[derive(Serialize)]
struct Similar {
    value: String,
    gender: Option<String>,
    distance: i64,
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
fn startup_state(app: State<App>) -> Startup {
    let (parish_id, parish_name) = app.parish.lock().map(|p| p.clone()).unwrap_or((1, String::new()));
    Startup {
        error: app.startup_error.clone(),
        db_path: app.db_path.lock().map(|p| p.clone()).unwrap_or_default(),
        log_path: app.log_path.to_string_lossy().to_string(),
        parish_id,
        parish_name,
        // Предупреждение показывается один раз — после чтения снимается.
        warning: app.warning.lock().ok().and_then(|mut w| w.take()),
    }
}

#[tauri::command]
fn read_log(app: State<App>, lines: Option<usize>) -> String {
    let take = lines.unwrap_or(200);
    match std::fs::read_to_string(&app.log_path) {
        Ok(text) => {
            let all: Vec<&str> = text.lines().collect();
            let start = all.len().saturating_sub(take);
            let mut out = String::new();
            for line in &all[start..] {
                let _ = writeln!(out, "{line}");
            }
            if out.is_empty() {
                "Журнал пуст — ошибок не было.".to_string()
            } else {
                out
            }
        }
        Err(_) => "Журнал пуст — ошибок не было.".to_string(),
    }
}

/// Сколько значений в каждом перечне.
///
/// Роман искал «крестьянскую жену» в поле мужских званий и решил, что звания
/// пропали. Теперь состав всех перечней виден целиком и проверяется глазами,
/// а не по поведению одного поля.
#[tauri::command]
fn lookup_summary(app: State<App>) -> Result<Vec<LookupSize>, String> {
    with_conn(&app, "Состав справочников", |conn| {
        let mut stmt = conn
            .prepare(
                "SELECT k.kind, k.title, count(l.id)
                   FROM lookup_kind k LEFT JOIN lookup l ON l.kind = k.kind
                  GROUP BY k.kind, k.title
                  ORDER BY count(l.id) DESC, k.title",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(LookupSize { kind: r.get(0)?, title: r.get(1)?, count: r.get(2)? })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    })
}

#[tauri::command]
fn db_info(handle: tauri::AppHandle, app: State<App>) -> Result<DbInfo, String> {
    let db_path = app.db_path.lock().map(|p| p.clone()).unwrap_or_default();
    let log_path = app.log_path.to_string_lossy().to_string();
    let version = handle.package_info().version.to_string();
    with_conn(&app, "Сведения о базе", |conn| {
        let count = |sql: &str| -> Result<i64, String> {
            conn.query_row(sql, [], |r| r.get(0)).map_err(|e| e.to_string())
        };
        Ok(DbInfo {
            names: count("SELECT count(*) FROM name_dict")?,
            name_forms: count("SELECT count(*) FROM name_form")?,
            lookups: count("SELECT count(*) FROM lookup")?,
            // Населённые пункты — на этом экране человек проверяет, доехало ли
            // обновление. Справочник НП пришёл в поставке впервые, и если он
            // не появился, это надо увидеть здесь, а не гадать в форме.
            places: count("SELECT count(*) FROM place")?,
            roles: count("SELECT count(*) FROM role")?,
            db_path,
            log_path,
            app_version: version,
            schema_version: count("SELECT coalesce(max(version), 0) FROM schema_version")?,
            seed_stamp: conn
                .query_row("SELECT value FROM setting WHERE key = 'seed_stamp'", [], |r| r.get(0))
                .optional()
                .map_err(|e| e.to_string())?
                .unwrap_or_else(|| "нет".to_string()),
            repaired_entries: conn
                .query_row("SELECT CAST(value AS INTEGER) FROM setting WHERE key = 'repair_count_column'",
                           [], |r| r.get(0))
                .optional()
                .map_err(|e| e.to_string())?
                .unwrap_or(0),
            unknown_sex_entries: conn
                .query_row("SELECT CAST(value AS INTEGER) FROM setting WHERE key = 'repair_unknown_sex'",
                           [], |r| r.get(0))
                .optional()
                .map_err(|e| e.to_string())?
                .unwrap_or(0),
            clergy_noname_entries: conn
                .query_row("SELECT CAST(value AS INTEGER) FROM setting WHERE key = 'repair_clergy_noname'",
                           [], |r| r.get(0))
                .optional()
                .map_err(|e| e.to_string())?
                .unwrap_or(0),
        })
    })
}

/// Подсказки по префиксу.
///
/// Порядок выдачи задан требованием А-1: текущее дело, затем приход, затем вся
/// база, затем словарь; внутри группы — по убыванию частоты. Дела в этой сборке
/// ещё нет, поэтому работают только уровни «вся база» и «словарь».
#[tauri::command]
fn suggest(
    app: State<App>,
    kind: String,
    prefix: String,
    limit: Option<i64>,
    gender: Option<String>,
) -> Result<Vec<Suggestion>, String> {
    // Звания в книгах — в старой орфографии («крестьянинъ»): каждое слово
    // без конечного «ъ», с «і», «ѣ», «ѳ» по-современному (техдолг 23.09.2026).
    let pattern = if kind.starts_with("rank") {
        like_prefix(&normalize_words(&prefix))
    } else {
        like_prefix(&prefix)
    };
    // До 200: «весь перечень» по кнопке ▾ — губерний 115, архивов 59.
    // Проверяющий 21.09.2026: с пределом 50 из перечня пропадали «ЦГА Москвы»
    // и «Московская губерния», а тест этого не видел — он не ходит через Rust.
    let limit = limit.unwrap_or(8).clamp(1, 200);
    with_conn(&app, &format!("Подсказки «{prefix}»"), |conn| {
        // Имена и отчества лежат в таблице форм, населённые пункты — в place,
        // остальные перечни — в lookup. Сами запросы в db/statements.sql:
        // ошибка «НП ищется в lookup» прожила три недели именно потому, что
        // запрос был в коде и его нечем было проверить.
        // Сборка — в крейте (genmetric_core::suggest_sql), с тестом: ветка
        // частот тоже отбирается по полу — имена и отчества по словарю,
        // фамилии по окончанию.
        let (sql, gendered) = genmetric_core::suggest_sql(&kind)?;

        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        // :gender есть только в запросе отчеств — лишний именованный параметр
        // rusqlite считает ошибкой, поэтому список собирается по месту.
        let mut params: Vec<(&str, &dyn rusqlite::ToSql)> =
            vec![(":kind", &kind), (":prefix", &pattern), (":limit", &limit)];
        // Пол нужен отчествам, именам и фамилиям. Заказчик 13.09.2026: матери
        // подставлялись мужские имена — фильтровались только отчества.
        if gendered {
            params.push((":gender", &gender));
        }
        let rows = stmt
            .query_map(&params[..], |r| {
                Ok(Suggestion { value: r.get(0)?, tier: r.get(1)?, count: r.get(2)? })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    })
}

/// Звание по умолчанию: самое частое в приходе у этой роли и пола.
/// `role` — группа ролей формы: отец, жених, восприемник, поручитель.
#[tauri::command]
fn rank_default(app: State<App>, role: String, gender: Option<String>) -> Result<Option<String>, String> {
    let pattern = match role.as_str() {
        "father" => "father",
        "groom" => "groom",
        "godparent" => "godparent%",
        "witness" => "witness%",
        // Родственники жениха и невесты (Роман 09.10.2026); родственника
        // умершего исключает сам запрос (rank_default).
        "relative" => "%_relative",
        other => return Err(format!("Неизвестная роль для звания по умолчанию: {other}")),
    };
    with_conn(&app, "Звание по умолчанию", |conn| {
        genmetric_core::records::default_rank(conn, pattern, gender.as_deref())
    })
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
fn parse_iof(app: State<App>, text: String) -> Result<ParsedIof, String> {
    with_conn(&app, &format!("Разбор «{text}»"), |conn| parse_iof_in(conn, &text))
}



/// Похожие имена (kind = "name") или отчества ("patr") — для окна сверки.
/// Пол сужает список, если известен.
#[tauri::command]
fn similar_names(app: State<App>, text: String, kind: String, gender: Option<String>,
                 limit: Option<i64>) -> Result<Vec<Similar>, String> {
    with_conn(&app, &format!("Похожие на «{text}»"), |conn| {
        let sql = statement(if kind == "patr" { "patr_forms" } else { "name_headwords" })?;
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, String>(0)?,
                                   r.get::<_, Option<String>>(2)?)))
            .map_err(|e| e.to_string())?;
        let mut items = Vec::new();
        for row in rows {
            let (norm, value, g) = row.map_err(|e| e.to_string())?;
            if let (Some(want), Some(have)) = (&gender, &g) {
                if want != have { continue; }
            }
            items.push((norm, value, g));
        }
        let q = normalize_name(&text);
        let max_dist = (q.chars().count() / 3).max(3);
        Ok(rank_similar(&q, items, limit.unwrap_or(12).clamp(1, 50) as usize, max_dist))
    })
}

/// Поиск по началу слова для окна сверки — только словарь, см. dict_name_prefix.
#[tauri::command]
fn dict_search(app: State<App>, prefix: String, kind: String, gender: Option<String>,
               limit: Option<i64>) -> Result<Vec<Similar>, String> {
    with_conn(&app, &format!("Словарь по «{prefix}»"), |conn| {
        let sql = statement(if kind == "patr" { "dict_patr_prefix" } else { "dict_name_prefix" })?;
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::named_params! {
                ":prefix": like_prefix(&normalize_name(&prefix)), ":gender": gender,
                ":limit": limit.unwrap_or(12).clamp(1, 50),
            }, |r| Ok(Similar { value: r.get(0)?, gender: r.get(1)?, distance: 0 }))
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    })
}

/// «Запомнить» в окне сверки. target None — «новое имя» с полом.
#[tauri::command]
fn alias_save(app: State<App>, kind: String, form: String, target: Option<String>,
              gender: Option<String>) -> Result<(), String> {
    with_conn_shared(&app, &format!("Соответствие «{form}»"), |conn| {
        if kind != "name" && kind != "patr" {
            return Err(format!("Неизвестный вид соответствия: {kind}"));
        }
        conn.execute(&statement("alias_save")?, rusqlite::named_params! {
            ":kind": kind, ":form": form.trim(), ":form_norm": normalize_name(&form),
            ":target": target, ":gender": gender,
        }).map_err(|e| e.to_string())?;
        Ok(())
    })
}

/// Чтение настройки. Настройки живут в базе, поэтому переживают перезапуск.
#[tauri::command]
fn get_setting(app: State<App>, key: String) -> Result<Option<String>, String> {
    // Настройки окна — в общем файле, одни на все приходы (parish.rs).
    with_conn(&app, &format!("Чтение настройки «{key}»"), |conn| parish::setting_get(conn, &key))
}

#[tauri::command]
fn set_setting(app: State<App>, key: String, value: String) -> Result<(), String> {
    with_conn(&app, &format!("Запись настройки «{key}»"), |conn| parish::setting_set(conn, &key, &value))
}

#[tauri::command]
fn set_always_on_top(window: tauri::Window, value: bool) -> Result<(), String> {
    window.set_always_on_top(value).map_err(|e| e.to_string())
}

// ============================================================================
//  Открытие и обновление базы
// ============================================================================





// ============================================================================
//  Дело и записи
// ============================================================================





#[derive(Serialize)]
struct PersonHint {
    iof: String,
    place: Option<String>,
    rank: Option<String>,
    gender: Option<String>,
    uses: i64,
}

#[derive(Serialize)]
struct SpouseHint {
    iof: String,
    place: Option<String>,
    rank: Option<String>,
}

#[derive(Serialize)]
struct EntryBrief {
    id: i64,
    page: Option<String>,
    no_male: Option<i64>,
    no_female: Option<i64>,
    event_day: Option<i64>,
    event_month: Option<i64>,
    event_year: Option<i64>,
    rite_month: Option<i64>,
    child: Option<String>,
    father: Option<String>,
    clergy_noname: bool,
    groom: Option<String>,
    bride: Option<String>,
    deceased: Option<String>,
    rite_year: Option<i64>,
}

/// Дело года (year) или текущее — дело последней записи. Дело — на год
/// книги (спека 2026-10-02, п. 3); экран «Дело» остаётся одной формой.
#[tauri::command]
fn case_load(app: State<App>, year: Option<i64>) -> Result<Option<Case>, String> {
    with_conn(&app, "Чтение дела", |conn| {
        conn.query_row(&statement("case_current")?, rusqlite::named_params! { ":year": year },
            |r| Ok(Case {
                id: r.get(0)?, archive: r.get(1)?, fond: r.get(2)?, opis: r.get(3)?,
                delo: r.get(4)?, church: r.get(5)?, village: r.get(6)?, uyezd: r.get(7)?,
                guberniya: r.get(8)?, year: r.get(9)?, indexer: r.get(10)?,
            }),
        )
        .optional()
        .map_err(|e| e.to_string())
    })
}

#[derive(Serialize)]
struct CaseYear {
    year: i64,
    entries: i64,
}

/// Годы, у которых есть дело, — для выбора года на экране «Дело».
#[tauri::command]
fn case_years(app: State<App>) -> Result<Vec<CaseYear>, String> {
    with_conn(&app, "Годы дел", |conn| {
        let mut stmt = conn.prepare(&statement("case_years")?).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| Ok(CaseYear { year: r.get(0)?, entries: r.get(1)? }))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn case_save(app: State<App>, case: Case, overwrite: Option<bool>) -> Result<CaseSaved, String> {
    with_conn_shared(&app, "Сохранение дела", |conn| save_case(conn, &case, overwrite.unwrap_or(false)))
}

/// Убрать дело года, в котором нет ни одной записи. Возвращает, убрано ли.
#[tauri::command]
fn case_delete(app: State<App>, year: i64) -> Result<bool, String> {
    with_conn(&app, "Удаление пустого дела", |conn| {
        conn.execute(&statement("case_delete_empty")?, rusqlite::named_params! { ":year": year })
            .map(|n| n > 0)
            .map_err(|e| e.to_string())
    })
}

/// Сохранение записи со всеми упомянутыми персонами — genmetric_core::records.
#[tauri::command]
fn entry_save(app: State<App>, entry: EntryInput) -> Result<Saved, String> {
    with_conn_shared(&app, "Сохранение записи", |conn| save_entry(conn, &entry))
}

/// Подсказка персонами: ИОФ вместе с населённым пунктом и званием.
///
/// Главное требование заказчика от 17.08.2026. Раньше подсказки шли пословно —
/// отдельно имя, отдельно отчество, отдельно фамилия, — и населённый пункт
/// со званием приходилось набирать руками для каждой персоны.
#[tauri::command]
fn suggest_person(app: State<App>, prefix: String, limit: Option<i64>, gender: Option<String>,
                  prefer_infant: Option<bool>, keep_wife_rank: Option<bool>)
    -> Result<Vec<PersonHint>, String>
{
    // Умерший: младенцы из записей о рождении — первыми (Роман 28.09.2026).
    // Имя блока — литералом в каждой ветке: так его видит проверка
    // параметров в test_incidents.py.
    let sql = if prefer_infant.unwrap_or(false) { statement("person_suggest_infant")? }
              else { statement("person_suggest")? };
    let pattern = like_prefix(&prefix);
    let limit = limit.unwrap_or(8).clamp(1, 50);
    with_conn(&app, &format!("Поиск персоны «{prefix}»"), |conn| {
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::named_params! {
                ":prefix": pattern, ":limit": limit, ":gender": gender,
            }, |r| {
                Ok(PersonHint {
                    iof: r.get(0)?, place: r.get(1)?, rank: r.get(2)?,
                    gender: r.get(3)?, uses: r.get(4)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        drop(stmt);
        // «Законная жена его» — звание матери в записи о рождении; в других
        // ролях та же женщина — «крестьянская жена»: по званию мужа (Роман
        // 06.10.2026). В поле матери звание остаётся как есть.
        if !keep_wife_rank.unwrap_or(false) {
            let husband = statement("wife_husband_rank")?;
            for hint in out.iter_mut() {
                if hint.rank.as_deref().map(str::trim) != Some(genmetric_core::records::WIFE_RANK) {
                    continue;
                }
                let rank: Option<String> = conn
                    .query_row(&husband, rusqlite::named_params! { ":iof": hint.iof, ":place": hint.place },
                               |r| r.get(0))
                    .optional()
                    .map_err(|e| e.to_string())?;
                hint.rank = Some(genmetric_core::records::wife_rank(rank.as_deref()).to_string());
            }
        }
        Ok(out)
    })
}

/// Ребёнок из записи о рождении — строка подсказки ИОФ умершего.
#[derive(Serialize)]
struct InfantHint {
    iof: String,
    gender: Option<String>,
    /// «отец» или «мать» (если отца в записи нет).
    kin: Option<String>,
    parent: Option<String>,
    place: Option<String>,
    rank: Option<String>,
    /// Дата рождения «05.01.1886» — чтобы различать тёзок.
    born: Option<String>,
}

/// Дети из записей о рождении этого дела по началу имени — каждый отдельной
/// строкой с родителем (Роман 30.09.2026).
#[tauri::command]
fn suggest_infant(app: State<App>, prefix: String, limit: Option<i64>,
                  gender: Option<String>, year: Option<i64>, place: Option<String>)
    -> Result<Vec<InfantHint>, String>
{
    let pattern = like_prefix(&prefix);
    // «Евдокия Ив»: первое слово — имя ребёнка, остальное — начало имени
    // родителя (спека 2026-10-03, п. 2.3).
    let (name, parent) = match prefix.trim().split_once(char::is_whitespace) {
        Some((first, rest)) if !rest.trim().is_empty() => (Some(like_prefix(first)), Some(like_prefix(rest.trim()))),
        _ => (None, None),
    };
    let place = place.as_deref().map(normalize).filter(|p| !p.is_empty());
    let limit = limit.unwrap_or(6).clamp(1, 50);
    with_conn(&app, &format!("Поиск младенца «{prefix}»"), |conn| {
        let mut stmt = conn.prepare(&statement("infant_suggest")?).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::named_params! {
                ":prefix": pattern, ":gender": gender, ":limit": limit, ":year": year,
                ":name": name, ":parent": parent, ":place": place,
            }, |r| {
                let (d, mo, y): (Option<i64>, Option<i64>, Option<i64>) = (r.get(6)?, r.get(7)?, r.get(8)?);
                let part = |v: Option<i64>| v.map(|n| format!("{n:02}")).unwrap_or_else(|| "??".into());
                Ok(InfantHint {
                    iof: r.get(0)?, gender: r.get(1)?, kin: r.get(2)?, parent: r.get(3)?,
                    place: r.get(4)?, rank: r.get(5)?,
                    // Только год — без «??.??.»: так бывает у записей без даты.
                    born: y.map(|y| if d.is_none() && mo.is_none() { y.to_string() }
                                    else { format!("{}.{}.{y}", part(d), part(mo)) }),
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    })
}

#[derive(Serialize)]
struct FatherHint {
    iof: String,
    place: Option<String>,
    rank: Option<String>,
    /// Сколько записей о рождении с таким ребёнком в деле: больше одной —
    /// отец не подставляется, форма просит выбрать самому.
    births: i64,
}

/// Отец ребёнка из записи о рождении — для «Смертей»: выбрали умершего
/// младенца, родственник заполняется сам (Роман 28.09.2026).
#[tauri::command]
fn birth_father(app: State<App>, iof: String, year: Option<i64>, place: Option<String>)
    -> Result<Option<FatherHint>, String>
{
    let place = place.as_deref().map(normalize).filter(|p| !p.is_empty());
    with_conn(&app, "Отец из записи о рождении", |conn| {
        conn.query_row(&statement("birth_father")?, rusqlite::named_params! {
            ":iof": iof.trim(), ":year": year, ":place": place,
        }, |r| Ok(FatherHint { iof: r.get(0)?, place: r.get(1)?, rank: r.get(2)?, births: r.get(3)? }))
            .optional()
            .map_err(|e| e.to_string())
            .map(|o| o.filter(|f| !f.iof.is_empty()))
    })
}

/// Что дала загрузка архива — показывается человеку.
#[derive(Serialize)]
struct ImportReport {
    persons_added: i64,
    spouses_added: i64,
    clergy_added: i64,
    places_added: i64,
    source: String,
}

/// Загрузка архива подсказок, собранного из рабочего файла Excel-индексатора
/// (db/tools/build_archive.py). Файл приходит байтами из окна выбора файла
/// в интерфейсе — так не нужен ни плагин диалогов, ни доступ к путям.
///
/// Заказчик 13.09.2026 прислал файл с 2089 записями; замер на незнакомой
/// странице — 55 секунд на запись — сделан с пустой памятью подсказок.
/// Перенос памяти — самый большой рычаг из всех, что у нас остались.
#[tauri::command]
fn import_archive(app: State<App>, bytes: Vec<u8>) -> Result<ImportReport, String> {
    // Байты приходят обычным JSON-аргументом (массив чисел), а не сырым телом
    // запроса. Сырое тело (tauri::ipc::Request / InvokeBody::Raw) на Windows
    // 13.09.2026 не дошло: fetch на ipc.localhost у WebView2 не прошёл, Tauri
    // молча переключился на postMessage, и тело пришло уже как JSON. Роман
    // получил «ожидался файл архива, а пришло что-то другое». Через аргумент
    // работает любой транспорт; ~5 МБ JSON на разовую загрузку — терпимо.
    if bytes.is_empty() {
        return Err("файл пустой — пришло 0 байт".into());
    }
    if !bytes.starts_with(b"SQLite format 3\0") {
        return Err(format!(
            "это не файл архива GenMetric: нет заголовка SQLite (пришло {} байт, начало {:?})",
            bytes.len(),
            String::from_utf8_lossy(&bytes[..bytes.len().min(16)])
        ));
    }
    // Архив кладём рядом с базой во временный файл: SQLite подключает
    // только файлы, а не память.
    let db_path = app.db_path.lock().map(|p| p.clone()).unwrap_or_default();
    let tmp = Path::new(&db_path)
        .parent()
        .map(|d| d.join("archive-import.tmp.sqlite"))
        .ok_or("не найдена папка базы")?;
    std::fs::write(&tmp, &bytes).map_err(|e| format!("не удалось записать временный файл: {e}"))?;
    let tmp_str = tmp.to_string_lossy().to_string();

    let result = with_conn(&app, "Загрузка архива", |conn| {
        conn.execute("ATTACH DATABASE ?1 AS archive", [&tmp_str]).map_err(|e| e.to_string())?;
        let done = (|| -> Result<ImportReport, String> {
            let count = |sql: &str| -> Result<i64, String> {
                conn.query_row(sql, [], |r| r.get(0)).map_err(|e| e.to_string())
            };
            // Это архив GenMetric? У него есть наша таблица памяти и отметка источника.
            let is_archive = count(
                "SELECT count(*) FROM archive.sqlite_master WHERE name = 'person_index'")? == 1;
            if !is_archive {
                return Err("в файле нет памяти подсказок — это не архив GenMetric".into());
            }
            let source: String = conn
                .query_row(
                    "SELECT coalesce((SELECT value FROM archive.setting WHERE key='archive_source'),'?') \
                     || ' @ ' || coalesce((SELECT value FROM archive.setting WHERE key='archive_built'),'?')",
                    [], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            // Тот же архив второй раз — счётчики сложились бы повторно.
            let loaded: Option<String> = conn
                .query_row("SELECT value FROM setting WHERE key = 'archive_loaded'", [], |r| r.get(0))
                .optional()
                .map_err(|e| e.to_string())?;
            if loaded.as_deref() == Some(source.as_str()) {
                return Err(format!("этот архив уже загружен: {source}"));
            }
            let before = (
                count("SELECT count(*) FROM person_index")?,
                count("SELECT count(*) FROM spouse_index")?,
                count("SELECT count(*) FROM clergy_index")?,
                count("SELECT count(*) FROM place")?,
            );
            conn.execute_batch(IMPORT_ARCHIVE_SQL).map_err(|e| e.to_string())?;
            Ok(ImportReport {
                persons_added: count("SELECT count(*) FROM person_index")? - before.0,
                spouses_added: count("SELECT count(*) FROM spouse_index")? - before.1,
                clergy_added: count("SELECT count(*) FROM clergy_index")? - before.2,
                places_added: count("SELECT count(*) FROM place")? - before.3,
                source,
            })
        })();
        // Слияние — одна транзакция внутри import_archive.sql. Если пакет
        // упал посередине, транзакция осталась открытой: DETACH в ней не
        // выполнится, а соединение зависнет в ней до перезапуска. Откатываем
        // сами; когда транзакции нет, is_autocommit() это и скажет.
        if done.is_err() && !conn.is_autocommit() {
            let _ = conn.execute_batch("ROLLBACK");
        }
        // Отключаем архив в любом случае, иначе следующая загрузка упрётся
        // в «archive уже подключён». Ошибку отключения не глотаем.
        conn.execute_batch("DETACH DATABASE archive").map_err(|e| e.to_string())?;
        done
    });
    let _ = std::fs::remove_file(&tmp);
    result
}

/// Церковнослужитель из памяти: ИОФ вместе со званием.
#[derive(Serialize)]
struct ClergyHint {
    iof: String,
    rank: Option<String>,
    uses: i64,
}

/// Весь список причта. Не подсказка по префиксу, а именно список для выбора:
/// заказчик просил кнопку, открывающую перечень, а не ввод первых букв.
#[tauri::command]
fn list_clergy(app: State<App>, limit: Option<i64>) -> Result<Vec<ClergyHint>, String> {
    let limit = limit.unwrap_or(20).clamp(1, 100);
    with_conn(&app, "Список церковнослужителей", |conn| {
        let mut stmt = conn.prepare(&statement("clergy_list")?).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::named_params! { ":limit": limit }, |r| {
                Ok(ClergyHint { iof: r.get(0)?, rank: r.get(1)?, uses: r.get(2)? })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    })
}

/// Жена по мужу: выбор отца заполняет мать.
#[tauri::command]
fn suggest_spouse(app: State<App>, husband: String, place: Option<String>) -> Result<Option<SpouseHint>, String> {
    // НП мужа — чтобы не подставить жену его тёзки из другой деревни.
    let place = place.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
    with_conn(&app, "Поиск жены", |conn| {
        conn.query_row(
            &statement("spouse_lookup")?,
            rusqlite::named_params! { ":husband_norm": normalize(&husband), ":place": place },
            |r| Ok(SpouseHint { iof: r.get(0)?, place: r.get(1)?, rank: r.get(2)? }),
        )
        .optional()
        .map_err(|e| e.to_string())
    })
}

/// Экран «Поиск»: найденные персоны — один ИОФ в одном НП (search.rs).
#[tauri::command]
fn search_persons(app: State<App>, filter: genmetric_core::search::Filter)
    -> Result<genmetric_core::search::Found, String>
{
    with_conn(&app, &format!("Поиск персоны «{}»", filter.query), |conn| {
        genmetric_core::search::find(conn, &filter)
    })
}

/// Досье выбранной персоны: все записи, где стоит её ИОФ в этом НП.
#[tauri::command]
fn person_dossier(app: State<App>, key: String, place: String, filter: genmetric_core::search::Filter)
    -> Result<genmetric_core::search::Dossier, String>
{
    with_conn(&app, &format!("Досье «{key}»"), |conn| {
        genmetric_core::search::dossier(conn, &key, &place, &filter)
    })
}

#[derive(Serialize)]
struct PlaceCheck {
    known: bool,
    /// Написание известного пункта в справочнике — поле НП приводит к нему
    /// набранное («заборье (нежитино)» → «Заборье (Нежитино)»).
    canonical: Option<String>,
    similar: Vec<Similar>,
    /// Волость последнего пункта, заведённого человеком, — в карточку нового.
    last_volost: String,
}

/// Известен ли населённый пункт, и на что он похож, если нет. Форма
/// спрашивает при уходе из поля НП: неизвестный — карточка, похожий —
/// предложение выбрать («Букарина» → «Бухарино», Роман 13.09.2026).
#[tauri::command]
fn place_check(app: State<App>, name: String) -> Result<PlaceCheck, String> {
    with_conn(&app, &format!("Проверка НП «{name}»"), |conn| {
        let norm = normalize(&name);
        if norm.is_empty() {
            return Ok(PlaceCheck { known: true, canonical: None, similar: vec![], last_volost: String::new() });
        }
        let known: Option<String> = conn
            .query_row(&statement("place_find")?, rusqlite::named_params! { ":name_norm": norm },
                       |r| r.get::<_, String>(1))
            .optional()
            .map_err(|e| e.to_string())?;
        if known.is_some() {
            return Ok(PlaceCheck { known: true, canonical: known, similar: vec![], last_volost: String::new() });
        }
        let last_volost: String = conn
            .query_row(&statement("place_last_volost")?, [], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let mut stmt = conn.prepare(&statement("place_names")?).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, String>(0)?, None)))
            .map_err(|e| e.to_string())?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row.map_err(|e| e.to_string())?);
        }
        let max_dist = (norm.chars().count() / 4).max(2);
        // Деревни-тёзки с комментарием — первыми: название то же самое.
        let mut similar: Vec<Similar> = Vec::new();
        {
            let mut stmt = conn.prepare(&statement("place_commented")?).map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(|e| e.to_string())?;
            for row in rows {
                let (label, comment) = row.map_err(|e| e.to_string())?;
                if normalize(genmetric_core::text::place_clean(&label, &comment)) == norm {
                    similar.push(Similar { value: label, gender: None, distance: 0 });
                }
            }
        }
        for s in rank_similar(&norm, items, 8, max_dist) {
            if !similar.iter().any(|x| x.value == s.value) {
                similar.push(s);
            }
        }
        Ok(PlaceCheck { known: false, canonical: None, similar, last_volost })
    })
}

#[derive(Deserialize)]
struct PlaceCard {
    /// Чистое название — без комментария; метку «Название (комментарий)»
    /// собирает `place_label`.
    name: String,
    /// Комментарий деревни-тёзки (Роман 06.10.2026); пусто — нет.
    #[serde(default)]
    comment: Option<String>,
    np_type: Option<String>,
    guberniya: Option<String>,
    uyezd: Option<String>,
    volost: Option<String>,
    familio_url: Option<String>,
}

#[derive(Serialize)]
struct PlaceInfo {
    id: i64,
    name: String,
    np_type: Option<String>,
    guberniya: Option<String>,
    uyezd: Option<String>,
    volost: Option<String>,
    familio_url: Option<String>,
    origin: String,
    /// Комментарий деревни-тёзки и чистое название без него.
    comment: String,
    clean: String,
}

/// Карточка известного пункта — на правку. None, если пункта нет.
#[tauri::command]
fn place_get(app: State<App>, name: String) -> Result<Option<PlaceInfo>, String> {
    with_conn(&app, &format!("Карточка НП «{name}»"), |conn| {
        conn.query_row(&statement("place_get")?,
                       rusqlite::named_params! { ":name_norm": normalize(&name) },
                       |r| {
                           let (name, comment): (String, String) = (r.get(1)?, r.get(8)?);
                           let clean = genmetric_core::text::place_clean(&name, &comment).to_string();
                           Ok(PlaceInfo {
                               id: r.get(0)?, name, np_type: r.get(2)?, guberniya: r.get(3)?,
                               uyezd: r.get(4)?, volost: r.get(5)?, familio_url: r.get(6)?, origin: r.get(7)?,
                               comment, clean,
                           })
                       })
            .optional()
            .map_err(|e| e.to_string())
    })
}

/// Правка карточки известного пункта (Роман 24.09.2026), включая название
/// (25.09.2026). Всё одной транзакцией: пункт и текстовые копии названия в
/// памяти подсказок.
#[tauri::command]
fn place_update(app: State<App>, id: i64, card: PlaceCard) -> Result<(), String> {
    with_conn_shared(&app, &format!("Правка НП «{}»", card.name), |conn| {
        let clean = card.name.trim().to_string();
        if clean.is_empty() {
            return Err("Название населённого пункта пустое".to_string());
        }
        let comment = card.comment.as_deref().map(str::trim).unwrap_or("").to_string();
        // Название в программе — с комментарием; смена комментария — то же
        // переименование: записи и память подсказок следуют за пунктом.
        let name = genmetric_core::text::place_label(&clean, &comment);
        let norm = normalize(&name);
        let old_name: String = conn
            .query_row("SELECT name FROM place WHERE id = ?1", [id], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Населённого пункта с id {id} нет"))?;
        let renamed = normalize(&old_name) != norm;
        // Совпадение с другим пунктом — понятными словами, а не «UNIQUE
        // constraint failed» (техдолг, ревьюер 24.09.2026). Только при смене
        // названия: правка уезда у пункта, у которого в базе есть тёзка,
        // не должна запрещаться (проверяющий 25.09.2026).
        if renamed {
            if let Some(other) = conn
                .query_row(&statement("place_name_taken")?,
                           rusqlite::named_params! { ":name_norm": norm, ":id": id },
                           |r| r.get::<_, String>(0))
                .optional()
                .map_err(|e| e.to_string())?
            {
                return Err(format!("Пункт «{other}» уже есть в справочнике — выберите его в поле НП, а не переименовывайте этот"));
            }
        }
        let blank = |v: &Option<String>| v.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        card_lookups(conn, &card)?;
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        tx.execute(&statement("place_update")?, rusqlite::named_params! {
            ":id": id, ":name": name, ":name_norm": norm,
            ":np_type": blank(&card.np_type), ":guberniya": blank(&card.guberniya),
            ":uyezd": blank(&card.uyezd), ":volost": blank(&card.volost),
            ":familio_url": blank(&card.familio_url),
            ":clean": clean, ":comment": if comment.is_empty() { None } else { Some(comment.clone()) },
        }).map_err(|e| {
            if e.to_string().contains("UNIQUE") {
                format!("Пункт «{name}» с такими типом, уездом и губернией уже есть в справочнике")
            } else {
                e.to_string()
            }
        })?;
        if old_name != name {
            // Каждому блоку — ровно его параметры: rusqlite на лишний
            // именованный параметр отвечает ошибкой (ревьюер 25.09.2026 —
            // так переименование падало всегда, а Python-тест лишнее прощал).
            // Сначала слить с уже запомненным под новым названием, потом
            // переименовать остальное (техдолг после #36: частоты терялись).
            tx.execute(&statement("place_rename_persons_merge")?, rusqlite::named_params! {
                ":name": name, ":old_name": old_name,
            }).map_err(|e| e.to_string())?;
            tx.execute(&statement("place_rename_persons_drop")?, rusqlite::named_params! {
                ":name": name, ":old_name": old_name,
            }).map_err(|e| e.to_string())?;
            tx.execute(&statement("place_rename_usage_merge")?, rusqlite::named_params! {
                ":name": name, ":old_name": old_name,
            }).map_err(|e| e.to_string())?;
            tx.execute(&statement("place_rename_usage_drop")?, rusqlite::named_params! {
                ":name": name, ":old_name": old_name,
            }).map_err(|e| e.to_string())?;
            tx.execute(&statement("place_rename_persons")?, rusqlite::named_params! {
                ":name": name, ":old_name": old_name,
            }).map_err(|e| e.to_string())?;
            tx.execute(&statement("place_rename_spouses")?, rusqlite::named_params! {
                ":name": name, ":old_name": old_name,
            }).map_err(|e| e.to_string())?;
            tx.execute(&statement("place_rename_usage")?, rusqlite::named_params! {
                ":name": name, ":name_norm": norm, ":old_name": old_name,
            }).map_err(|e| e.to_string())?;
        }
        if renamed {
            // Прежнее название — в память переименований: при обновлении
            // пункт поставки под ним не вернётся (ревьюер 25.09.2026 — без
            // ссылки Familio возвращался).
            tx.execute(&statement("place_renamed_remember")?, rusqlite::named_params! {
                ":old_norm": normalize(&old_name),
            }).map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    })
}

/// Губерния, уезд и волость из карточки — в перечни: следующая карточка их
/// подскажет (Роман 02.10.2026).
fn card_lookups(conn: &Connection, card: &PlaceCard) -> Result<(), String> {
    extend_lookup(conn, "guberniya", card.guberniya.as_deref())?;
    extend_lookup(conn, "uyezd", card.uyezd.as_deref())?;
    extend_lookup(conn, "volost", card.volost.as_deref())
}

// ----------------------------------------------------------------------------
//  Список на сверку после импорта (спека 2026-10-03, п. 5)
// ----------------------------------------------------------------------------

#[derive(Serialize)]
struct ReviewItem {
    id: i64,
    kind: String,
    text: String,
    sheet: Option<String>,
    row: Option<i64>,
    entry_id: Option<i64>,
    done: bool,
    section: Option<i64>,
    year: Option<i64>,
    page: Option<String>,
    no: Option<i64>,
}

#[tauri::command]
fn review_list(app: State<App>, done: bool) -> Result<Vec<ReviewItem>, String> {
    with_conn(&app, "Список на сверку", |conn| {
        let mut stmt = conn.prepare(&statement("review_list")?).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::named_params! { ":done": done as i64 }, |r| Ok(ReviewItem {
                id: r.get(0)?, kind: r.get(1)?, text: r.get(2)?, sheet: r.get(3)?, row: r.get(4)?,
                entry_id: r.get(5)?, done: r.get::<_, i64>(6)? != 0, section: r.get(7)?, year: r.get(8)?,
                page: r.get(9)?, no: r.get(10)?,
            }))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn review_count(app: State<App>) -> Result<i64, String> {
    with_conn(&app, "Список на сверку", |conn| {
        conn.query_row(&statement("review_count")?, [], |r| r.get(0)).map_err(|e| e.to_string())
    })
}

#[tauri::command]
fn review_done(app: State<App>, id: i64, done: bool) -> Result<(), String> {
    with_conn(&app, "Список на сверку", |conn| {
        conn.execute(&statement("review_done")?, rusqlite::named_params! { ":id": id, ":done": done as i64 })
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
}

/// Карточка населённого пункта при первом вводе. Уже известное название
/// не задваивается — возвращается прежний id.
#[tauri::command]
fn place_save(app: State<App>, card: PlaceCard) -> Result<i64, String> {
    with_conn_shared(&app, &format!("Карточка НП «{}»", card.name), |conn| {
        let clean = card.name.trim().to_string();
        if clean.is_empty() {
            return Err("Название населённого пункта пустое".to_string());
        }
        let comment = card.comment.as_deref().map(str::trim).unwrap_or("").to_string();
        let name = genmetric_core::text::place_label(&clean, &comment);
        let norm = normalize(&name);
        if let Some(id) = conn
            .query_row(&statement("place_find")?, rusqlite::named_params! { ":name_norm": norm },
                       |r| r.get::<_, i64>(0))
            .optional()
            .map_err(|e| e.to_string())?
        {
            return Ok(id);
        }
        let blank = |v: &Option<String>| v.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        // Новый пункт: поля карточки — в перечни и в частоты, чтобы следующая
        // карточка предлагала самое частое первым (Роман 06.10.2026).
        conn.execute(&statement("place_save")?, rusqlite::named_params! {
            ":name": name, ":name_norm": norm,
            ":np_type": blank(&card.np_type), ":guberniya": blank(&card.guberniya),
            ":uyezd": blank(&card.uyezd), ":volost": blank(&card.volost),
            ":familio_url": blank(&card.familio_url),
            ":clean": clean, ":comment": if comment.is_empty() { None } else { Some(comment.clone()) },
        }).map_err(|e| e.to_string())?;
        let id = conn.last_insert_rowid();
        // Пункт заведён — тогда и частоты: не раньше, чтобы сбой вставки не
        // оставил частоту у пункта, которого нет.
        for (kind, value) in [("np_type", &card.np_type), ("guberniya", &card.guberniya),
                              ("uyezd", &card.uyezd), ("volost", &card.volost)] {
            genmetric_core::records::remember_card(conn, kind, value.as_deref())?;
        }
        Ok(id)
    })
}



#[derive(Serialize)]
struct MentionOut {
    role_code: String,
    sort_order: i64,
    surname: Option<String>,
    first_name: Option<String>,
    patronymic: Option<String>,
    gender: Option<String>,
    rank: Option<String>,
    confession: Option<String>,
    place: Option<String>,
    note: Option<String>,
    age_years: Option<i64>,
    marriage_order: Option<String>,
    kinship: Option<String>,
    age_text: Option<String>,
    death_cause: Option<String>,
}

#[derive(Serialize)]
struct EntryFull {
    id: i64,
    page: Option<String>,
    no_male: Option<i64>,
    no_female: Option<i64>,
    event_day: Option<i64>,
    event_month: Option<i64>,
    event_year: Option<i64>,
    rite_day: Option<i64>,
    rite_month: Option<i64>,
    rite_year: Option<i64>,
    note: Option<String>,
    persons: Vec<MentionOut>,
}

/// Запись целиком — чтобы поднять её в форму и поправить.
/// Заказчик 22.09.2026 продолжает индексацию в программе; до этого любая
/// ошибка в сохранённой записи стоила перенабора.
#[tauri::command]
fn entry_load(app: State<App>, id: i64) -> Result<EntryFull, String> {
    with_conn(&app, "Чтение записи", |conn| {
        let mut entry = conn
            .query_row(&statement("entry_get")?, rusqlite::named_params! { ":id": id }, |r| {
                Ok(EntryFull {
                    id: r.get(0)?, page: r.get(1)?, no_male: r.get(2)?, no_female: r.get(3)?,
                    event_day: r.get(4)?, event_month: r.get(5)?, event_year: r.get(6)?,
                    rite_day: r.get(7)?, rite_month: r.get(8)?, rite_year: r.get(9)?,
                    note: r.get(10)?, persons: Vec::new(),
                })
            })
            .map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare(&statement("mentions_of_entry")?).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::named_params! { ":entry_id": id }, |r| {
                Ok(MentionOut {
                    role_code: r.get(0)?, sort_order: r.get(1)?, surname: r.get(2)?,
                    first_name: r.get(3)?, patronymic: r.get(4)?, gender: r.get(9)?,
                    rank: r.get(10)?, confession: r.get(11)?, place: r.get(12)?, note: r.get(13)?,
                    age_years: r.get(15)?, marriage_order: r.get(16)?, kinship: r.get(17)?,
                    age_text: r.get(18)?, death_cause: r.get(19)?,
                })
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            entry.persons.push(row.map_err(|e| e.to_string())?);
        }
        Ok(entry)
    })
}

#[derive(Serialize)]
struct ClergyMention {
    role_code: String,
    iof: String,
    rank: Option<String>,
    note: Option<String>,
}

/// Причт последней записи дела — форма продолжает с ним после перезапуска.
#[tauri::command]
fn last_clergy(app: State<App>, section: i64) -> Result<Vec<ClergyMention>, String> {
    with_conn(&app, "Причт последней записи", |conn| {
        let mut stmt = conn.prepare(&statement("last_clergy")?).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::named_params! { ":section": section }, |r| {
                Ok(ClergyMention { role_code: r.get(0)?, iof: r.get(1)?, rank: r.get(2)?, note: r.get(3)? })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    })
}

#[tauri::command]
fn entry_list(app: State<App>, section: i64, year: Option<i64>, last: Option<bool>)
    -> Result<Vec<EntryBrief>, String>
{
    // year — год книги: список «Набрано» показывает его записи; last — одна
    // последняя запись раздела любого года, для «продолжить с места».
    with_conn(&app, "Список записей", |conn| {
        let mut stmt = conn.prepare(&statement("entry_list")?).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::named_params! {
                ":section": section, ":year": year, ":last": last.unwrap_or(false) as i64,
            }, |r| {
                Ok(EntryBrief {
                    id: r.get(0)?, page: r.get(1)?, no_male: r.get(2)?, no_female: r.get(3)?,
                    event_day: r.get(4)?, event_month: r.get(5)?, event_year: r.get(6)?,
                    rite_month: r.get(7)?, child: r.get(8)?, father: r.get(9)?,
                    clergy_noname: r.get::<_, i64>(10)? != 0,
                    groom: r.get(11)?, bride: r.get(12)?, deceased: r.get(13)?,
                    rite_year: r.get(14)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    })
}

/// Растянуть окно по высоте рабочей области монитора и прижать к её верху.
fn fit_height(win: &tauri::WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = win.current_monitor()? else { return Ok(()) };
    let area = monitor.work_area();
    let outer = win.outer_size()?;
    let inner = win.inner_size()?;
    let frame = outer.height.saturating_sub(inner.height);
    // Не больше рабочей области: прежний нижний предел 400 на низком экране
    // вытягивал окно за край (техдолг Д8). Меньше minHeight из конфигурации
    // окно всё равно не станет — это решает система.
    let height = area.size.height.saturating_sub(frame);
    if height == 0 {
        return Ok(());
    }
    win.set_size(tauri::PhysicalSize::new(inner.width, height))?;
    let pos = win.outer_position()?;
    win.set_position(tauri::PhysicalPosition::new(pos.x, area.position.y))?;
    Ok(())
}

// ============================================================================
//  Приходы (спека 2026-10-02): каждый — свой файл, открыт один
// ============================================================================

/// Название прихода — в заголовке окна: при двух приходах человек должен
/// видеть, в каком он сейчас.
fn window_title(parish_name: &str) -> String {
    if parish_name.trim().is_empty() {
        "GenMetric — индексатор метрических книг".to_string()
    } else {
        format!("GenMetric — {parish_name}")
    }
}

fn bundled_seed(app: &State<App>) -> Result<PathBuf, String> {
    app.bundled.clone().map_err(|e| format!("в программе не найдена база поставки: {e}"))
}

#[tauri::command]
fn parish_list(app: State<App>) -> Result<Vec<parish::ParishRow>, String> {
    parish::list(&app.data_dir).map_err(|e| {
        let msg = format!("Перечень приходов: {e}");
        write_log(&app.log_path, &msg);
        msg
    })
}

/// Сменить открытый приход. Прежнее соединение закрывается только после
/// того, как новый приход открылся: при ошибке человек остаётся в прежнем.
/// Окно после этого перечитывает всё заново (перезагрузка на стороне окна).
fn switch_parish(app: &State<App>, window: &tauri::Window, id: i64) -> Result<String, String> {
    let fail = |e: String| {
        let msg = format!("Открытие прихода: {e}");
        write_log(&app.log_path, &msg);
        msg
    };
    let bundled = bundled_seed(app).map_err(fail)?;
    let mut guard = app.conn.lock().map_err(|e| fail(e.to_string()))?;
    // Тот же приход открыт сейчас: два соединения с одним файлом и общим
    // файлом ни к чему — закрываем прежнее заранее.
    let same = app.parish.lock().map(|p| p.0 == id).unwrap_or(false);
    if same {
        *guard = None;
    }
    let opened = match parish::open(&app.data_dir, &bundled, id) {
        Ok(o) => o,
        Err(e) => {
            if same {
                // Вернуть как было, чтобы окно не осталось без базы.
                *guard = parish::open(&app.data_dir, &bundled, id).ok().map(|o| o.conn);
            }
            return Err(fail(e));
        }
    };
    if let Some(w) = &opened.warning {
        write_log(&app.log_path, w);
    }
    *guard = Some(opened.conn);
    if let Ok(mut p) = app.db_path.lock() {
        *p = opened.path.to_string_lossy().to_string();
    }
    if let Ok(mut p) = app.parish.lock() {
        *p = (opened.id, opened.name.clone());
    }
    if let Ok(mut w) = app.warning.lock() {
        *w = opened.warning;
    }
    let _ = window.set_title(&window_title(&opened.name));
    Ok(opened.name)
}

#[tauri::command]
fn parish_open(app: State<App>, window: tauri::Window, id: i64) -> Result<String, String> {
    switch_parish(&app, &window, id)
}

/// Новый приход: пустой файл из поставки — и сразу открыт.
#[tauri::command]
fn parish_create(app: State<App>, window: tauri::Window, name: String) -> Result<String, String> {
    let bundled = bundled_seed(&app)?;
    let id = parish::create(&app.data_dir, &bundled, &name).map_err(|e| {
        let msg = format!("Новый приход «{}»: {e}", name.trim());
        write_log(&app.log_path, &msg);
        msg
    })?;
    switch_parish(&app, &window, id)
}

// ----------------------------------------------------------------------------
//  Импорт из Excel-индексатора (спека 2026-10-02, п. 4)
// ----------------------------------------------------------------------------

/// Очередная часть файла. `first` — начало нового файла: прежний сбрасывается.
#[tauri::command]
fn import_chunk(app: State<App>, bytes: Vec<u8>, first: bool) -> Result<usize, String> {
    let mut file = app.import_file.lock().unwrap_or_else(|e| e.into_inner());
    if first {
        file.clear();
    }
    file.extend_from_slice(&bytes);
    Ok(file.len())
}

#[derive(Serialize)]
struct ImportSeen {
    #[serde(flatten)]
    seen: genmetric_core::import::Inspect,
    size: usize,
    /// Приход, уже импортированный из этого файла (по имени и размеру).
    already_id: Option<i64>,
    already_name: Option<String>,
}

/// Что в присланном файле — до импорта: тот ли это файл и сколько в нём записей.
#[tauri::command]
async fn import_inspect(app: State<'_, App>, file_name: String) -> Result<ImportSeen, String> {
    let fail = |e: String| {
        let msg = format!("Чтение файла «{file_name}»: {e}");
        write_log(&app.log_path, &msg);
        msg
    };
    // Замок, отравленный паникой прошлого разбора, берём всё равно: в нём
    // просто байты файла.
    let file = app.import_file.lock().unwrap_or_else(|e| e.into_inner());
    // Паника разбора битого файла — ошибка в окне, а не закрытая программа
    // (в выпуске до 06.10.2026 стоял panic = "abort").
    let seen = genmetric_core::guarded("разбор файла Excel", || genmetric_core::import::inspect(&file))
        .map_err(fail)?;
    let already = parish::imported_from(&app.data_dir, &file_name, file.len() as i64).map_err(fail)?;
    Ok(ImportSeen {
        seen,
        size: file.len(),
        already_id: already.as_ref().map(|a| a.0),
        already_name: already.map(|a| a.1),
    })
}

#[derive(Serialize)]
struct ImportDone {
    #[serde(flatten)]
    report: genmetric_core::import::ImportReport,
    parish_name: String,
}

/// Импорт: новый приход из присланного файла — и он же открыт. В открытый
/// приход ничего не добавляется. Ошибка — файла прихода не остаётся.
/// async — не в главном потоке: тысячи записей идут секунды.
#[tauri::command]
async fn import_run(app: State<'_, App>, window: tauri::Window, name: String, file_name: String,
                    replace: Option<i64>) -> Result<ImportDone, String> {
    let fail = |e: String| {
        let msg = format!("Импорт «{file_name}»: {e}");
        write_log(&app.log_path, &msg);
        msg
    };
    let bundled = bundled_seed(&app).map_err(fail)?;
    // Файл остаётся в памяти до удачи: занятое название — не повод выбирать
    // его заново (проверяющий 02.10.2026).
    let mut file = app.import_file.lock().unwrap_or_else(|e| e.into_inner());
    let bytes: &[u8] = &file;
    if bytes.is_empty() {
        return Err(fail("файл не прочитан — выберите его ещё раз".into()));
    }
    // Заменяемый приход открыт сейчас — его файл надо отпустить: открытый
    // файл Windows переименовать не даст.
    let current = app.parish.lock().map(|p| p.0).unwrap_or(1);
    let was_open = replace == Some(current);
    if was_open {
        if let Ok(mut guard) = app.conn.lock() {
            *guard = None;
        }
    }
    let made = parish::create_with(&app.data_dir, &bundled, &name, Some((&file_name, bytes.len() as i64)),
                                   replace, |conn| genmetric_core::import::import_into(conn, bytes));
    let (id, report) = match made {
        Ok(v) => v,
        Err(e) => {
            if was_open {
                // Вернуть прежний приход, чтобы окно не осталось без базы.
                if let Ok(mut guard) = app.conn.lock() {
                    *guard = parish::open(&app.data_dir, &bundled, current).ok().map(|o| o.conn);
                }
            }
            return Err(fail(e));
        }
    };
    write_log(&app.log_path, &format!(
        "Импорт «{file_name}»: рождений {}, браков {}, смертей {}; пропущено строк {}; имён не сверено {}",
        report.births, report.marriages, report.deaths, report.skipped.len(), report.unknown_names));
    file.clear();
    file.shrink_to_fit();
    drop(file);
    let parish_name = switch_parish(&app, &window, id)?;
    Ok(ImportDone { report, parish_name })
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // GENMETRIC_DATA_DIR — папка данных для проверок разработчика:
            // настоящая программа на чужих данных, не трогая свои. У
            // пользователя переменной нет — папка обычная.
            let data_dir = match std::env::var_os("GENMETRIC_DATA_DIR") {
                Some(dir) if !dir.is_empty() => PathBuf::from(dir),
                _ => app.path().app_data_dir()?,
            };
            std::fs::create_dir_all(&data_dir)?;
            let log_path = data_dir.join("genmetric-журнал.txt");

            // База справочников поставляется внутри установщика и при первом
            // запуске копируется в папку данных пользователя: там её можно
            // пополнять, не трогая файлы программы. С 02.10.2026 базы —
            // приходы: открывается тот, с которым работали последним
            // (прежняя genmetric.sqlite — первый приход).
            let bundled = app
                .path()
                .resolve("resources/seed.sqlite", BaseDirectory::Resource)
                .map_err(|e| e.to_string());
            let opened = bundled.clone().and_then(|bundled| parish::open_current(&data_dir, &bundled));

            let mut db_path = data_dir.join(parish::FIRST_FILE).to_string_lossy().to_string();
            let mut current = (1, String::new());
            let mut warning = None;
            let (conn, startup_error) = match opened {
                Ok(opened) => {
                    db_path = opened.path.to_string_lossy().to_string();
                    current = (opened.id, opened.name);
                    if let Some(w) = &opened.warning {
                        write_log(&log_path, w);
                    }
                    warning = opened.warning;
                    (Some(opened.conn), None)
                }
                Err(message) => {
                    // Программа всё равно запускается: человек должен увидеть
                    // объяснение, а не отсутствие окна.
                    write_log(&log_path, &format!("База не открылась: {message}"));
                    (None, Some(message))
                }
            };

            let log_for_window = log_path.clone();
            let title = window_title(&current.1);
            app.manage(App {
                conn: Mutex::new(conn),
                db_path: Mutex::new(db_path),
                parish: Mutex::new(current),
                warning: Mutex::new(warning),
                import_file: Mutex::new(Vec::new()),
                data_dir,
                bundled,
                log_path,
                startup_error,
            });

            // Окно создаём здесь, а не из конфигурации («create»: false в
            // tauri.conf.json): только так на Windows можно добавить WebView2
            // порт отладки для сквозной проверки в конвейере. Переменная
            // окружения WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS на раннере
            // 14.09.2026 до WebView2 не дошла — порт не открылся. У Романа
            // переменной GENMETRIC_E2E_DEBUG_PORT нет, окно как прежде.
            let window = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .cloned()
                .ok_or("в конфигурации нет окна main")?;
            #[allow(unused_mut)]
            let mut builder = tauri::WebviewWindowBuilder::from_config(app.handle(), &window)?;
            #[cfg(windows)]
            if let Ok(port) = std::env::var("GENMETRIC_E2E_DEBUG_PORT") {
                // Первая часть — то, что wry передаёт по умолчанию; при своих
                // аргументах её нужно повторить (см. документацию метода).
                builder = builder.additional_browser_args(&format!(
                    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
                     --remote-debugging-port={port}"
                ));
            }
            let win = builder.build()?;
            let _ = win.set_title(&title);
            // Высота окна — по рабочей области экрана (без панели задач):
            // Роман 24.09.2026, «программа при открытии становилась по высоте
            // экрана пользователя». Ширина прежняя. Любая ошибка здесь не
            // мешает запуску.
            // Высота по экрану — удобство, не условие запуска: ошибка в журнал,
            // окно остаётся прежнего размера (ревьюер 24.09.2026).
            if let Err(e) = fit_height(&win) {
                write_log(&log_for_window, &format!("Высота окна по экрану не выставлена: {e}"));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            startup_state,
            search_persons,
            person_dossier,
            read_log,
            db_info,
            lookup_summary,
            case_load,
            case_years,
            case_save,
            case_delete,
            review_list,
            review_count,
            review_done,
            entry_save,
            entry_list,
            entry_load,
            last_clergy,
            suggest_person,
            birth_father,
            suggest_infant,
            suggest_spouse,
            list_clergy,
            import_archive,
            get_setting,
            set_setting,
            suggest,
            rank_default,
            parse_iof,
            similar_names,
            dict_search,
            alias_save,
            place_check,
            place_save,
            place_get,
            place_update,
            set_always_on_top,
            parish_list,
            parish_open,
            parish_create,
            import_chunk,
            import_inspect,
            import_run,
            export::export_years,
            export::export_familio,
            export::export_excel,
            export::reveal_path,
            export::open_familio
        ])
        .run(tauri::generate_context!())
        .expect("не удалось запустить GenMetric");
}
