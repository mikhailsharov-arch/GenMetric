//! Приходы (спека 2026-10-02, п. 1 и 2).
//!
//! Каждый приход — самостоятельный файл SQLite той же схемы; программа держит
//! открытым один. Перечень приходов, настройки окна и общие справочники лежат
//! в общем файле рядом (`genmetric-общее.sqlite`, db/common.sql). К соединению
//! открытого прихода общий файл подключён как `common` — так справочники
//! сверяются одним SQL-файлом (db/parish_sync.sql), который прогоняет и
//! db/test_parish.py.
//!
//! Роман 02.10.2026: файлы нужны, чтобы «индексировать сразу несколько
//! приходов одновременно, переключаясь между ними … не смешивая данные в
//! одну кучу».

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Serialize;

use crate::db::open_database;
use crate::export::{parish_name, safe_name};
use crate::text::normalize;

pub const COMMON_SQL: &str = include_str!("../../../db/common.sql");
pub const SYNC_SQL: &str = include_str!("../../../db/parish_sync.sql");

/// Первый приход — прежняя единственная база: остаётся на месте.
pub const FIRST_FILE: &str = "genmetric.sqlite";
pub const COMMON_FILE: &str = "genmetric-общее.sqlite";
/// Подпапка новых приходов в папке данных.
pub const PARISH_DIR: &str = "приходы";
const NO_NAME: &str = "Без названия";
const NAME_MAX: usize = 60;

/// Настройки окна — общие для всех приходов; остальные ключи (отпечаток
/// поставки, загруженный архив, счётчики исправлений) — у каждого прихода свои.
pub const COMMON_SETTINGS: &[&str] = &["ui_font_scale", "ui_one_column", "clergy_open", "familio_about"];

#[derive(Serialize, Debug, Clone)]
pub struct ParishRow {
    pub id: i64,
    pub name: String,
    pub file: String,
    pub opened_at: Option<String>,
    /// Число записей; None — файл не читается.
    pub entries: Option<i64>,
    /// Файл убрали руками: удаления в программе нет, приход остаётся в
    /// перечне с пометкой «файл не найден».
    pub missing: bool,
    pub current: bool,
    /// Имя файла Excel, из которого приход импортирован.
    pub source_name: Option<String>,
}

pub struct Opened {
    pub conn: Connection,
    pub id: i64,
    pub name: String,
    pub path: PathBuf,
    /// Сверка справочников не прошла — приход открыт, работа продолжается
    /// со справочниками прихода (спека, п. 2.4).
    pub warning: Option<String>,
}

fn s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

pub fn common_path(dir: &Path) -> PathBuf {
    dir.join(COMMON_FILE)
}

/// Общий файл: создаётся при первом обращении; прежняя база становится
/// первым приходом.
pub fn open_common(dir: &Path) -> Result<Connection, String> {
    let conn = Connection::open(common_path(dir)).map_err(|e| format!("общий файл не открылся: {e}"))?;
    conn.busy_timeout(std::time::Duration::from_secs(3)).map_err(s)?;
    conn.execute_batch(COMMON_SQL).map_err(|e| format!("общий файл не размечен: {e}"))?;
    // Колонка комментария пункта (08.10.2026): общий файл размечается только
    // CREATE … IF NOT EXISTS, и у прежнего файла новой колонки не появилось бы.
    let has_comment: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('place') WHERE name = 'comment'")
        .and_then(|mut st| st.exists([]))
        .map_err(s)?;
    if !has_comment {
        conn.execute_batch("ALTER TABLE place ADD COLUMN comment TEXT")
            .map_err(|e| format!("общий файл: не добавлена колонка комментария пункта: {e}"))?;
    }
    conn.execute("INSERT OR IGNORE INTO parish (id, file) VALUES (1, ?1)", [FIRST_FILE]).map_err(s)?;
    adopt_orphans(&conn, dir)?;
    Ok(conn)
}

/// Общий файл пропал (удалён, восстановлена одна первая база), а файлы
/// приходов лежат в папке: без этого шага они есть на диске, но из программы
/// недостижимы — и человек об этом не узнаёт (ревьюер 02.10.2026). Только
/// когда в перечне один первый приход: так шаг не трогает файл, который
/// прямо сейчас создаётся (create_with), и убранные «-заменён-».
fn adopt_orphans(common: &Connection, dir: &Path) -> Result<(), String> {
    let known: i64 = common.query_row("SELECT count(*) FROM parish", [], |r| r.get(0)).map_err(s)?;
    let Ok(files) = std::fs::read_dir(dir.join(PARISH_DIR)) else { return Ok(()) };
    if known != 1 {
        return Ok(());
    }
    let mut found: Vec<String> = files
        .filter_map(|f| f.ok().map(|f| f.file_name().to_string_lossy().to_string()))
        // Не приходы: прежний файл заменённого прихода и копии перед обновлением.
        .filter(|n| n.ends_with(".sqlite") && !n.contains("-заменён-") && !n.contains(crate::db::BACKUP_MARK))
        .collect();
    found.sort();
    for file in found {
        // «2-Николо-Макарово.sqlite» → номер 2, название «Николо-Макарово».
        let stem = file.trim_end_matches(".sqlite");
        let (id, name) = match stem.split_once('-') {
            Some((n, rest)) if n.parse::<i64>().is_ok() => (n.parse::<i64>().ok(), rest.to_string()),
            _ => (None, stem.to_string()),
        };
        common
            .execute(
                "INSERT OR IGNORE INTO parish (id, name, file) VALUES (?1, ?2, ?3)",
                rusqlite::params![id.filter(|n| *n > 1), name, format!("{PARISH_DIR}/{file}")],
            )
            .map_err(s)?;
    }
    Ok(())
}

fn current_id(common: &Connection) -> i64 {
    common
        .query_row("SELECT CAST(value AS INTEGER) FROM setting WHERE key = 'current_parish'", [], |r| r.get(0))
        .optional()
        .ok()
        .flatten()
        .unwrap_or(1)
}

/// Название прихода: заданное при создании, иначе — село из дела. Пусто —
/// у первого прихода, пока дело не заполнено.
fn display_name(name: Option<String>, conn: Option<&Connection>) -> String {
    name.filter(|n| !n.trim().is_empty())
        .or_else(|| conn.map(parish_name).filter(|n| !n.trim().is_empty()))
        .unwrap_or_default()
}

/// Перечень приходов для окна «Приходы».
pub fn list(dir: &Path) -> Result<Vec<ParishRow>, String> {
    let common = open_common(dir)?;
    let current = current_id(&common);
    let mut stmt = common
        .prepare("SELECT id, name, file, opened_at, source_name FROM parish ORDER BY id")
        .map_err(s)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?, r.get::<_, Option<String>>(4)?))
        })
        .map_err(s)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(s)?;
    let mut out = Vec::new();
    for (id, name, file, opened_at, source_name) in rows {
        let path = dir.join(&file);
        let missing = !path.exists();
        let peek = if missing {
            None
        } else {
            Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()
        };
        let entries = peek
            .as_ref()
            .and_then(|c| c.query_row("SELECT count(*) FROM entry", [], |r| r.get(0)).ok());
        out.push(ParishRow {
            id,
            name: Some(display_name(name, peek.as_ref())).filter(|n| !n.is_empty())
                .unwrap_or_else(|| NO_NAME.to_string()),
            file,
            opened_at,
            entries,
            missing,
            current: id == current,
            source_name,
        });
    }
    Ok(out)
}

/// Подключить общий файл к соединению прихода и перенести туда настройки
/// окна, которых там ещё нет (у первого прихода они лежали в его базе).
fn attach_common(conn: &Connection, dir: &Path) -> Result<(), String> {
    // Общий файл на мгновение занят другим соединением (перечень приходов,
    // настройка из окна) — подождать, а не падать с «database is locked».
    conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(s)?;
    conn.execute("ATTACH DATABASE ?1 AS common", [common_path(dir).to_string_lossy().to_string()])
        .map_err(|e| format!("общий файл не подключился: {e}"))?;
    for key in COMMON_SETTINGS {
        conn.execute(
            "INSERT OR IGNORE INTO common.setting (key, value) SELECT key, value FROM main.setting WHERE key = ?1",
            [key],
        )
        .map_err(s)?;
    }
    Ok(())
}

/// Сверка справочников прихода с общим файлом — в обе стороны, одной
/// транзакцией. Соединение должно быть с подключённым `common`.
pub fn sync(conn: &Connection) -> Result<(), String> {
    if !has_common(conn) {
        return Err("общий файл не подключён".into());
    }
    conn.execute_batch("BEGIN IMMEDIATE").map_err(s)?;
    match conn.execute_batch(SYNC_SQL) {
        Ok(()) => conn.execute_batch("COMMIT").map_err(s),
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e.to_string())
        }
    }
}

pub fn has_common(conn: &Connection) -> bool {
    conn.query_row("SELECT 1 FROM pragma_database_list WHERE name = 'common'", [], |_| Ok(()))
        .optional()
        .ok()
        .flatten()
        .is_some()
}

/// Открыть приход по номеру: обновление из поставки (как у любой базы),
/// подключение общего файла, сверка справочников.
pub fn open(dir: &Path, bundled: &Path, id: i64) -> Result<Opened, String> {
    let common = open_common(dir)?;
    let (name, file): (Option<String>, String) = common
        .query_row("SELECT name, file FROM parish WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()
        .map_err(s)?
        .ok_or_else(|| format!("прихода № {id} нет в перечне"))?;
    let path = dir.join(&file);
    // Только первая база создаётся из поставки сама (первый запуск программы).
    // Пропавший файл другого прихода молча пустым не подменяется.
    if id != 1 && !path.exists() {
        return Err(format!("файл прихода не найден: {}", path.display()));
    }
    let conn = open_database(bundled, &path).map_err(s)?;
    common
        .execute("UPDATE parish SET opened_at = datetime('now', 'localtime') WHERE id = ?1", [id])
        .map_err(s)?;
    common
        .execute(
            "INSERT INTO setting (key, value) VALUES ('current_parish', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [id.to_string()],
        )
        .map_err(s)?;
    drop(common);

    // Фамилии уже набранных записей — в подсказку фамилий (один раз).
    let seeded = crate::records::seed_surnames(&conn).map(|_| ())
        .and_then(|()| crate::records::seed_usage(&conn))
        .err()
        .map(|e| format!("Подсказки по частоте не подготовлены: {e}"));
    let synced = attach_common(&conn, dir)
        .and_then(|()| sync(&conn))
        .err()
        .map(|e| format!("Справочники не сверены с общими: {e}"));
    let warning = match (synced, seeded) {
        (Some(a), Some(b)) => Some(format!("{a} {b}")),
        (a, b) => a.or(b),
    };
    let name = display_name(name, Some(&conn));
    Ok(Opened { conn, id, name, path, warning })
}

/// Открытие при запуске: приход, с которым работали последним. Не открылся
/// (файл убрали) — первый, с предупреждением. Общий файл не открылся совсем —
/// прежняя база напрямую: работать можно, приходы недоступны.
pub fn open_current(dir: &Path, bundled: &Path) -> Result<Opened, String> {
    let id = match open_common(dir) {
        Ok(common) => current_id(&common),
        Err(e) => {
            let path = dir.join(FIRST_FILE);
            let conn = open_database(bundled, &path).map_err(s)?;
            let name = display_name(None, Some(&conn));
            return Ok(Opened { conn, id: 1, name, path, warning: Some(format!("Приходы недоступны: {e}")) });
        }
    };
    match open(dir, bundled, id) {
        Ok(opened) => Ok(opened),
        Err(e) if id != 1 => {
            let mut first = open(dir, bundled, 1)?;
            let note = format!("Приход, с которым работали в прошлый раз, не открылся ({e}) — открыт первый.");
            first.warning = Some(match first.warning.take() {
                Some(w) => format!("{note} {w}"),
                None => note,
            });
            Ok(first)
        }
        Err(e) => Err(e),
    }
}

/// `except` — приход, который заменяется: его название занять можно.
fn check_name(dir: &Path, name: &str, except: Option<i64>) -> Result<String, String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err("у прихода должно быть название".into());
    }
    // Название идёт в имя файла: длинное система не примет (и сообщит
    // по-английски), а в перечне и заголовке окна оно не поместится.
    if name.chars().count() > NAME_MAX {
        return Err(format!("название прихода длиннее {NAME_MAX} знаков — сократите"));
    }
    // Сравнение — с тем, что человек видит в перечне (у первого прихода это
    // село из дела), ключом поиска программы: нижний регистр SQLite кириллицу
    // не знает.
    let taken = list(dir)?.into_iter().any(|r| Some(r.id) != except && normalize(&r.name) == normalize(&name));
    if taken {
        return Err(format!("приход «{name}» уже есть — назовите иначе, чтобы не путать"));
    }
    Ok(name)
}

/// Новый пустой приход: файл из поставки — без записей, персон и причта.
/// `fill` наполняет его (импорт из Excel) до того, как приход попадёт в
/// перечень: при ошибке файла и строки в перечне не остаётся (спека, п. 4.5).
pub fn create_with<T>(
    dir: &Path,
    bundled: &Path,
    name: &str,
    source: Option<(&str, i64)>,
    replacing: Option<i64>,
    fill: impl FnOnce(&Connection) -> Result<T, String>,
) -> Result<(i64, T), String> {
    let name = check_name(dir, name, replacing)?;
    let common = open_common(dir)?;
    let folder = dir.join(PARISH_DIR);
    std::fs::create_dir_all(&folder).map_err(|e| format!("не создана папка {}: {e}", folder.display()))?;
    let next: i64 = common.query_row("SELECT coalesce(max(id), 0) + 1 FROM parish", [], |r| r.get(0)).map_err(s)?;
    let file = format!("{PARISH_DIR}/{next}-{}.sqlite", safe_name(&name));
    let path = dir.join(&file);
    if path.exists() {
        return Err(format!("файл {} уже есть — уберите его или назовите приход иначе", path.display()));
    }
    let remove = |path: &Path| {
        for suffix in ["", "-wal", "-shm"] {
            let mut p = path.as_os_str().to_owned();
            p.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(p));
        }
    };
    let filled = (|| -> Result<T, String> {
        let conn = open_database(bundled, &path).map_err(s)?;
        // Справочники — сразу общие: импорт сверяет имена и заводит пункты
        // уже с учётом набранного в других приходах.
        attach_common(&conn, dir)?;
        sync(&conn)?;
        // Обычная транзакция, не IMMEDIATE: та заняла бы и подключённый общий
        // файл на всё время импорта, и запись настройки из окна ждала бы и
        // падала с «database is locked» (ревьюер 02.10.2026). Наполнение в
        // общий файл не пишет.
        conn.execute_batch("BEGIN").map_err(s)?;
        // Паника в наполнении (битый файл Excel) — та же ошибка: откат и
        // уборка файла ниже, а не файл прихода с -wal без строки в перечне.
        let out = match crate::guarded("наполнение нового прихода", || fill(&conn)) {
            Ok(v) => v,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        };
        conn.execute_batch("COMMIT").map_err(s)?;
        sync(&conn)?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").map_err(s)?;
        Ok(out)
    })();
    let out = match filled {
        Ok(v) => v,
        Err(e) => {
            remove(&path);
            return Err(e);
        }
    };
    // Замена — до записи в перечень: не удалась (прежний файл занят) — нового
    // прихода не остаётся, а не «ошибка, но приход появился» (проверяющий).
    if let Some(old) = replacing {
        if let Err(e) = replace(dir, old, next) {
            remove(&path);
            return Err(e);
        }
    }
    let registered = common.execute(
        "INSERT INTO parish (id, name, file, source_name, source_size) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![next, name, file, source.map(|x| x.0), source.map(|x| x.1)],
    );
    if let Err(e) = registered {
        remove(&path);
        return Err(e.to_string());
    }
    Ok((next, out))
}

pub fn create(dir: &Path, bundled: &Path, name: &str) -> Result<i64, String> {
    create_with(dir, bundled, name, None, None, |_| Ok(())).map(|(id, ())| id)
}

/// Приход, уже импортированный из этого файла Excel (по имени и размеру).
pub fn imported_from(dir: &Path, source_name: &str, source_size: i64) -> Result<Option<(i64, String)>, String> {
    let common = open_common(dir)?;
    common
        .query_row(
            "SELECT id, coalesce(name, '') FROM parish WHERE source_name = ?1 AND source_size = ?2 ORDER BY id DESC LIMIT 1",
            rusqlite::params![source_name, source_size],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(s)
}

/// «Заменить тот приход»: прежний файл не удаляется — переименовывается и
/// уходит из перечня; его место и название занимает новый приход.
pub fn replace(dir: &Path, old_id: i64, new_id: i64) -> Result<(), String> {
    if old_id == 1 || old_id == new_id {
        return Err("этот приход заменить нельзя".into());
    }
    let common = open_common(dir)?;
    let old_file: String = common
        .query_row("SELECT file FROM parish WHERE id = ?1", [old_id], |r| r.get(0))
        .map_err(s)?;
    let old_path = dir.join(&old_file);
    if old_path.exists() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let kept = old_path.with_file_name(format!(
            "{}-заменён-{stamp}.sqlite",
            old_path.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default()
        ));
        std::fs::rename(&old_path, &kept).map_err(|e| format!("прежний файл не переименован: {e}"))?;
    }
    common.execute("DELETE FROM parish WHERE id = ?1", [old_id]).map_err(s)?;
    Ok(())
}

// ----------------------------------------------------------------------------
//  Настройки: окно — в общем файле, остальное — в приходе
// ----------------------------------------------------------------------------

fn setting_table(conn: &Connection, key: &str) -> &'static str {
    if COMMON_SETTINGS.contains(&key) && has_common(conn) { "common.setting" } else { "main.setting" }
}

pub fn setting_get(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    let table = setting_table(conn, key);
    conn.query_row(&format!("SELECT value FROM {table} WHERE key = ?1"), [key], |r| r.get(0))
        .optional()
        .map_err(s)
}

pub fn setting_set(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    let table = setting_table(conn, key);
    conn.execute(
        &format!("INSERT INTO {table} (key, value) VALUES (?1, ?2)
                  ON CONFLICT(key) DO UPDATE SET value = excluded.value"),
        [key, value],
    )
    .map(|_| ())
    .map_err(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> Option<PathBuf> {
        // Поставку собирает db/build_seed.py; в конвейере она есть до тестов.
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../resources/seed.sqlite");
        p.exists().then_some(p)
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("genmetric-parish-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn open_create_and_share() {
        let Some(seed) = seed() else {
            eprintln!("нет resources/seed.sqlite — тест приходов пропущен");
            return;
        };
        let dir = temp_dir("share");

        // Первый запуск: прежняя база — первый приход.
        let first = open_current(&dir, &seed).unwrap();
        assert_eq!(first.id, 1);
        assert!(first.warning.is_none(), "{:?}", first.warning);
        assert!(dir.join(FIRST_FILE).exists() && common_path(&dir).exists());
        first.conn.execute(
            "INSERT INTO place (name, name_norm, np_type, uyezd, origin, updated_at)
             VALUES ('Новосёлки Дальние', 'новоселки дальние', 'д.', 'Макарьевский', 'user', datetime('now'))", [])
            .unwrap();
        first.conn.execute(
            "INSERT INTO person_index (iof, iof_norm, place, rank, gender) VALUES ('Пётр Сидоров', 'петр сидоров', '', '', 'М')", [])
            .unwrap();
        setting_set(&first.conn, "ui_font_scale", "125").unwrap();
        sync(&first.conn).unwrap();
        drop(first);

        // Новый приход: пустой, но со справочниками и настройками окна.
        let id = create(&dir, &seed, "Николо-Макарово").unwrap();
        assert_eq!(id, 2);
        assert!(create(&dir, &seed, " николо-макарово ").is_err(), "двойник названия");
        assert!(create(&dir, &seed, &"я".repeat(61)).is_err(), "слишком длинное название");
        // Первый приход называется по селу дела — и это название занято тоже.
        let first = open(&dir, &seed, 1).unwrap();
        first.conn.execute("INSERT INTO mk_case (id, village, updated_at) VALUES (1, 'Борисоглебское', datetime('now'))", []).unwrap();
        drop(first);
        assert!(create(&dir, &seed, "борисоглебское").is_err(), "название первого прихода занято");
        let second = open(&dir, &seed, id).unwrap();
        assert_eq!(second.name, "Николо-Макарово");
        let one = |sql: &str| -> i64 { second.conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(one("SELECT count(*) FROM place WHERE name = 'Новосёлки Дальние' AND np_type = 'д.'"), 1);
        assert_eq!(one("SELECT count(*) FROM person_index"), 0, "персоны не общие");
        assert_eq!(one("SELECT count(*) FROM entry"), 0);
        assert_eq!(setting_get(&second.conn, "ui_font_scale").unwrap().as_deref(), Some("125"));
        drop(second);

        let rows = list(&dir).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows[1].current && !rows[0].current && rows[1].entries == Some(0));

        // Запуск открывает последний; пропавший файл — первый с предупреждением.
        assert_eq!(open_current(&dir, &seed).unwrap().id, 2);
        std::fs::remove_file(dir.join(&rows[1].file)).unwrap();
        let back = open_current(&dir, &seed).unwrap();
        assert_eq!(back.id, 1);
        assert!(back.warning.is_some());
        assert!(list(&dir).unwrap()[1].missing);

        // Общий файл пропал — приходы из папки возвращаются в перечень.
        let lost = create(&dir, &seed, "Потерянный").unwrap();
        std::fs::remove_file(common_path(&dir)).unwrap();
        let rows = list(&dir).unwrap();
        assert!(rows.iter().any(|r| r.id == lost && r.name == "Потерянный" && !r.missing), "{rows:?}");
        let n_before = rows.len();

        // Сбой наполнения — ни файла, ни строки в перечне.
        let failed = create_with(&dir, &seed, "Сбойный", None, None, |_| Err::<(), _>("нарочно".to_string()));
        assert!(failed.is_err());
        assert_eq!(list(&dir).unwrap().len(), n_before);
        assert!(!std::fs::read_dir(dir.join(PARISH_DIR)).unwrap()
            .any(|f| f.unwrap().file_name().to_string_lossy().contains("Сбойный")));

        // Паника наполнения (битый файл Excel) — то же: ошибка, а не падение,
        // и ни файла с -wal/-shm, ни строки в перечне.
        let crashed = create_with(&dir, &seed, "Паника", None, None, |conn| -> Result<(), String> {
            conn.execute("INSERT INTO lookup (kind, value, value_norm) VALUES ('rank_m', 'мусор', 'мусор')", []).unwrap();
            panic!("разбор сломался")
        });
        let why = crashed.unwrap_err();
        assert!(why.contains("внутренняя ошибка") && why.contains("разбор сломался"), "{why}");
        assert_eq!(list(&dir).unwrap().len(), n_before);
        assert!(!std::fs::read_dir(dir.join(PARISH_DIR)).unwrap()
            .any(|f| f.unwrap().file_name().to_string_lossy().contains("Паника")));
        // Копия перед обновлением лежит рядом с приходом и называется по его
        // файлу; при пропаже общего файла приходом она не становится.
        let lost_file = list(&dir).unwrap().into_iter().find(|r| r.id == lost).unwrap().file;
        let lost_path = dir.join(&lost_file);
        crate::db::backup(&Connection::open(&lost_path).unwrap(), &lost_path).unwrap();
        let copies: Vec<String> = std::fs::read_dir(dir.join(PARISH_DIR)).unwrap()
            .map(|f| f.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(crate::db::BACKUP_MARK)).collect();
        assert_eq!(copies.len(), 1, "{copies:?}");
        assert!(copies[0].starts_with(&format!("{lost}-Потерянный{}", crate::db::BACKUP_MARK)), "{copies:?}");
        std::fs::remove_file(common_path(&dir)).unwrap();
        assert_eq!(list(&dir).unwrap().len(), n_before, "копия «до-обновления» — не приход");

        // Импорт и повторный импорт того же файла: «заменить тот приход» —
        // прежний файл остаётся на диске под другим именем, в перечне — новый.
        let xlsx: &[u8] = include_bytes!("../../../db/fixtures/indexer.xlsx");
        let import = |replacing| create_with(&dir, &seed, "Никольское (из Excel)", Some(("indexer.xlsx", xlsx.len() as i64)),
                                             replacing, |conn| crate::import::import_into(conn, xlsx));
        let (first_id, report) = import(None).unwrap();
        assert_eq!((report.births, report.marriages, report.deaths), (4, 2, 4));
        assert_eq!(imported_from(&dir, "indexer.xlsx", xlsx.len() as i64).unwrap().map(|x| x.0), Some(first_id));
        assert!(import(None).is_err(), "то же название без замены — отказ");
        let (second_id, _) = import(Some(first_id)).unwrap();
        let rows = list(&dir).unwrap();
        assert_eq!(rows.iter().filter(|r| r.name == "Никольское (из Excel)").map(|r| r.id).collect::<Vec<_>>(), vec![second_id]);
        assert_eq!(rows.iter().find(|r| r.id == second_id).unwrap().entries, Some(10));
        let kept = std::fs::read_dir(dir.join(PARISH_DIR)).unwrap()
            .filter(|f| f.as_ref().unwrap().file_name().to_string_lossy().contains("заменён")).count();
        assert_eq!(kept, 1);
        // Импортированный приход открывается, а его пункты уже в общих справочниках.
        let opened = open(&dir, &seed, second_id).unwrap();
        assert!(opened.warning.is_none(), "{:?}", opened.warning);
        let shared: i64 = opened.conn.query_row(
            "SELECT count(*) FROM common.place WHERE name = 'Новое Тестово'", [], |r| r.get(0)).unwrap();
        assert_eq!(shared, 1);
        drop(opened);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
