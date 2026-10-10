//! Открытие и обновление базы пользователя. Тот же путь проходит и первая
//! база, и каждый файл прихода при открытии.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};

/// Версия схемы, которую понимает эта сборка.
pub const SCHEMA_VERSION: i64 = 11;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn seed() -> Option<PathBuf> {
        // Поставку собирает db/build_seed.py; в конвейере она есть до тестов.
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../resources/seed.sqlite");
        p.exists().then_some(p)
    }

    /// Обновление базы — настоящим SQLite программы. До 08.10.2026 `migrate.sql`
    /// исполнял только Python-тест, а у Python и у программы SQLite разных
    /// сборок: вложенную цепочку replace один разбирал, другой отвечал «parser
    /// stack overflow» (так упала сборка 08.10.2026). У человека это было бы
    /// «база не обновилась».
    #[test]
    fn upgrade_runs_in_app_sqlite() {
        let Some(seed) = seed() else {
            crate::seed_missing("тест обновления пропущен");
            return;
        };
        let dir = std::env::temp_dir().join(format!("genmetric-upgrade-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("genmetric.sqlite");
        std::fs::copy(&seed, &path).unwrap();
        {
            // «Прежняя установка»: другой отпечаток, запись со званием в
            // дореформенном написании и два своих звания — занятое и нет.
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "UPDATE setting SET value = 'прежняя' WHERE key = 'seed_stamp';
                 INSERT INTO mk_case (id, church, village, year) VALUES (1, 'Никольская', 'Никольское', 1890);
                 INSERT INTO entry (id, case_id, section, event_year) VALUES (1, 1, 1, 1890);
                 INSERT INTO person_mention (entry_id, role_code, sort_order, first_name, rank)
                 VALUES (1, 'father', 20, 'Иван', 'Безземельный крестьянинъ');
                 INSERT INTO lookup (kind, value, value_norm, sort_order, origin)
                 VALUES ('rank_m', 'безземельный крестьянин', 'безземельный крестьянин', 9001, 'user'),
                        ('rank_m', 'отставной канонир', 'отставной канонир', 9002, 'user');
                 INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count)
                 VALUES ('rank_m', 'global', '', 'отставной канонир', 'отставной канонир', 3);",
            ).unwrap();
        }
        let conn = open_database(&seed, &path).expect("обновление прошло");
        let num = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(num("SELECT count(*) FROM lookup WHERE value = 'безземельный крестьянин'"), 1, "занятое осталось");
        assert_eq!(num("SELECT count(*) FROM lookup WHERE value = 'отставной канонир'"), 0, "незанятое убрано");
        assert_eq!(num("SELECT count(*) FROM usage_stat WHERE value = 'отставной канонир'"), 0);
        assert_eq!(num("SELECT count(*) FROM lookup_dropped WHERE value_norm = 'отставной канонир'"), 1);
        assert_eq!(num("SELECT count(*) FROM person_mention WHERE rank = 'Безземельный крестьянинъ'"), 1, "запись цела");
        assert_eq!(num("SELECT count(*) FROM lookup WHERE kind = 'rank_m' AND origin = 'seed'"), 51);
        let stamp: String = conn.query_row("SELECT value FROM setting WHERE key = 'seed_stamp'", [], |r| r.get(0)).unwrap();
        assert_ne!(stamp, "прежняя", "отпечаток поставки обновлён");
        assert_eq!(num("SELECT max(version) FROM schema_version"), SCHEMA_VERSION);
        drop(conn);
        // Копия «до-обновления» сделана.
        let copies = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(BACKUP_MARK)).count();
        assert_eq!(copies, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// То же на настоящей базе — только у разработчика:
    ///     GENMETRIC_UPGRADE_DB=/путь/копия.sqlite cargo test -p genmetric-core upgrade_real -- --ignored --nocapture
    /// Файл обновляется на месте — давать копию.
    #[test]
    #[ignore]
    fn upgrade_real() {
        let seed = seed().expect("resources/seed.sqlite");
        let path = PathBuf::from(std::env::var("GENMETRIC_UPGRADE_DB").expect("GENMETRIC_UPGRADE_DB"));
        let count = |c: &Connection, t: &str| -> i64 { c.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0)).unwrap() };
        let before = { let c = Connection::open(&path).unwrap(); (count(&c, "entry"), count(&c, "person_mention"), count(&c, "lookup")) };
        let conn = open_database(&seed, &path).expect("обновление прошло");
        println!("записей {} → {}, упоминаний {} → {}, значений перечней {} → {}",
                 before.0, count(&conn, "entry"), before.1, count(&conn, "person_mention"), before.2, count(&conn, "lookup"));
        assert_eq!((before.0, before.1), (count(&conn, "entry"), count(&conn, "person_mention")));
    }
}
