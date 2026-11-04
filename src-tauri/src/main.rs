// GenMetric — индексатор метрических книг
// Лицензия GPL-3.0-or-later, см. файл LICENSE в корне репозитория.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::path::BaseDirectory;
use tauri::{Manager, State};

mod export;

mod scanserve;

mod windows;

use genmetric_core::parish;
use genmetric_core::records::{extend_lookup, parse_iof_with, save_case, save_entry, Case, CaseSaved, EntryInput, ParsedIof, Saved};
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
    /// Файлы Familio, выбранные для импорта в один приход (имя, байты).
    familio_files: Mutex<Vec<(String, Vec<u8>)>>,
    /// Упоминания открытого прихода для экрана «Поиск» и счётчик изменений
    /// соединения, при котором они прочитаны (search.rs, `Index`).
    search: Mutex<Option<(i64, Arc<genmetric_core::search::Index>)>>,
    /// Годы смерти для «†» в подсказке — отдельно от памяти поиска: она
    /// держится только пока открыт экран «Поиск» (долг п. 68).
    died: Mutex<Option<(i64, Arc<genmetric_core::search::Deaths>)>>,
    /// Память поиска перестраивается в фоне для подсказки (`refresh_search_soon`).
    search_busy: std::sync::atomic::AtomicBool,
    /// Экран «Поиск» открыт: только тогда память поиска хранится (долг п. 84 —
    /// запрос, пришедший после ухода с экрана, клал её обратно).
    search_live: std::sync::atomic::AtomicBool,
    /// Ширина окна до включения блока скана — к ней окно возвращается, когда
    /// блок скрывают: ширина и положение левого края (в физических точках).
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

/// Замок соединения. Отравленный паникой прошлой команды берётся всё равно:
/// соединение цело, а отказ означал бы, что до перезапуска не работает ничего
/// — ни набор, ни сохранение (до 10.10.2026 так и было, а импорт и смена
/// прихода такой замок молча пропускали).
fn lock_conn<'a>(app: &'a App) -> MutexGuard<'a, Option<Connection>> {
    app.conn.lock().unwrap_or_else(|poisoned| {
        write_log(&app.log_path, "Замок базы остался от команды, упавшей с внутренней ошибкой, — работа продолжается");
        app.conn.clear_poison();
        poisoned.into_inner()
    })
}

/// Память экрана «Поиск» больше не годится: открыт другой приход или файл.
fn drop_search(app: &App) {
    *app.search.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *app.died.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Экран «Поиск» закрыт — его память больше не нужна (долг п. 68): подсказке
/// хватает годов смерти (`App::died`).
#[tauri::command]
fn search_release(app: State<App>) {
    app.search_live.store(false, std::sync::atomic::Ordering::SeqCst);
    *app.search.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Экран «Поиск» открыт — память поиска можно хранить.
#[tauri::command]
fn search_open(app: State<App>) {
    app.search_live.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Оборачивает работу с базой: логирует неудачу и возвращает её наверх.
///
/// Работа идёт под перехватом паники: обычная команда окна исполняется в
/// главном потоке, и паника в ней закрывала программу вместе с набранным в
/// форме. Теперь это ошибка в окне и строка в журнале; начатую и не
/// законченную транзакцию откатываем сами.
fn with_conn<T>(
    app: &State<App>,
    what: &str,
    body: impl FnOnce(&Connection) -> Result<T, String>,
) -> Result<T, String> {
    let guard = lock_conn(app);
    let conn = match guard.as_ref() {
        Some(c) => c,
        None => {
            let msg = format!("{what}: база не открыта");
            write_log(&app.log_path, &msg);
            return Err(msg);
        }
    };
    let done = genmetric_core::guarded_db(conn, body);
    done.map_err(|e| {
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
        drop(stmt);
        // Звания — ещё и по вхождению (Роман 12.10.2026: набрал «от» и хочет
        // увидеть «сын-отрок», а не только «отставной рядовой»). Сначала всё,
        // что начинается с набранного, ниже — где набранное в середине. С
        // двух букв: одна буква в середине есть почти в каждом звании.
        if let Some(inside) = genmetric_core::rank_inside_pattern(&kind, &pattern) {
            if (out.len() as i64) < limit {
                let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
                let mut params: Vec<(&str, &dyn rusqlite::ToSql)> =
                    vec![(":kind", &kind), (":prefix", &inside), (":limit", &limit)];
                if gendered {
                    params.push((":gender", &gender));
                }
                let more = stmt
                    .query_map(&params[..], |r| {
                        Ok(Suggestion { value: r.get(0)?, tier: r.get(1)?, count: r.get(2)? })
                    })
                    .map_err(|e| e.to_string())?;
                for row in more {
                    let row = row.map_err(|e| e.to_string())?;
                    if (out.len() as i64) < limit && !out.iter().any(|have: &Suggestion| have.value == row.value) {
                        out.push(row);
                    }
                }
            }
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
fn parse_iof(app: State<App>, text: String, name_surname: Option<bool>) -> Result<ParsedIof, String> {
    with_conn(&app, &format!("Разбор «{text}»"), |conn| parse_iof_with(conn, &text, name_surname.unwrap_or(false)))
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
    /// Сохранён как имя и фамилия — выбор ставит отметку «ИФ» сам (Роман 15.10.2026).
    name_surname: bool,
    /// Год смерти: персона умерла, и событий после смерти нет (спека
    /// 2026-10-15-semya-celikom, п. 1.6). Такие — в конце списка с «†».
    died: Option<i64>,
}

#[derive(Serialize)]
struct SpouseHint {
    iof: String,
    place: Option<String>,
    rank: Option<String>,
    name_surname: bool,
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
    scan_file: Option<String>,
    rite_day: Option<i64>,
}

/// Дело года (year) или текущее — дело последней записи. Дело — на год
/// книги (спека 2026-10-02, п. 3); экран «Дело» остаётся одной формой. id —
/// дело, выбранное в списке: у года после импорта их бывает два (п. 94).
#[tauri::command]
fn case_load(app: State<App>, year: Option<i64>, id: Option<i64>) -> Result<Option<Case>, String> {
    with_conn(&app, "Чтение дела", |conn| {
        let row = |r: &rusqlite::Row| Ok(Case {
            id: r.get(0)?, archive: r.get(1)?, fond: r.get(2)?, opis: r.get(3)?,
            delo: r.get(4)?, church: r.get(5)?, village: r.get(6)?, uyezd: r.get(7)?,
            guberniya: r.get(8)?, year: r.get(9)?, indexer: r.get(10)?,
        });
        match id {
            Some(id) => conn.query_row(&statement("case_by_id")?, rusqlite::named_params! { ":id": id }, row),
            None => conn.query_row(&statement("case_current")?, rusqlite::named_params! { ":year": year }, row),
        }
        .optional()
        .map_err(|e| e.to_string())
    })
}

#[derive(Serialize)]
struct OpenedYear {
    /// Дело года; None — год не трогаем (дальний год без записей — похоже на
    /// опечатку: форма просто стоит на нём, как раньше).
    case: Option<Case>,
    /// Год без записей: формы начинают его с чистого листа.
    fresh: bool,
    /// Дело завела эта команда (а не человек на экране «Дело»): только такое
    /// пустое дело можно убрать при возврате (ревьюер 19.10.2026).
    created: bool,
    /// Год без записей не заведён: в прежнем году нет записей — похоже на
    /// опечатку; окно говорит об этом полосой (Роман 20.10.2026).
    typo: bool,
    /// Записей этого года по разделам (рождения, браки, смерти): форма, в
    /// разделе которой их нет, начинает год с чистого листа (спека
    /// 2026-10-20-dosye-i-dovodka, п. 2).
    sections: [i64; 3],
}

/// Год на форме сменили (спека 2026-10-19-oshibki-svyazej-i-dovodka, Ж2.7;
/// Роман 19.10.2026: «программа должна автоматически переключить на этот
/// год и две другие вкладки»). Новый год — следующий за любым годом с
/// записями (не только за самым поздним в приходе: Роман набирает и
/// приходы с годами вразброс), или у которого дело уже заведено, или первый
/// в приходе: дело заводится
/// (копией реквизитов), `fresh`. Год с записями — его дело, без чистого
/// листа (возврат после опечатки). Дальний год без записей (1889 → 1898) —
/// None: опечатка не должна заводить дело и стирать формы (ревьюер 19.10.2026).
#[tauri::command]
fn case_open_year(app: State<App>, year: i64) -> Result<OpenedYear, String> {
    with_conn(&app, &format!("Дело {year} года"), |conn| {
        let count = |sql: &str| -> Result<i64, String> { conn.query_row(sql, [year], |r| r.get(0)).map_err(|e| e.to_string()) };
        let entries = count("SELECT count(*) FROM entry e JOIN mk_case c ON c.id = e.case_id WHERE c.year = ?1")?;
        let has_case = count("SELECT count(*) FROM mk_case WHERE year = ?1")? > 0;
        let last: Option<i64> = conn
            .query_row("SELECT max(c.year) FROM entry e JOIN mk_case c ON c.id = e.case_id", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let fresh = entries == 0;
        let mut sections = [0i64; 3];
        for (k, n) in sections.iter_mut().enumerate() {
            *n = conn.query_row("SELECT count(*) FROM entry e JOIN mk_case c ON c.id = e.case_id WHERE c.year = ?1 AND e.section = ?2",
                                rusqlite::params![year, k as i64 + 1], |r| r.get(0)).map_err(|e| e.to_string())?;
        }
        // В приходе ещё нет ни одной записи — переходить не с чего: формы и так
        // с чистого листа, а набранную до года страницу стирать нельзя (e2e
        // 19.10.2026: страница, набранная до года, пропадала). Год получит
        // дело при первой записи, как раньше.
        if last.is_none() {
            return Ok(OpenedYear { case: None, fresh, created: false, typo: false, sections });
        }
        let before = count("SELECT count(*) FROM entry e JOIN mk_case c ON c.id = e.case_id WHERE c.year = ?1 - 1")?;
        if fresh && !has_case && before == 0 {
            return Ok(OpenedYear { case: None, fresh, created: false, typo: true, sections });
        }
        let created = fresh && genmetric_core::records::case_for_year(conn, year)?.1;
        let case = conn.query_row(&statement("case_current")?, rusqlite::named_params! { ":year": year },
            |r| Ok(Case {
                id: r.get(0)?, archive: r.get(1)?, fond: r.get(2)?, opis: r.get(3)?,
                delo: r.get(4)?, church: r.get(5)?, village: r.get(6)?, uyezd: r.get(7)?,
                guberniya: r.get(8)?, year: r.get(9)?, indexer: r.get(10)?,
            }))
            .optional()
            .map_err(|e| e.to_string())?;
        Ok(OpenedYear { case, fresh, created, typo: false, sections })
    })
}

#[derive(Serialize)]
struct CaseYear {
    year: i64,
    entries: i64,
    /// Дело, а не год: у года после импорта бывает два дела (п. 94).
    id: i64,
    delo: Option<String>,
}

/// Годы, у которых есть дело, — для выбора года на экране «Дело».
#[tauri::command]
fn case_years(app: State<App>) -> Result<Vec<CaseYear>, String> {
    with_conn(&app, "Годы дел", |conn| {
        let mut stmt = conn.prepare(&statement("case_years")?).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| Ok(CaseYear { year: r.get(0)?, entries: r.get(1)?, id: r.get(2)?, delo: r.get(3)? }))
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
fn suggest_person(handle: tauri::AppHandle, app: State<App>, prefix: String, limit: Option<i64>, gender: Option<String>,
                  prefer_infant: Option<bool>, keep_wife_rank: Option<bool>, living: Option<bool>)
    -> Result<Vec<PersonHint>, String>
{
    // Умерший: младенцы из записей о рождении — первыми (Роман 28.09.2026).
    // Имя блока — литералом в каждой ветке: так его видит проверка
    // параметров в test_incidents.py.
    // Маска «*** Иванов Кулаев» — свой запрос (Роман 15.10.2026).
    let mask = genmetric_core::records::person_mask(&prefix);
    let sql = if mask.is_some() { statement("person_suggest_mask")? }
              else if prefer_infant.unwrap_or(false) { statement("person_suggest_infant")? }
              else { statement("person_suggest")? };
    let pattern = like_prefix(&prefix);
    let limit = limit.unwrap_or(8).clamp(1, 50);
    // С запасом: одинаковые на вид строки ниже сворачиваются.
    let fetch = (limit * 2).min(100);
    let found = with_conn(&app, &format!("Поиск персоны «{prefix}»"), |conn| {
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let hint = |r: &rusqlite::Row| -> rusqlite::Result<PersonHint> {
            Ok(PersonHint {
                iof: r.get(0)?, place: r.get(1)?, rank: r.get(2)?,
                gender: r.get(3)?, uses: r.get(4)?, name_surname: r.get(5)?, died: None,
            })
        };
        let rows = match &mask {
            Some((name, rest)) => stmt.query_map(rusqlite::named_params! {
                ":name": name, ":rest": rest, ":limit": fetch, ":gender": gender,
            }, hint),
            None => stmt.query_map(rusqlite::named_params! {
                ":prefix": pattern, ":limit": fetch, ":gender": gender,
            }, hint),
        }
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
        // Один человек — одна строка: книжные написания одного отчества
        // («Александрова» и «Александровна») у того же пункта сворачиваются,
        // остаётся самая частая строка, частоты складываются (Роман
        // 20.10.2026: «две абсолютно одинаковые персоны»).
        let mut seen: Vec<(String, Option<String>)> = Vec::new();
        let mut kept: Vec<PersonHint> = Vec::new();
        for hint in out {
            let key = (genmetric_core::records::person_key(conn, &hint.iof, hint.name_surname)?, hint.place.clone());
            match seen.iter().position(|k| *k == key) {
                Some(n) => kept[n].uses += hint.uses,
                None => { seen.push(key); kept.push(hint); }
            }
        }
        kept.truncate(limit as usize);
        Ok((kept, genmetric_core::search::changes(conn)?))
    })?;
    let (mut out, now) = found;
    // Умершие — по памяти поиска (связи «семьи целиком»). Её перестройка
    // занимает доли секунды, а подсказка идёт на каждую букву: устаревшую
    // память берём как есть (в ней нет только последних сохранений), а
    // свежую строим в фоне.
    let cached = app.died.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if cached.as_ref().map_or(true, |(at, _)| *at != now) {
        refresh_search_soon(&handle, now);
    }
    if let Some((_, died)) = cached {
        for hint in out.iter_mut() {
            hint.died = died.of(&hint.iof, hint.place.as_deref());
        }
        // Роман 16.10.2026: «оставлять, но показывать в самом конце
        // выпадающего списка с пометкой „† год смерти“». В поле «Умерший» —
        // как раньше.
        if living.unwrap_or(false) {
            out.sort_by_key(|h| h.died.is_some());
        }
    }
    Ok(out)
}

/// Перестроить память поиска в фоне, если её уже не строят. Читает своё
/// соединение «только чтение»: база в режиме WAL, и чтение не держит ни замок
/// соединения окна, ни запись — набор не ждёт (ревьюер 17.10.2026: на
/// приходе в 100 тысяч упоминаний это около секунды на каждое сохранение).
/// Память помечается счётчиком изменений `now`, при котором её начали
/// строить: сохранили что-то за это время — следующая подсказка построит
/// заново. Приход сменился — построенное выбрасывается.
fn refresh_search_soon(handle: &tauri::AppHandle, now: i64) {
    use std::sync::atomic::Ordering;
    let app = handle.state::<App>();
    if app.search_busy.swap(true, Ordering::SeqCst) {
        return;
    }
    let path = app.db_path.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let handle = handle.clone();
    std::thread::spawn(move || {
        let app = handle.state::<App>();
        let built = std::panic::catch_unwind(|| -> Result<genmetric_core::search::Index, String> {
            let conn = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX)
                .map_err(|e| e.to_string())?;
            conn.busy_timeout(std::time::Duration::from_secs(3)).map_err(|e| e.to_string())?;
            genmetric_core::search::Index::load(&conn)
        });
        match built {
            Ok(Ok(index)) => {
                // Замки — по одному, не вложенно: смена прихода берёт их в
                // своём порядке.
                let same = *app.db_path.lock().unwrap_or_else(|e| e.into_inner()) == path;
                // Хранятся только годы смерти; сама память поиска —
                // выбрасывается (долг п. 68).
                if same {
                    *app.died.lock().unwrap_or_else(|e| e.into_inner()) = Some((now, Arc::new(index.deaths())));
                }
            }
            Ok(Err(e)) => write_log(&app.log_path, &format!("Память поиска для подсказки (умершие с «†»): {e}")),
            Err(_) => write_log(&app.log_path, "Память поиска для подсказки: внутренняя ошибка"),
        }
        app.search_busy.store(false, Ordering::SeqCst);
    });
}

/// Полное ИОФ ребёнка под полем «Ребёнок» и «Умерший»: имя + отчество по
/// имени отца + фамилия отца (спека 2026-10-15-semya-celikom, п. 1.5). Только
/// показ — в запись идёт набранное (Роман 16.10.2026, ответ 2а).
#[tauri::command]
fn child_full_iof(app: State<App>, child: String, gender: Option<String>, father: String, father_name_surname: Option<bool>)
    -> Result<Option<String>, String>
{
    with_conn(&app, &format!("Полное ИОФ «{child}»"), |conn| {
        genmetric_core::search::child_full_iof(conn, &child, gender.as_deref(), &father, father_name_surname.unwrap_or(false))
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
    name_surname: bool,
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
        }, |r| Ok(FatherHint { iof: r.get(0)?, place: r.get(1)?, rank: r.get(2)?, births: r.get(3)?, name_surname: r.get(4)? }))
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
        // Под своим перехватом: паника посреди слияния не должна оставить
        // архив подключённым — отключение ниже идёт в любом случае.
        let done = genmetric_core::guarded_db(conn, |conn| -> Result<ImportReport, String> {
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
        });
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
    name_surname: bool,
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
                Ok(ClergyHint { iof: r.get(0)?, rank: r.get(1)?, uses: r.get(2)?, name_surname: r.get(3)? })
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
        let lookup = |husband_norm: &str| conn.query_row(
            &statement("spouse_lookup")?,
            rusqlite::named_params! { ":husband_norm": husband_norm, ":place": place },
            |r| Ok(SpouseHint { iof: r.get(0)?, place: r.get(1)?, rank: r.get(2)?, name_surname: false }),
        )
        .optional()
        .map_err(|e| e.to_string());
        // Точного мужа нет — тот же муж в другом написании («Давыдов» и
        // «Давидов», Роман 26.10.2026).
        let wife = match lookup(&normalize(&husband))? {
            Some(w) => Some(w),
            None => match genmetric_core::records::spouse_husband_alias(conn, &husband)? {
                Some(alias) => lookup(&alias)?,
                None => None,
            },
        };
        let Some(mut wife) = wife else { return Ok(None) };
        wife.name_surname = conn.query_row(&statement("person_name_surname")?,
                                           rusqlite::named_params! { ":iof": wife.iof, ":place": wife.place },
                                           |r| r.get(0))
            .optional().map_err(|e| e.to_string())?.unwrap_or(false);
        Ok(Some(wife))
    })
}

/// Упоминания открытого прихода для поиска: из памяти, пока база не менялась
/// (счётчик изменений соединения тот же), иначе — прочитать заново. Замок
/// базы держим только на время чтения; сам поиск идёт уже без него.
fn search_index(app: &State<App>, what: &str) -> Result<Arc<genmetric_core::search::Index>, String> {
    with_conn(app, what, |conn| {
        let now = genmetric_core::search::changes(conn)?;
        let mut cache = app.search.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, index)) = cache.as_ref() {
            if *at == now {
                return Ok(index.clone());
            }
        }
        let index = Arc::new(genmetric_core::search::Index::load(conn)?);
        // Экран уже закрыт (запрос пришёл после ухода) — ответить, но не хранить.
        if app.search_live.load(std::sync::atomic::Ordering::SeqCst) {
            *cache = Some((now, index.clone()));
        }
        Ok(index)
    })
}

/// Экран «Поиск»: найденные персоны — один ИОФ в одном НП (search.rs).
/// async — не в главном потоке: поиск идёт на каждую набранную букву, и на
/// большом приходе окно не должно замирать.
#[tauri::command]
async fn search_persons(app: State<'_, App>, filter: genmetric_core::search::Filter)
    -> Result<genmetric_core::search::Found, String>
{
    let index = search_index(&app, &format!("Поиск персоны «{}»", filter.query))?;
    Ok(genmetric_core::search::find_in(&index, &filter))
}

/// Досье выбранной персоны: все записи, где стоит её ИОФ в этом НП.
#[tauri::command]
async fn person_dossier(app: State<'_, App>, key: String, place: String, filter: genmetric_core::search::Filter)
    -> Result<genmetric_core::search::Dossier, String>
{
    let index = search_index(&app, &format!("Досье «{key}»"))?;
    Ok(genmetric_core::search::dossier_in(&index, &key, &place, &filter))
}

#[derive(Serialize, Default)]
struct PlaceCheck {
    known: bool,
    /// Написание известного пункта в справочнике — поле НП приводит к нему
    /// набранное («заборье (нежитино)» → «Заборье (Нежитино)»).
    canonical: Option<String>,
    similar: Vec<Similar>,
    /// Волость последнего пункта, заведённого человеком, — в карточку нового.
    last_volost: String,
    /// Тип, губерния и уезд того же пункта (Роман 20.10.2026).
    last_type: String,
    last_guberniya: String,
    last_uyezd: String,
}

/// Известен ли населённый пункт, и на что он похож, если нет. Форма
/// спрашивает при уходе из поля НП: неизвестный — карточка, похожий —
/// предложение выбрать («Букарина» → «Бухарино», Роман 13.09.2026).
#[tauri::command]
fn place_check(app: State<App>, name: String) -> Result<PlaceCheck, String> {
    with_conn(&app, &format!("Проверка НП «{name}»"), |conn| {
        let norm = normalize(&name);
        if norm.is_empty() {
            return Ok(PlaceCheck { known: true, canonical: None, similar: vec![], ..PlaceCheck::default() });
        }
        let known: Option<String> = conn
            .query_row(&statement("place_find")?, rusqlite::named_params! { ":name_norm": norm },
                       |r| r.get::<_, String>(1))
            .optional()
            .map_err(|e| e.to_string())?;
        if known.is_some() {
            return Ok(PlaceCheck { known: true, canonical: known, similar: vec![], ..PlaceCheck::default() });
        }
        let (last_volost, last_type, last_guberniya, last_uyezd): (String, String, String, String) = conn
            .query_row(&statement("place_last_volost")?, [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
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
        // Набрали название пункта, объединённого с другим, — первым идёт
        // оставшийся: одним Enter запись встаёт к нему (03.11.2026).
        if let Some(target) = conn
            .query_row(&statement("place_merged_target")?, rusqlite::named_params! { ":name_norm": norm },
                       |r| r.get::<_, String>(0))
            .optional()
            .map_err(|e| e.to_string())?
        {
            similar.push(Similar { value: target, gender: None, distance: 0 });
        }
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
        Ok(PlaceCheck { known: false, canonical: None, similar, last_volost, last_type, last_guberniya, last_uyezd })
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
            ":clean": genmetric_core::text::strip_place_type(&clean), ":comment": if comment.is_empty() { None } else { Some(comment.clone()) },
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
            // Объединённые в этот пункт идут за ним под новым названием, а
            // новое название — живое, даже если его когда-то объединяли в
            // другой (ревьюер 03.11.2026: иначе цепочка «X → Y» обрывалась
            // в приходе, где Y нет).
            tx.execute(&statement("place_merged_chain")?, rusqlite::named_params! {
                ":old_norm": normalize(&old_name), ":into_norm": norm,
            }).map_err(|e| e.to_string())?;
            tx.execute(&statement("place_merged_alive")?, rusqlite::named_params! { ":into_norm": norm })
                .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    })
}

/// Сколько записей у двух пунктов — окно объединения показывает, у какого
/// их больше (по умолчанию остаётся он).
#[tauri::command]
fn place_merge_uses(app: State<App>, a: String, b: String) -> Result<(i64, i64), String> {
    with_conn(&app, "Объединение пунктов: число записей", |conn| {
        Ok((genmetric_core::place_merge::uses(conn, &normalize(&a))?,
            genmetric_core::place_merge::uses(conn, &normalize(&b))?))
    })
}

/// Сколько строк справочника с этим названием (п. 121, 04.11.2026):
/// «Лукино» двух уездов — две строки, объединение уберёт обе.
#[tauri::command]
fn place_rows(app: State<App>, name: String) -> Result<i64, String> {
    with_conn(&app, "Объединение пунктов: строки названия", |conn| {
        conn.query_row("SELECT count(*) FROM place WHERE name_norm = ?1", [normalize(&name)], |r| r.get(0))
            .map_err(|e| e.to_string())
    })
}

/// Пункты, похожие на этот, — кандидаты в объединение: «Глинница» к
/// «Глинице», «Шишкино» к «с.Шишкино» (приставка типа не считается).
#[tauri::command]
fn place_merge_candidates(app: State<App>, name: String) -> Result<Vec<Similar>, String> {
    with_conn(&app, &format!("Похожие на пункт «{name}»"), |conn| {
        let key = |n: &str| normalize(genmetric_core::text::strip_place_type(n));
        let (own, q) = (normalize(&name), key(&name));
        let mut stmt = conn.prepare(&statement("place_names")?).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?;
        let mut items = Vec::new();
        for row in rows {
            let (value, norm) = row.map_err(|e| e.to_string())?;
            if norm != own && !items.iter().any(|(_, v, _): &(String, String, Option<String>)| *v == value) {
                items.push((key(&value), value, None));
            }
        }
        let max_dist = (q.chars().count() / 4).max(2);
        Ok(rank_similar(&q, items, 8, max_dist))
    })
}

/// Объединение пунктов (спека 2026-10-20-obedinenie-punktov.md): остаётся
/// `keep_id`, `drop_id` уходит. Перед этим — копии файла прихода и общего
/// файла рядом с ними (метка «-до-объединения-пунктов-»).
#[tauri::command]
fn place_merge(app: State<App>, keep_id: i64, drop_id: i64) -> Result<genmetric_core::place_merge::Merged, String> {
    let db_path = app.db_path.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let dir = app.data_dir.clone();
    with_conn_shared(&app, "Объединение пунктов", |conn| {
        genmetric_core::db::backup_marked(conn, std::path::Path::new(&db_path), genmetric_core::db::MERGE_MARK)
            .map_err(|e| format!("копия прихода не сделана, объединение отменено: {e}"))?;
        if parish::has_common(conn) {
            let common = parish::common_path(&dir);
            let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs()).unwrap_or(0);
            let stem = common.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
            let mut target = common.with_file_name(format!("{stem}{}{seconds}.sqlite", genmetric_core::db::MERGE_MARK));
            for n in 2.. {
                if !target.exists() {
                    break;
                }
                target = common.with_file_name(format!("{stem}{}{seconds}-{n}.sqlite", genmetric_core::db::MERGE_MARK));
            }
            std::fs::copy(&common, &target)
                .map_err(|e| format!("копия общего файла не сделана, объединение отменено: {e}"))?;
            // Копий общего файла — тоже не больше пяти (п. 123).
            if let Err(e) = genmetric_core::db::prune_copies(&common, genmetric_core::db::MERGE_MARK, genmetric_core::db::KEEP_COPIES) {
                write_log(&app.log_path, &format!("Старые копии общего файла не убраны: {e}"));
            }
        }
        genmetric_core::place_merge::merge(conn, keep_id, drop_id)
    })
}

/// Файлы других приходов — для очистки справочников: значение общего
/// перечня, занятое там, не предлагается. None — другие приходы неизвестны
/// (нет общего файла, файл прихода пропал): перечни тогда не предлагаются
/// вовсе (ревьюер 04.11.2026).
fn other_parish_files(conn: &Connection, dir: &std::path::Path, own: &str) -> Option<Vec<std::path::PathBuf>> {
    if !parish::has_common(conn) {
        return None;
    }
    let own = std::fs::canonicalize(own).unwrap_or_else(|_| std::path::PathBuf::from(own));
    let files: Vec<String> = conn.prepare("SELECT file FROM common.parish")
        .and_then(|mut st| st.query_map([], |r| r.get::<_, String>(0))?.collect())
        .ok()?;
    let mut out = Vec::new();
    for f in files {
        let p = dir.join(f);
        let same = std::fs::canonicalize(&p).map(|c| c == own).unwrap_or(false);
        if same {
            continue;
        }
        if !p.exists() {
            return None;
        }
        out.push(p);
    }
    Some(out)
}

/// Неиспользуемые значения справочников открытого прихода (З3.4, спека
/// 2026-11-04-ochistka-spravochnikov-i-dolg.md): только список, ничего не
/// удаляется.
#[tauri::command(async)]
fn cleanup_find(app: State<App>) -> Result<Vec<genmetric_core::cleanup::Unused>, String> {
    let db_path = app.db_path.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let dir = app.data_dir.clone();
    with_conn(&app, "Поиск неиспользуемых значений", |conn| {
        genmetric_core::cleanup::find(conn, other_parish_files(conn, &dir, &db_path).as_deref())
    })
}

#[derive(Serialize)]
struct Cleaned {
    removed: usize,
    copy: String,
}

/// Убрать отмеченное: перед этим — копия файла прихода рядом с ним. Перечни
/// уходят и из общего файла (сверка после удаления).
#[tauri::command(async)]
fn cleanup_remove(app: State<App>, keys: Vec<String>) -> Result<Cleaned, String> {
    let db_path = app.db_path.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let dir = app.data_dir.clone();
    with_conn_shared(&app, "Очистка справочников", |conn| {
        let copy = genmetric_core::db::backup_marked(conn, std::path::Path::new(&db_path), genmetric_core::db::CLEANUP_MARK)
            .map_err(|e| format!("копия прихода не сделана, очистка отменена: {e}"))?;
        let (removed, problems) = genmetric_core::cleanup::remove(conn, &keys, other_parish_files(conn, &dir, &db_path).as_deref())?;
        for p in &problems {
            write_log(&app.log_path, &format!("Очистка справочников: перечень другого прихода не поправлен — {p}"));
        }
        Ok(Cleaned { removed, copy: copy.to_string_lossy().to_string() })
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
            ":clean": genmetric_core::text::strip_place_type(&clean), ":comment": if comment.is_empty() { None } else { Some(comment.clone()) },
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
    /// Месяцы к годам жениха и невесты («17,25» из Familio — 17 лет 3 мес):
    /// форма собирает из них книжное написание, иначе правка их теряла
    /// (проверяющий 02.11.2026).
    age_months: Option<i64>,
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
    scan_file: Option<String>,
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
                    note: r.get(10)?, scan_file: r.get(11)?, persons: Vec::new(),
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
                    age_text: r.get(18)?, death_cause: r.get(19)?, age_months: r.get(20)?,
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
    name_surname: bool,
}

/// Причт последней записи дела — форма продолжает с ним после перезапуска.
#[tauri::command]
fn last_clergy(app: State<App>, section: i64) -> Result<Vec<ClergyMention>, String> {
    with_conn(&app, "Причт последней записи", |conn| {
        let mut stmt = conn.prepare(&statement("last_clergy")?).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::named_params! { ":section": section }, |r| {
                Ok(ClergyMention { role_code: r.get(0)?, iof: r.get(1)?, rank: r.get(2)?, note: r.get(3)?, name_surname: r.get(4)? })
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
                Ok((EntryBrief {
                    id: r.get(0)?, page: r.get(1)?, no_male: r.get(2)?, no_female: r.get(3)?,
                    event_day: r.get(4)?, event_month: r.get(5)?, event_year: r.get(6)?,
                    rite_month: r.get(7)?, child: r.get(8)?, father: r.get(9)?,
                    clergy_noname: r.get::<_, i64>(10)? != 0,
                    groom: r.get(11)?, bride: r.get(12)?, deceased: r.get(13)?,
                    rite_year: r.get(14)?, scan_file: r.get(15)?, rite_day: r.get(16)?,
                }, r.get::<_, Option<String>>(17)?, r.get::<_, Option<String>>(18)?))
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        // Одна семья — много младенцев: полное ИОФ считается один раз на
        // имя, пол и отца (ревьюер 20.10.2026: словарь читался на строку).
        let mut known: std::collections::HashMap<(String, Option<String>, String), Option<String>> = std::collections::HashMap::new();
        // Словарь отчеств — один раз на список, по первому младенцу. Не
        // прочитался — второй раз не пробуем: каждый младенец тогда читает своё
        // имя сам, как до 21.10 (долг п. 83).
        let mut dict: Option<genmetric_core::search::Patronymics> = None;
        let mut dict_tried = false;
        for row in rows {
            let (mut brief, gender, father) = row.map_err(|e| e.to_string())?;
            // Младенец одним именем — полным ИОФ по отцу, тем же правилом, что
            // строка разбора под полем (только показ; в базе — как в книге).
            if let (Some(dead), Some(father)) = (brief.deceased.as_deref(), father.as_deref()) {
                if !dead.trim().is_empty() && !dead.trim().contains(' ') {
                    let key = (dead.to_string(), gender.clone(), father.to_string());
                    let full = match known.get(&key) {
                        Some(v) => v.clone(),
                        None => {
                            if !dict_tried {
                                dict_tried = true;
                                dict = genmetric_core::search::patronymics(conn, None).ok();
                            }
                            let v = genmetric_core::search::child_full_iof_in(conn, dict.as_ref(), dead, gender.as_deref(), father, false)
                                .ok().flatten();
                            known.insert(key, v.clone());
                            v
                        }
                    };
                    if let Some(full) = full {
                        brief.deceased = Some(full);
                    }
                }
            }
            out.push(brief);
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

// ----------------------------------------------------------------------------
//  Сканы рядом с формой (спека 2026-10-10-skany-ryadom-s-formoj)
// ----------------------------------------------------------------------------

/// Что блок скана знает о папке дела.
#[derive(Serialize)]
struct ScanState {
    /// Папка сканов дела этого года; None — не выбрана.
    dir: Option<String>,
    /// Листы по порядку: имена файлов (scans::list), у PDF — «имя.pdf#N».
    files: Vec<String>,
    /// В папке и картинки, и PDF (спринт 2 от 03.11.2026): какой вид
    /// показан — «images» или «pdf»; None — вид в папке один.
    kind: Option<String>,
    /// Листы другого вида — запись, привязанная к ним, показывается всё равно.
    other: Vec<String>,
    /// Листы, которые программа показать не может, и почему.
    bad: std::collections::HashMap<String, String>,
    /// Разделитель пути этой системы — окно склеивает путь к файлу само.
    sep: String,
    /// Папка была выбрана, но сейчас не читается (другой компьютер, диск не подключён).
    missing: Option<String>,
    /// Файл, на котором остановились в прошлый раз, или тот, что перетащили.
    open: Option<String>,
}

fn scan_keys(year: Option<i64>) -> (String, String) {
    let y = year.map(|y| y.to_string()).unwrap_or_default();
    (format!("scan_dir_{y}"), format!("scan_last_{y}"))
}

/// Место в листах PDF помнится отдельно от места в картинках (проверяющий
/// 03.11.2026): заглянули в PDF — место в сотнях JPG той же папки не теряется.
fn scan_last_key(year: Option<i64>, file: &str) -> String {
    let y = year.map(|y| y.to_string()).unwrap_or_default();
    if genmetric_core::scans::split_sheet(file).1.is_some() { format!("scan_last_pdf_{y}") } else { format!("scan_last_{y}") }
}

/// Места, на которых остановились: в картинках и в листах PDF.
fn scan_lasts(conn: &Connection, year: Option<i64>) -> Result<Vec<String>, String> {
    let y = year.map(|y| y.to_string()).unwrap_or_default();
    Ok([format!("scan_last_{y}"), format!("scan_last_pdf_{y}")].iter()
        .filter_map(|k| parish::setting_get(conn, k).transpose())
        .collect::<Result<Vec<_>, _>>()?)
}

/// Вид листов в папке, где есть и картинки, и PDF: «images» или «pdf».
fn scan_kind_key(year: Option<i64>) -> String {
    format!("scan_kind_{}", year.map(|y| y.to_string()).unwrap_or_default())
}

fn scan_empty(dir: Option<String>, missing: Option<String>) -> ScanState {
    ScanState { dir, files: Vec::new(), kind: None, other: Vec::new(), bad: Default::default(),
                sep: std::path::MAIN_SEPARATOR.to_string(), missing, open: None }
}

/// Прочитать папку и разрешить окну её файлы — только её, без вложенных.
/// Каждая страница PDF — отдельный лист. Есть в папке картинки — PDF не
/// показываются (Роман 20.10, ответ 6.2: он распаковывает PDF в JPG «прямо
/// в ту же самую папку»), пока человек не выбрал вид «PDF» (`kind`).
fn scan_open_dir(handle: &tauri::AppHandle, log: &Path, dir: &str, open: Vec<String>, kind: Option<String>) -> ScanState {
    use genmetric_core::{pdf::Page, scans};
    let names = match scans::list(Path::new(dir)) {
        Ok(names) => names,
        Err(e) => return scan_empty(Some(dir.to_string()), Some(e)),
    };
    let sheets = handle.state::<scanserve::Sheets>();
    sheets.allow(Path::new(dir));
    let images: Vec<String> = names.iter().filter(|n| scans::is_image(n)).cloned().collect();
    let mut pdf_sheets = Vec::new();
    let mut bad = std::collections::HashMap::new();
    for name in names.iter().filter(|n| scans::is_pdf(n)) {
        match sheets.pdf_pages(&Path::new(dir).join(name)) {
            Ok(pages) => for (k, page) in pages.iter().enumerate() {
                let sheet = scans::sheet(name, k + 1);
                if let Page::Unsupported(why) = page {
                    bad.insert(sheet.clone(), why.clone());
                }
                pdf_sheets.push(sheet);
            },
            // Файл не разобрался — один лист с объяснением, а не тишина.
            Err(why) => {
                write_log(log, &format!("PDF «{name}» в папке «{dir}» не разобран: {why}"));
                let sheet = scans::sheet(name, 1);
                bad.insert(sheet.clone(), format!("PDF не читается: {why}"));
                pdf_sheets.push(sheet);
            }
        }
    }
    let both = !images.is_empty() && !pdf_sheets.is_empty();
    let pdf_shown = if both { kind.as_deref() == Some("pdf") } else { images.is_empty() };
    let (files, other) = if pdf_shown { (pdf_sheets, images) } else { (images, pdf_sheets) };
    let allowed = handle.asset_protocol_scope().allow_directory(dir, false);
    ScanState {
        dir: Some(dir.to_string()),
        // Первое из запомненных мест, что есть среди показанных листов.
        open: open.into_iter().find(|f| files.contains(f)),
        files,
        kind: both.then(|| if pdf_shown { "pdf" } else { "images" }.to_string()),
        other,
        bad,
        sep: std::path::MAIN_SEPARATOR.to_string(),
        missing: allowed.err().map(|e| format!("программа не получила доступ к папке: {e}")),
    }
}

/// Папка сканов дела этого года и её файлы. Не в главном потоке окна (как и
/// `scan_set_dir`, `scan_set_kind`): папка с PDF читается целиком при первом
/// открытии — 55 МБ файла Романа не должны останавливать окно.
#[tauri::command(async)]
fn scan_state(handle: tauri::AppHandle, app: State<App>, year: Option<i64>) -> Result<ScanState, String> {
    let (dir_key, _) = scan_keys(year);
    let (dir, last, kind) = with_conn(&app, "Папка сканов", |conn| {
        let mut dir = parish::setting_get(conn, &dir_key)?.filter(|d| !d.trim().is_empty());
        // Папку выбрали, когда у дела ещё не было года (новое дело): теперь
        // год есть — она переходит к нему, а не теряется (проверяющий 10.10.2026).
        if dir.is_none() && year.is_some() {
            let (loose_key, _) = scan_keys(None);
            if let Some(loose) = parish::setting_get(conn, &loose_key)?.filter(|d| !d.trim().is_empty()) {
                parish::setting_set(conn, &dir_key, &loose)?;
                parish::setting_set(conn, &loose_key, "")?;
                dir = Some(loose);
            }
        }
        Ok((dir, scan_lasts(conn, year)?, parish::setting_get(conn, &scan_kind_key(year))?))
    })?;
    Ok(match dir.filter(|d| !d.trim().is_empty()) {
        Some(dir) => scan_open_dir(&handle, &app.log_path, &dir, last, kind),
        None => scan_empty(None, None),
    })
}

/// В папке и картинки, и PDF: показать «images» или «pdf». Выбор —
/// настройка прихода по году дела, рядом с папкой.
#[tauri::command(async)]
fn scan_set_kind(handle: tauri::AppHandle, app: State<App>, year: Option<i64>, kind: String) -> Result<ScanState, String> {
    if kind != "images" && kind != "pdf" {
        return Err(format!("Вид сканов «{kind}» неизвестен"));
    }
    with_conn(&app, "Вид сканов", |conn| parish::setting_set(conn, &scan_kind_key(year), &kind))?;
    scan_state(handle, app, year)
}

/// Запомнить вид листов, не перечитывая папку: вид уже показан окном
/// (работают на листе другого вида, открытом по ссылке). Ключ настройки —
/// только здесь, в `scan_kind_key` (долг п. 113: окно собирало его само).
#[tauri::command]
fn scan_remember_kind(app: State<App>, year: Option<i64>, kind: String) -> Result<(), String> {
    if kind != "images" && kind != "pdf" {
        return Err(format!("Вид сканов «{kind}» неизвестен"));
    }
    with_conn(&app, "Вид сканов", |conn| parish::setting_set(conn, &scan_kind_key(year), &kind))
}

/// Системное окно «Выберите папку» и выбранная в нём папка. Окно открывает
/// Rust, а не страница: команда плагина разрешила бы окну выбранную папку
/// вместе со всеми вложенными (выбрали домашнюю папку — открыт весь диск), а
/// нам нужна только она сама (ревьюер 10.10.2026). None — человек передумал.
/// async — не в главном потоке: окно выбора ждёт человека.
#[tauri::command]
async fn scan_pick_dir(handle: tauri::AppHandle, app: State<'_, App>, year: Option<i64>)
    -> Result<Option<ScanState>, String>
{
    use tauri_plugin_dialog::DialogExt;
    let picked = handle.dialog().file().set_title("Папка со сканами дела").blocking_pick_folder();
    let Some(folder) = picked else { return Ok(None) };
    let path = folder.into_path().map_err(|e| format!("Папка сканов: путь не прочитан ({e})"))?;
    scan_set_dir(handle.clone(), app, year, path.to_string_lossy().to_string(), None).map(Some)
}

/// Выбрать папку сканов дела: путь к папке или к любому её файлу (так
/// приходит перетаскивание — перетащили файл, открывается он). `dropped` —
/// путь брошен на окно мышью: чужой файл (не скан) тогда не трогает ничего —
/// на окно бросают и другое, и папка сканов от этого меняться не должна.
#[tauri::command(async)]
fn scan_set_dir(handle: tauri::AppHandle, app: State<App>, year: Option<i64>, path: String,
                dropped: Option<bool>) -> Result<ScanState, String>
{
    let picked = Path::new(path.trim());
    let fail = |text: String| {
        write_log(&app.log_path, &format!("Папка сканов: {text}"));
        text
    };
    let (dir, file) = if picked.is_dir() {
        (picked.to_path_buf(), None)
    } else if picked.is_file() {
        let name = picked.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if !genmetric_core::scans::is_scan(&name) {
            if dropped == Some(true) {
                return scan_state(handle, app, year);
            }
            return Err(fail(format!("«{name}» — не скан: программа показывает файлы .jpg, .jpeg, .png, .tif и .pdf")));
        }
        let parent = picked.parent().ok_or_else(|| fail(format!("у файла «{path}» нет папки")))?;
        // Перетащили PDF — открывается его первая страница.
        let open = if genmetric_core::scans::is_pdf(&name) { genmetric_core::scans::sheet(&name, 1) } else { name };
        (parent.to_path_buf(), Some(open))
    } else {
        return Err(fail(format!("«{path}» — не папка и не файл")));
    };
    let dir = dir.to_string_lossy().to_string();
    let (dir_key, _) = scan_keys(year);
    let (last, kind) = with_conn(&app, "Папка сканов", |conn| {
        let before = parish::setting_get(conn, &dir_key)?;
        parish::setting_set(conn, &dir_key, &dir)?;
        // Перетащили файл — вид листов по нему: PDF или картинки.
        if let Some(f) = &file {
            let kind = if genmetric_core::scans::split_sheet(f).1.is_some() { "pdf" } else { "images" };
            parish::setting_set(conn, &scan_kind_key(year), kind)?;
        }
        let kind = parish::setting_get(conn, &scan_kind_key(year))?;
        // Та же папка выбрана снова — место в ней остаётся.
        Ok((if before.as_deref() == Some(dir.as_str()) { scan_lasts(conn, year)? } else { Vec::new() }, kind))
    })?;
    let state = scan_open_dir(&handle, &app.log_path, &dir, file.into_iter().chain(last).collect(), kind);
    if let Some(why) = &state.missing {
        write_log(&app.log_path, &format!("Папка сканов «{dir}»: {why}"));
    }
    Ok(state)
}

/// Запомнить открытый разворот — после перезапуска блок покажет его же.
#[tauri::command]
fn scan_remember(app: State<App>, year: Option<i64>, file: String) -> Result<(), String> {
    let last_key = scan_last_key(year, &file);
    // Листание — запись в базу прихода, а память поиска сверяется по счётчику
    // изменений соединения: каждое перелистывание заставляло бы перечитать все
    // упоминания прихода. Место скана упоминаний не меняет — память, свежая
    // до этой записи, остаётся свежей и после неё.
    with_conn(&app, "Открытый скан", |conn| {
        let before = genmetric_core::search::changes(conn)?;
        parish::setting_set(conn, &last_key, &file)?;
        let after = genmetric_core::search::changes(conn)?;
        let mut cache = app.search.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, _)) = cache.as_mut() {
            if *at == before {
                *at = after;
            }
        }
        drop(cache);
        let mut died = app.died.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, _)) = died.as_mut() {
            if *at == before {
                *at = after;
            }
        }
        Ok(())
    })
}

/// Блок скана включили — узкое окно расширяется на рабочую область экрана;
/// выключили — возвращается к прежней ширине. Окно, которое человек уже сам
/// сделал широким, не трогаем.
#[tauri::command]
fn scan_window(window: tauri::Window, wins: State<windows::Windows>, on: bool) -> Result<(), String> {
    let e = |e: tauri::Error| e.to_string();
    let inner = window.inner_size().map_err(e)?;
    let scale = window.scale_factor().map_err(e)?;
    // Развёрнутое на весь экран окно не трогаем ни в какую сторону.
    if window.is_maximized().unwrap_or(false) || window.is_fullscreen().unwrap_or(false) {
        if !on {
            wins.set_narrow(None);
        }
        return Ok(());
    }
    if !on {
        // Узкая ширина помнится и после перезапуска (ревьюер 28.10.2026, п. 97).
        // Хранится в логических точках: после перезапуска на мониторе другого
        // масштаба ширина та же (ревьюер 02.11.2026).
        if let Some((w, x)) = wins.narrow() {
            wins.set_narrow(None);
            let (width, x) = ((w as f64 * scale).round() as u32, (x as f64 * scale).round() as i32);
            window.set_size(tauri::PhysicalSize::new(width, inner.height)).map_err(e)?;
            let pos = window.outer_position().map_err(e)?;
            // Прежний левый край — на мониторе, которого уже нет: окно не уводим
            // за экран, оставляем где стоит.
            let on_monitor = window.current_monitor().ok().flatten().is_some_and(|m| {
                let a = m.work_area();
                x >= a.position.x && x < a.position.x + a.size.width as i32
            });
            if on_monitor {
                window.set_position(tauri::PhysicalPosition::new(x, pos.y)).map_err(e)?;
            }
        }
        return Ok(());
    }
    // Уже широкое (форма и скан помещаются) — оставляем как есть.
    if (inner.width as f64) / scale >= 900.0 {
        return Ok(());
    }
    let Some(monitor) = window.current_monitor().map_err(e)? else { return Ok(()) };
    let area = monitor.work_area();
    let frame = window.outer_size().map_err(e)?.width.saturating_sub(inner.width);
    let width = area.size.width.saturating_sub(frame).min((1600.0 * scale) as u32);
    if width <= inner.width {
        return Ok(());
    }
    let pos = window.outer_position().map_err(e)?;
    wins.set_narrow(Some(((inner.width as f64 / scale).round() as u32, (pos.x as f64 / scale).round() as i32)));
    window.set_size(tauri::PhysicalSize::new(width, inner.height)).map_err(e)?;
    // Не уходить за правый край экрана: сдвигаем окно влево, сколько нужно.
    let right = area.position.x + area.size.width as i32;
    if pos.x + (width + frame) as i32 > right {
        let x = (right - (width + frame) as i32).max(area.position.x);
        window.set_position(tauri::PhysicalPosition::new(x, pos.y)).map_err(e)?;
    }
    Ok(())
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
    let mut guard = lock_conn(app);
    drop_search(app);
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

/// Переименовать приход. Открытый — сразу и в заголовке окна.
#[tauri::command]
fn parish_rename(app: State<App>, window: tauri::Window, id: i64, name: String) -> Result<String, String> {
    let name = parish::rename(&app.data_dir, id, &name)?;
    let mut current = app.parish.lock().unwrap_or_else(|p| p.into_inner());
    if current.0 == id {
        current.1 = name.clone();
        let _ = window.set_title(&window_title(&name));
    }
    write_log(&app.log_path, &format!("Приход {id} переименован: «{name}»"));
    Ok(name)
}

/// Удалить приход: файл — в папку «Удалённые приходы», строка — из перечня.
#[tauri::command]
fn parish_delete(app: State<App>, id: i64) -> Result<String, String> {
    if app.parish.lock().map(|p| p.0 == id).unwrap_or(false) {
        return Err("открытый приход не удаляется — сначала откройте другой".into());
    }
    let kept = parish::remove(&app.data_dir, id)?;
    write_log(&app.log_path, &format!("Приход {id} удалён из перечня, файл: {}",
                                       if kept.is_empty() { "не было" } else { &kept }));
    Ok(kept)
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
        *lock_conn(&app) = None;
        drop_search(&app);
    }
    let made = parish::create_with(&app.data_dir, &bundled, &name, Some((&file_name, bytes.len() as i64)),
                                   replace, |conn| genmetric_core::import::import_into(conn, bytes));
    let (id, report) = match made {
        Ok(v) => v,
        Err(e) => {
            if was_open {
                // Вернуть прежний приход, чтобы окно не осталось без базы.
                *lock_conn(&app) = parish::open(&app.data_dir, &bundled, current).ok().map(|o| o.conn);
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

// ----------------------------------------------------------------------------
//  Импорт из шаблона Familio (спека 2026-10-17-import-iz-shablona-familio)
// ----------------------------------------------------------------------------

/// Начать выбор файлов Familio заново.
#[tauri::command]
fn familio_reset(app: State<App>) {
    let mut files = app.familio_files.lock().unwrap_or_else(|e| e.into_inner());
    files.clear();
    files.shrink_to_fit();
}

#[derive(Serialize)]
struct FamilioSeen {
    #[serde(flatten)]
    seen: genmetric_core::import_familio::FileSeen,
    size: usize,
    /// Приход, уже импортированный из этого файла (по имени и размеру).
    already_id: Option<i64>,
    already_name: Option<String>,
    /// Название нового прихода по всем выбранным файлам: «Н.П. (из Familio)».
    parish_village: String,
}

/// Присланный частями файл (`import_chunk`) — в список файлов Familio: что в
/// нём нашлось. Тот же файл второй раз — отказ.
#[tauri::command]
async fn familio_add(app: State<'_, App>, file_name: String) -> Result<FamilioSeen, String> {
    let fail = |e: String| {
        let msg = format!("Чтение файла «{file_name}»: {e}");
        write_log(&app.log_path, &msg);
        msg
    };
    let mut file = app.import_file.lock().unwrap_or_else(|e| e.into_inner());
    let bytes = std::mem::take(&mut *file);
    drop(file);
    let mut files = app.familio_files.lock().unwrap_or_else(|e| e.into_inner());
    // Тот же файл — отказ; другой файл с тем же именем — тоже: в списке на
    // сверку файлы различаются по имени (ревьюер 21.10.2026).
    if let Some((_, b)) = files.iter().find(|(n, _)| *n == file_name) {
        return Err(if b.len() == bytes.len() {
            format!("файл «{file_name}» уже выбран")
        } else {
            format!("файл с именем «{file_name}» уже выбран — переименуйте один из них")
        });
    }
    let seen = genmetric_core::guarded("разбор файла Familio", || genmetric_core::import_familio::inspect(&file_name, &bytes))
        .map_err(fail)?;
    let already = parish::imported_from(&app.data_dir, &file_name, bytes.len() as i64).map_err(fail)?;
    let size = bytes.len();
    files.push((file_name.clone(), bytes));
    let mut all: Vec<genmetric_core::import_familio::FileSeen> = Vec::new();
    for (n, b) in files.iter() {
        if *n == file_name {
            all.push(seen.clone());
        } else if let Ok(s) = genmetric_core::import_familio::inspect(n, b) {
            all.push(s);
        }
    }
    Ok(FamilioSeen {
        parish_village: genmetric_core::import_familio::parish_village(&all),
        seen,
        size,
        already_id: already.as_ref().map(|a| a.0),
        already_name: already.map(|a| a.1),
    })
}

/// Импорт выбранных файлов Familio в один новый приход — и он же открыт.
#[tauri::command]
async fn familio_run(app: State<'_, App>, window: tauri::Window, name: String,
                     replace: Option<i64>) -> Result<ImportDone, String> {
    let fail = |e: String| {
        let msg = format!("Импорт Familio: {e}");
        write_log(&app.log_path, &msg);
        msg
    };
    let bundled = bundled_seed(&app).map_err(fail)?;
    let mut files = app.familio_files.lock().unwrap_or_else(|e| e.into_inner());
    if files.is_empty() {
        return Err(fail("файлы не выбраны — выберите их ещё раз".into()));
    }
    // Один файл — именем и размером, как импорт Excel; несколько — строками.
    let (source, size) = if files.len() == 1 {
        (files[0].0.clone(), files[0].1.len() as i64)
    } else {
        let list: Vec<(String, i64)> = files.iter().map(|(n, b)| (n.clone(), b.len() as i64)).collect();
        (parish::sources_text(&list), list.iter().map(|x| x.1).sum())
    };
    // «Заменить тот приход» — как у импорта Excel (долг п. 81): открытый
    // заменяемый приход отпускаем, при неудаче — открываем обратно.
    let current = app.parish.lock().map(|p| p.0).unwrap_or(1);
    let was_open = replace == Some(current);
    if was_open {
        *lock_conn(&app) = None;
        drop_search(&app);
    }
    let made = parish::create_with(&app.data_dir, &bundled, &name, Some((&source, size)), replace,
                                   |conn| genmetric_core::import_familio::import_into(conn, &files));
    let (id, report) = match made {
        Ok(v) => v,
        Err(e) => {
            if was_open {
                *lock_conn(&app) = parish::open(&app.data_dir, &bundled, current).ok().map(|o| o.conn);
            }
            return Err(fail(e));
        }
    };
    write_log(&app.log_path, &format!(
        "Импорт Familio ({} файл.): рождений {}, браков {}, смертей {}; имён не сверено {}",
        files.len(), report.births, report.marriages, report.deaths, report.unknown_names));
    files.clear();
    files.shrink_to_fit();
    drop(files);
    let parish_name = switch_parish(&app, &window, id)?;
    Ok(ImportDone { report, parish_name })
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // Листы PDF и TIFF для блока скана (03.11.2026, scanserve.rs).
        .register_asynchronous_uri_scheme_protocol("scan", |ctx, request, responder| {
            scanserve::handle(ctx.app_handle().clone(), request, responder)
        })
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
            app.manage(windows::Windows::load(&data_dir));
            app.manage(scanserve::Sheets::new(&data_dir));
            app.manage(App {
                conn: Mutex::new(conn),
                db_path: Mutex::new(db_path),
                parish: Mutex::new(current),
                warning: Mutex::new(warning),
                import_file: Mutex::new(Vec::new()),
                familio_files: Mutex::new(Vec::new()),
                search: Mutex::new(None),
                died: Mutex::new(None),
                search_busy: std::sync::atomic::AtomicBool::new(false),
                search_live: std::sync::atomic::AtomicBool::new(false),
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
            if let Some(args) = windows::e2e_browser_args() {
                builder = builder.additional_browser_args(&args);
            }
            let win = builder.build()?;
            let _ = win.set_title(&title);
            // Окно встаёт, где его оставили (бэклог З3.2): место, размер,
            // развёрнутое — развёрнутым. Места нет или монитор отключён — как
            // раньше, по высоте экрана.
            let placed = windows::restore(&win, windows::MAIN).unwrap_or_else(|e| {
                write_log(&log_for_window, &format!("Окно не поставлено на прежнее место: {e}"));
                false
            });
            if placed {
                return Ok(());
            }
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
            child_full_iof,
            read_log,
            db_info,
            lookup_summary,
            case_load,
            case_years,
            case_open_year,
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
            place_merge,
            place_rows,
            cleanup_find,
            cleanup_remove,
            place_merge_uses,
            place_merge_candidates,
            set_always_on_top,
            scan_state,
            scan_set_dir,
            scan_pick_dir,
            scan_remember,
            scan_set_kind,
            scan_remember_kind,
            scan_window,
            windows::scan_detach,
            windows::scan_focus_main,
            parish_list,
            parish_open,
            parish_create,
            parish_rename,
            parish_delete,
            import_chunk,
            import_inspect,
            import_run,
            familio_reset,
            search_release,
            search_open,
            familio_add,
            familio_run,
            export::export_years,
            export::export_familio,
            export::export_excel,
            export::reveal_path,
            export::open_familio
        ])
        .on_window_event(windows::on_event)
        .build(tauri::generate_context!())
        .expect("не удалось запустить GenMetric")
        .run(|app, event| {
            match event {
                tauri::RunEvent::ExitRequested { .. } => windows::on_exit_requested(app),
                tauri::RunEvent::Exit => windows::on_exit(app),
                _ => {}
            }
        });
}
