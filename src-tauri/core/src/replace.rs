//! «Найти в записях» и «Заменить во всех» (спека 2026-11-13-sprint-139-149,
//! новая функция 2). Роман 12.11.2026: опечатка «головая боль» стоит в
//! какой-то записи — её не найти среди сотен, а удалить значение из
//! перечня значит потерять причину в той записи. Здесь — где стоит значение
//! и замена во всех записях: каждая запись пересохраняется тем же путём, что
//! правка в форме (`save_entry_in_tx`) — память подсказок снимается и
//! пополняется сама. Закрывает и задачу 11 бэклога Романа (поиск фамилии
//! «попов» в таком написании).

use rusqlite::Connection;
use serde::Serialize;

use crate::records::{save_entry_in_tx, PersonInput};
use crate::statement;
use crate::text::normalize;

fn s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Поля, которые можно искать и заменять: звание, причина смерти, родство,
/// фамилия. Имя и отчество — нет: у них современная форма из словаря, и
/// замена написания её бы рассогласовала.
pub const FIELDS: [&str; 4] = ["rank", "death_cause", "kinship", "surname"];

fn check_field(field: &str) -> Result<(), String> {
    if FIELDS.contains(&field) { Ok(()) } else { Err(format!("поле «{field}» заменять нельзя")) }
}

/// Написание и число записей с ним.
#[derive(Serialize, Debug, PartialEq)]
pub struct Spelling {
    pub value: String,
    pub entries: i64,
}

/// Написания поля, совпадающие с набранным без учёта регистра и «ё», — и
/// содержащие его (от трёх букв), ниже. Набранное как есть — первым.
pub fn spellings(conn: &Connection, field: &str, query: &str) -> Result<Vec<Spelling>, String> {
    check_field(field)?;
    let want = normalize(query);
    if want.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(&statement("replace_values")?).map_err(s)?;
    let rows = stmt.query_map(rusqlite::named_params! { ":field": field }, |r| Ok(Spelling { value: r.get(0)?, entries: r.get(1)? }))
        .map_err(s)?;
    let mut exact = Vec::new();
    let mut inside = Vec::new();
    for row in rows {
        let row = row.map_err(s)?;
        let key = normalize(&row.value);
        if key == want {
            exact.push(row);
        } else if want.chars().count() >= 3 && key.contains(&want) {
            inside.push(row);
        }
    }
    exact.sort_by(|a, b| (a.value.trim() != query.trim()).cmp(&(b.value.trim() != query.trim())).then(b.entries.cmp(&a.entries)));
    inside.sort_by(|a, b| b.entries.cmp(&a.entries).then(a.value.cmp(&b.value)));
    exact.extend(inside.into_iter().take(30));
    Ok(exact)
}

/// Запись, где стоит значение.
#[derive(Serialize, Debug)]
pub struct Hit {
    pub entry_id: i64,
    pub section: i64,
    pub year: Option<i64>,
    pub page: Option<String>,
    pub no: Option<i64>,
    pub role_code: String,
    pub who: String,
}

pub fn hits(conn: &Connection, field: &str, value: &str) -> Result<Vec<Hit>, String> {
    check_field(field)?;
    let mut stmt = conn.prepare(&statement("replace_hits")?).map_err(s)?;
    let rows = stmt.query_map(rusqlite::named_params! { ":field": field, ":value": value }, |r| Ok(Hit {
        entry_id: r.get(0)?, section: r.get(1)?, year: r.get(2)?, page: r.get(3)?, no: r.get(4)?,
        role_code: r.get(5)?, who: r.get(6)?,
    })).map_err(s)?;
    rows.collect::<Result<_, _>>().map_err(s)
}

fn slot<'a>(p: &'a mut PersonInput, field: &str) -> &'a mut Option<String> {
    match field {
        "rank" => &mut p.rank,
        "death_cause" => &mut p.death_cause,
        "kinship" => &mut p.kinship,
        _ => &mut p.surname,
    }
}

/// Заменить значение во всех записях одной транзакцией. Возвращает число
/// пересохранённых записей. Копию прихода делает команда окна до вызова.
pub fn replace(conn: &Connection, field: &str, old: &str, new: &str) -> Result<usize, String> {
    check_field(field)?;
    let (old, new) = (old.trim(), new.trim());
    if new.is_empty() {
        return Err("на что заменить — пусто".into());
    }
    if old == new {
        return Ok(0);
    }
    let mut ids: Vec<i64> = hits(conn, field, old)?.into_iter().map(|h| h.entry_id).collect();
    ids.dedup();
    let tx = conn.unchecked_transaction().map_err(s)?;
    let mut done = 0;
    for id in ids {
        let Some(mut entry) = crate::merge::entry_input(&tx, id)? else { continue };
        let mut changed = false;
        // Звание, причину и родство сохранение только прибавляет в частоты
        // (`forget_entry` снимает имена и фамилии), — прежнее значение
        // снимается здесь, иначе опечатка осталась бы в подсказке.
        let mut unbump: Vec<&'static str> = Vec::new();
        for p in &mut entry.persons {
            let kind = match field {
                "rank" if p.role_code.starts_with("clergy") => Some("rank_clergy"),
                "rank" if p.gender.as_deref() == Some("Ж") => Some("rank_f"),
                "rank" => Some("rank_m"),
                "death_cause" => Some("death_cause"),
                "kinship" => Some("kinship"),
                _ => None,
            };
            let v = slot(p, field);
            if v.as_deref().map(str::trim) == Some(old) {
                *v = Some(new.to_string());
                changed = true;
                unbump.extend(kind);
                // Фамилия в современном написании — та же, если совпадала и
                // новое написание современное (без «ъ» на конце).
                if field == "surname" && p.surname_modern.as_deref().map(str::trim) == Some(old)
                    && !new.ends_with(['ъ', 'Ъ']) {
                    p.surname_modern = Some(new.to_string());
                }
            }
        }
        if !changed {
            continue;
        }
        entry.id = Some(id);
        // Привязка скана остаётся как есть.
        entry.scan_file = None;
        let parish: String = tx
            .query_row(&statement("case_parish_key")?, rusqlite::named_params! { ":id": entry.case_id },
                       |r| r.get::<_, Option<String>>(0))
            .map_err(s)?.unwrap_or_default();
        save_entry_in_tx(&tx, &entry, &parish)?;
        for kind in unbump {
            for (scope, key) in [("case", entry.case_id.to_string()), ("parish", parish.clone()), ("global", String::new())] {
                tx.execute(&statement("usage_unbump")?, rusqlite::named_params! {
                    ":kind": kind, ":scope": scope, ":scope_key": key, ":value": old,
                }).map_err(s)?;
            }
            tx.execute(&statement("usage_drop_empty")?, rusqlite::named_params! { ":kind": kind, ":value": old }).map_err(s)?;
            tx.execute(&statement("lookup_drop_orphan")?, rusqlite::named_params! { ":kind": kind, ":value": old }).map_err(s)?;
        }
        done += 1;
    }
    tx.commit().map_err(s)?;
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::{save_case, save_entry, Case, EntryInput};
    use std::path::Path;

    /// Роман 12.11.2026: «головая боль» стоит в записи — найти и заменить на
    /// «головная боль»; память подсказок идёт за записью.
    #[test]
    fn typo_found_and_replaced() {
        let seed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../resources/seed.sqlite");
        if !seed.exists() {
            crate::seed_missing("тест замены в записях пропущен");
            return;
        }
        let path = std::env::temp_dir().join(format!("genmetric-replace-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::copy(&seed, &path).unwrap();
        let conn = Connection::open(&path).unwrap();
        save_case(&conn, &Case { id: 0, archive: None, fond: None, opis: None, delo: None, church: Some("Никольская".into()),
                                 village: Some("Тестово".into()), uyezd: None, guberniya: None, year: Some(1900), indexer: None }, false).unwrap();
        let death = |name: &str, cause: &str, surname: &str| -> EntryInput {
            serde_json::from_value(serde_json::json!({
                "id": null, "case_id": 1, "section": 3, "page": "5", "no_male": 1, "no_female": null,
                "event_day": 1, "event_month": 2, "event_year": 1900, "rite_day": 3, "rite_month": 2, "rite_year": 1900,
                "note": null, "uncertain": null,
                "persons": [{ "role_code": "deceased", "sort_order": 10, "surname": surname, "first_name": name, "patronymic": null,
                              "surname_modern": surname, "first_name_modern": name, "patronymic_modern": null, "maiden_surname": null,
                              "gender": "М", "rank": "крестьянин", "confession": null, "place": "Тестово", "note": null,
                              "uncertain": null, "age_text": "40", "age_years": 40, "death_cause": cause }]
            })).unwrap()
        };
        save_entry(&conn, &death("Иван", "головая боль", "попов")).unwrap();
        save_entry(&conn, &death("Пётр", "головная боль", "Попов")).unwrap();
        let found = spellings(&conn, "death_cause", "ГОЛОВАЯ боль").unwrap();
        assert_eq!(found, vec![Spelling { value: "головая боль".into(), entries: 1 }]);
        let both = spellings(&conn, "surname", "Попов").unwrap();
        assert_eq!(both.iter().map(|s| s.value.as_str()).collect::<Vec<_>>(), vec!["Попов", "попов"], "своё написание — первым");
        assert_eq!(hits(&conn, "death_cause", "головая боль").unwrap().len(), 1);

        assert_eq!(replace(&conn, "death_cause", "головая боль", "головная боль").unwrap(), 1);
        assert!(hits(&conn, "death_cause", "головая боль").unwrap().is_empty());
        assert_eq!(hits(&conn, "death_cause", "головная боль").unwrap().len(), 2);
        let used = |v: &str| -> i64 {
            conn.query_row("SELECT coalesce(max(count), 0) FROM usage_stat WHERE kind = 'death_cause' AND value = ?1", [v], |r| r.get(0)).unwrap()
        };
        assert_eq!((used("головая боль"), used("головная боль")), (0, 2), "память подсказок — за записями");
        assert_eq!(replace(&conn, "surname", "попов", "Попов").unwrap(), 1);
        assert_eq!(spellings(&conn, "surname", "попов").unwrap(), vec![Spelling { value: "Попов".into(), entries: 2 }]);
        assert!(replace(&conn, "first_name", "Иван", "Иоанн").is_err(), "имя не заменяется");
        std::mem::drop(conn);
        let _ = std::fs::remove_file(path);
    }
}
