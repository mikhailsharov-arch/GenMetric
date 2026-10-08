//! Открытие и обновление базы пользователя. Тот же путь проходит и первая
//! база, и каждый файл прихода при открытии.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};

/// Версия схемы, которую понимает эта сборка.
pub const SCHEMA_VERSION: i64 = 10;

/// Признак копии перед обновлением в имени файла.
pub const BACKUP_MARK: &str = "-до-обновления-";

/// Обновление справочников. Тот же файл прогоняет тест db/test_upgrade.py —
/// поэтому логика обновления проверена, хотя вызывающий её код на Rust
/// в песочнице не собирается.
pub const MIGRATE_SQL: &str = include_str!("../../../db/migrate.sql");

/// Открывает базу пользователя, при необходимости обновляя её из поставки.
///
/// База копируется в папку пользователя только при первой установке. Если
/// оставить только копирование, обновления схемы и справочников до человека
/// не доедут: он ставит новую версию поверх старой, а работает по-прежнему
/// со старой базой. Именно так вышло 13.08.2026 — у тестировщика не появилась
/// таблица name_form, и половина сборки молча не работала.
pub fn open_database(bundled: &Path, db_path: &Path) -> Result<Connection, Box<dyn std::error::Error>> {
    if !db_path.exists() {
        std::fs::copy(bundled, db_path)?;
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        return Ok(conn);
    }

    let conn = Connection::open(db_path)?;
    // WAL включается здесь, а не в schema.sql: это настройка соединения,
    // и в файле схемы она ломает сборку на сетевых файловых системах.
    // Именно execute_batch, а не pragma_update: PRAGMA journal_mode возвращает
    // строку результата, и pragma_update на этом падает.
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;

    let version: i64 = conn
        .query_row("SELECT coalesce(max(version), 0) FROM schema_version", [], |r| r.get(0))
        .unwrap_or(0);
    let stamp: String = conn
        .query_row("SELECT value FROM setting WHERE key = 'seed_stamp'", [], |r| r.get(0))
        .optional()?
        .unwrap_or_default();

    let bundled_stamp = {
        let seed = Connection::open(bundled)?;
        let value: Option<String> = seed
            .query_row("SELECT value FROM setting WHERE key = 'seed_stamp'", [], |r| r.get(0))
            .optional()?;
        value.unwrap_or_default()
    };

    if version >= SCHEMA_VERSION && stamp == bundled_stamp && !stamp.is_empty() {
        return Ok(conn); // база свежая, делать нечего
    }

    backup(&conn, db_path)?;
    upgrade(&conn, bundled, version)?;
    Ok(conn)
}

/// Копия базы перед обновлением. Дёшево и один раз спасёт.
///
/// Сначала — контрольная точка WAL: после сбоя прошлого сеанса часть данных
/// лежит в файле -wal, и копия одного основного файла отстала бы от базы.
/// С 21.09.2026 обновление правит набранные записи, так что копия обязана
/// быть полной (ревьюер).
///
/// Называется по файлу базы: у первого прихода — «genmetric-до-обновления-…»,
/// как всегда было, у остальных — «2-Николо-Макарово-до-обновления-…». Раньше
/// копии всех приходов назывались одинаково, и понять, чья копия, было нельзя.
pub fn backup(conn: &Connection, db_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let stem = db_path.file_stem().map(|x| x.to_string_lossy().to_string())
        .filter(|x| !x.is_empty()).unwrap_or_else(|| "genmetric".to_string());
    let name = format!("{stem}{BACKUP_MARK}{seconds}.sqlite");
    let target = db_path.with_file_name(name);
    std::fs::copy(db_path, target)?;
    Ok(())
}

/// Колонки, которых нет в таблицах пользователя, — по образцу поставки
/// (схема 6, 25.09.2026: `person_mention.kinship` для браков). Шаг
/// «недостающие таблицы» новую колонку в старой таблице не видит; этот —
/// видит. Только добавление: тип из поставки, без ограничений и значений по
/// умолчанию — SQLite не даёт ALTER ADD COLUMN с ними в общем случае.
/// Тот же шаг повторяет db/test_upgrade.py.
pub fn add_missing_columns(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let tables: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT name FROM seed.sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for table in tables {
        let cols = |schema: &str| -> Result<Vec<(String, String)>, rusqlite::Error> {
            let mut stmt = conn.prepare(&format!("PRAGMA {schema}.table_info(\"{table}\")"))?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
            rows.collect()
        };
        let have: Vec<String> = cols("main")?.into_iter().map(|(n, _)| n).collect();
        if have.is_empty() {
            continue; // таблицы нет — её создал шаг выше или она не нужна
        }
        for (name, ty) in cols("seed")? {
            if !have.contains(&name) {
                conn.execute_batch(&format!("ALTER TABLE main.\"{table}\" ADD COLUMN \"{name}\" {ty}"))?;
            }
        }
    }
    Ok(())
}

/// Обновление базы пользователя до текущей версии поставки.
///
/// Шаг первый: недостающие колонки в существующих таблицах (add_missing_columns,
/// схема 6), затем недостающие таблицы и индексы по образцу из поставки.
/// Колонки добавляются только простые: без NOT NULL и DEFAULT из поставки.
///
/// Шаг второй: обновляем справочники по db/migrate.sql. Тот же файл прогоняет
/// тест db/test_upgrade.py, поэтому логика обновления проверена по-настоящему.
pub fn upgrade(conn: &Connection, bundled: &Path, from: i64) -> Result<(), Box<dyn std::error::Error>> {
    conn.execute_batch("PRAGMA foreign_keys = OFF;")?;
    conn.execute("ATTACH DATABASE ?1 AS seed", [bundled.to_string_lossy().to_string()])?;

    let missing: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT name, sql FROM seed.sqlite_master
              WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%'",
        )?;
        let items: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut out = Vec::new();
        for (name, sql) in items {
            let exists: i64 = conn.query_row(
                "SELECT count(*) FROM main.sqlite_master WHERE name = ?1",
                [&name],
                |r| r.get(0),
            )?;
            if exists == 0 {
                out.push(sql);
            }
        }
        out
    };
    // Сначала колонки в существующих таблицах, потом недостающие таблицы и
    // индексы: будущий индекс по новой колонке иначе уронил бы обновление
    // (ревьюер 25.09.2026). Таблиц, которых ещё нет, шаг колонок не трогает.
    add_missing_columns(conn)?;
    for sql in missing {
        conn.execute_batch(&sql)?;
    }

    conn.execute_batch(MIGRATE_SQL)?;

    if from < SCHEMA_VERSION {
        conn.execute("INSERT INTO schema_version (version) VALUES (?1)", [SCHEMA_VERSION])?;
    }

    conn.execute("DETACH DATABASE seed", [])?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(())
}
