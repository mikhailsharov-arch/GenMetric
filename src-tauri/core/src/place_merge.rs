//! Объединение населённых пунктов (спека 2026-10-20-obedinenie-punktov.md;
//! Роман 12.10.2026: «Глиница» и «Глинница», «Воспица» и «Воспицы» — один
//! пункт; с 03.11.2026 ещё «Шишкино» и «с.Шишкино»). Кнопка — в карточке
//! пункта. Человек выбирает, какой пункт остаётся; записи, память подсказок
//! и село дел второго переходят к нему, второй уходит из справочника.
//!
//! Справочник пунктов общий для всех приходов: объединение записывается в
//! `place_merged`, сверка (`parish_sync.sql`) разносит запись по общему файлу
//! и приходам, а `apply_merges` при открытии другого прихода переводит и его
//! записи. Прежнее название ложится и в `place_renamed` — по нему сверка и
//! обновление не возвращают ушедший пункт.

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use crate::statement;
use crate::text::{normalize, place_clean, strip_place_type};

fn s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Карточка пункта для объединения.
#[derive(Debug, Clone)]
struct Card {
    id: i64,
    name: String,
    norm: String,
    np_type: String,
    guberniya: String,
    uyezd: String,
    volost: String,
    familio_url: String,
    comment: String,
}

fn card(conn: &Connection, id: i64) -> Result<Card, String> {
    conn.query_row(&statement("place_merge_card")?, rusqlite::named_params! { ":id": id }, |r| {
        Ok(Card {
            id: r.get(0)?, name: r.get(1)?, norm: r.get(2)?, np_type: r.get(3)?, guberniya: r.get(4)?,
            uyezd: r.get(5)?, volost: r.get(6)?, familio_url: r.get(7)?, comment: r.get(8)?,
        })
    })
    .optional()
    .map_err(s)?
    .ok_or_else(|| format!("Населённого пункта с id {id} нет — возможно, его уже объединили"))
}

/// Сколько записей прихода стоит у пункта с этим названием.
pub fn uses(conn: &Connection, norm: &str) -> Result<i64, String> {
    conn.query_row(&statement("place_merge_uses")?, rusqlite::named_params! { ":name_norm": norm }, |r| r.get(0))
        .map_err(s)
}

/// Итог объединения — для окна.
#[derive(Serialize, Debug, Default)]
pub struct Merged {
    /// Оставшийся пункт и ушедший — названия, как в поле НП.
    pub kept: String,
    pub dropped: String,
    /// Записи, перешедшие к оставшемуся.
    pub entries: i64,
    /// Дела, у которых село было ушедшим пунктом.
    pub cases: i64,
    /// Поля, пустые у оставшегося и взятые у ушедшего: «волость: Котельская».
    pub filled: Vec<String>,
    /// Поля, заполненные у обоих по-разному: остаётся своё, чужое — в отчёт.
    pub differs: Vec<String>,
}

const FIELDS: [&str; 5] = ["тип", "губерния", "уезд", "волость", "ссылка Familio"];

fn fields(c: &Card) -> [&str; 5] {
    [&c.np_type, &c.guberniya, &c.uyezd, &c.volost, &c.familio_url]
}

/// Объединить пункт `drop_id` с пунктом `keep_id`: остаётся `keep_id`. Одной
/// транзакцией; копии файлов делает вызывающий до неё.
pub fn merge(conn: &Connection, keep_id: i64, drop_id: i64) -> Result<Merged, String> {
    let keep = card(conn, keep_id)?;
    let gone = card(conn, drop_id)?;
    if keep.norm == gone.norm {
        return Err(format!("«{}» и «{}» — одно и то же название: объединять нечего", keep.name, gone.name));
    }
    let mut out = Merged { kept: keep.name.clone(), dropped: gone.name.clone(), ..Merged::default() };
    let mut filled = keep.clone();
    {
        let slots: [&mut String; 5] = [&mut filled.np_type, &mut filled.guberniya, &mut filled.uyezd,
                                       &mut filled.volost, &mut filled.familio_url];
        for ((slot, theirs), label) in slots.into_iter().zip(fields(&gone)).zip(FIELDS) {
            let (mine, theirs) = (slot.trim().to_string(), theirs.trim());
            if theirs.is_empty() || mine == theirs {
                continue;
            }
            if mine.is_empty() {
                *slot = theirs.to_string();
                out.filled.push(format!("{label}: {theirs}"));
            } else {
                out.differs.push(format!("{label}: осталось «{mine}», у «{}» было «{theirs}»", gone.name));
            }
        }
    }
    // Комментарий тёзки — часть названия, а название остаётся у оставшегося:
    // молча не теряем (проверяющий 03.11.2026).
    if !gone.comment.trim().is_empty() && gone.comment.trim() != keep.comment.trim() {
        out.differs.push(format!("комментарий «{}» у «{}» не перешёл: название — у оставшегося",
                                 gone.comment.trim(), gone.name));
    }
    out.entries = uses(conn, &gone.norm)?;
    let tx = conn.unchecked_transaction().map_err(s)?;
    if !out.filled.is_empty() {
        let clean = place_clean(&keep.name, &keep.comment).to_string();
        let blank = |v: &str| (!v.trim().is_empty()).then(|| v.trim().to_string());
        tx.execute(&statement("place_update")?, rusqlite::named_params! {
            ":id": keep.id, ":name": keep.name, ":name_norm": keep.norm,
            ":np_type": blank(&filled.np_type), ":guberniya": blank(&filled.guberniya),
            ":uyezd": blank(&filled.uyezd), ":volost": blank(&filled.volost),
            ":familio_url": blank(&filled.familio_url),
            ":clean": strip_place_type(&clean), ":comment": blank(&keep.comment),
        })
        .map_err(|e| {
            if e.to_string().contains("UNIQUE") {
                format!("У «{}» есть двойник с теми же типом, уездом и губернией — объединение не сделано", keep.name)
            } else {
                e.to_string()
            }
        })?;
    }
    out.cases = repoint(&tx, &gone.norm, &keep)?;
    tx.execute(&statement("place_merged_chain")?, rusqlite::named_params! {
        ":old_norm": gone.norm, ":into_norm": keep.norm,
    }).map_err(s)?;
    tx.execute(&statement("place_merged_alive")?, rusqlite::named_params! { ":into_norm": keep.norm })
        .map_err(s)?;
    tx.execute(&statement("place_merged_remember")?, rusqlite::named_params! {
        ":old_norm": gone.norm, ":into_norm": keep.norm,
    }).map_err(s)?;
    tx.execute(&statement("place_renamed_remember")?, rusqlite::named_params! { ":old_norm": gone.norm })
        .map_err(s)?;
    tx.commit().map_err(s)?;
    Ok(out)
}

/// Перевести всё, что ссылается на пункт `old_norm`, к пункту `keep` и убрать
/// `old_norm` из справочника. Без своей транзакции — её держит вызывающий.
/// Возвращает число дел, у которых поменялось село.
fn repoint(conn: &Connection, old_norm: &str, keep: &Card) -> Result<i64, String> {
    let names: Vec<String> = {
        let mut st = conn.prepare(&statement("place_merge_old_names")?).map_err(s)?;
        let rows = st.query_map(rusqlite::named_params! { ":old_norm": old_norm }, |r| r.get(0)).map_err(s)?;
        rows.collect::<Result<_, _>>().map_err(s)?
    };
    conn.execute(&statement("place_merge_mentions")?, rusqlite::named_params! { ":keep_id": keep.id, ":old_norm": old_norm })
        .map_err(s)?;
    conn.execute(&statement("place_merge_persons")?, rusqlite::named_params! { ":keep_id": keep.id, ":old_norm": old_norm })
        .map_err(s)?;
    // Память подсказок — те же шаги, что у переименования (main.rs,
    // place_update): совпавшие строки складываются.
    for old_name in &names {
        // Каждый блок — литералом: проверка инцидентов сверяет параметры
        // вызова с блоком только так (ревьюер 25.09.2026).
        conn.execute(&statement("place_rename_persons_merge")?, rusqlite::named_params! {
            ":name": keep.name, ":old_name": old_name,
        }).map_err(s)?;
        conn.execute(&statement("place_rename_persons_drop")?, rusqlite::named_params! {
            ":name": keep.name, ":old_name": old_name,
        }).map_err(s)?;
        conn.execute(&statement("place_rename_usage_merge")?, rusqlite::named_params! {
            ":name": keep.name, ":old_name": old_name,
        }).map_err(s)?;
        conn.execute(&statement("place_rename_usage_drop")?, rusqlite::named_params! {
            ":name": keep.name, ":old_name": old_name,
        }).map_err(s)?;
        conn.execute(&statement("place_rename_persons")?, rusqlite::named_params! {
            ":name": keep.name, ":old_name": old_name,
        }).map_err(s)?;
        conn.execute(&statement("place_rename_spouses")?, rusqlite::named_params! {
            ":name": keep.name, ":old_name": old_name,
        }).map_err(s)?;
        conn.execute(&statement("place_rename_usage")?, rusqlite::named_params! {
            ":name": keep.name, ":name_norm": keep.norm, ":old_name": old_name,
        }).map_err(s)?;
    }
    // Село дела — текстом; ключ прихода собран из села (records.rs,
    // Case::parish_key), и частоты «по приходу» идут за ним.
    let cases: Vec<(i64, String, String, String, String, String)> = {
        let mut st = conn.prepare(&statement("place_merge_cases")?).map_err(s)?;
        let rows = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))
            .map_err(s)?;
        rows.collect::<Result<_, _>>().map_err(s)?
    };
    let mut moved = 0;
    for (id, village, _church, _uyezd, _guberniya, old_key) in cases {
        if normalize(&village) != old_norm {
            continue;
        }
        // В ключе меняется только часть села: церковь у дел разных лет своя
        // (п. 139), а ключ прихода общий — собирать его из полей этого дела
        // нельзя. Ключ, где на месте села другое (импорт), не трогаем.
        let mut parts: Vec<String> = old_key.split('|').map(str::to_string).collect();
        let key = if parts.len() == 4 && normalize(&parts[1]) == old_norm {
            parts[1] = keep.name.clone();
            parts.join("|")
        } else {
            old_key.clone()
        };
        conn.execute(&statement("place_merge_case_set")?, rusqlite::named_params! {
            ":id": id, ":village": keep.name, ":parish_key": key,
        }).map_err(s)?;
        if key != old_key {
            crate::records::move_parish_stats(conn, &old_key, &key)?;
        }
        moved += 1;
    }
    conn.execute(&statement("place_merge_drop")?, rusqlite::named_params! { ":old_norm": old_norm })
        .map_err(s)?;
    Ok(moved)
}

/// Объединения, сделанные в других приходах: записи этого прихода с ушедшим
/// пунктом переходят к оставшемуся (ответ Романа 5.3 — во всех приходах).
/// Вызывается сверкой после `parish_sync.sql`, внутри её транзакции.
/// Возвращает число переведённых пунктов.
pub fn apply_merges(conn: &Connection) -> Result<usize, String> {
    let pending: Vec<(String, String)> = {
        let mut st = conn.prepare(&statement("place_merged_pending")?).map_err(s)?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(s)?;
        rows.collect::<Result<_, _>>().map_err(s)?
    };
    let villages: std::collections::HashSet<String> = {
        let mut st = conn.prepare(&statement("place_merge_cases")?).map_err(s)?;
        let rows = st.query_map([], |r| r.get::<_, String>(1)).map_err(s)?;
        rows.map(|v| v.map(|v| normalize(&v))).collect::<Result<_, _>>().map_err(s)?
    };
    let mut done = 0;
    for (old_norm, into) in pending {
        let has_row: bool = conn
            .query_row(&statement("place_find")?, rusqlite::named_params! { ":name_norm": old_norm }, |_| Ok(()))
            .optional()
            .map_err(s)?
            .is_some();
        if !has_row && !villages.contains(&old_norm) {
            continue;
        }
        // Цепочка «А → Б → В»: идём до живого пункта, не дальше десяти шагов
        // (защита от кольца из двух приходов).
        let mut target = into;
        let mut keep_id: Option<i64> = None;
        for _ in 0..10 {
            if target == old_norm {
                break;
            }
            keep_id = conn
                .query_row(&statement("place_find")?, rusqlite::named_params! { ":name_norm": target }, |r| r.get(0))
                .optional()
                .map_err(s)?;
            if keep_id.is_some() {
                break;
            }
            match conn
                .query_row(&statement("place_merged_into")?, rusqlite::named_params! { ":old_norm": target }, |r| r.get(0))
                .optional()
                .map_err(s)?
            {
                Some(next) => target = next,
                None => break,
            }
        }
        // Оставшегося пункта нет ни здесь, ни в общем файле — переводить
        // некуда; записи остаются при своём пункте.
        let Some(keep_id) = keep_id else { continue };
        let keep = card(conn, keep_id)?;
        repoint(conn, &old_norm, &keep)?;
        done += 1;
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../../db/schema.sql")).unwrap();
        // Ссылки проверяются, как в программе: пункт убирается только после
        // того, как на него не ссылается ни одна запись.
        conn.execute_batch("INSERT INTO role (code, title, section, sort_order) VALUES ('father', 'Отец', 1, 1);
                            INSERT INTO mk_case (id, village) VALUES (1, NULL);").unwrap();
        conn
    }

    fn place(conn: &Connection, name: &str, uyezd: Option<&str>, volost: Option<&str>) -> i64 {
        conn.execute("INSERT INTO place (name, name_norm, np_type, uyezd, volost) VALUES (?1, ?2, 'д.', ?3, ?4)",
                     rusqlite::params![name, normalize(name), uyezd, volost]).unwrap();
        conn.last_insert_rowid()
    }

    fn mention(conn: &Connection, place_id: i64) {
        conn.execute("INSERT INTO entry (case_id, section) VALUES (1, 1)", []).unwrap();
        let entry = conn.last_insert_rowid();
        conn.execute("INSERT INTO person_mention (entry_id, role_code, sort_order, place_id) VALUES (?1, 'father', 1, ?2)",
                     rusqlite::params![entry, place_id]).unwrap();
    }

    fn num(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn merge_moves_everything() {
        let conn = base();
        conn.execute("INSERT OR REPLACE INTO mk_case (id, village, church, uyezd, guberniya, parish_key)
                      VALUES (1, 'Глинница', 'Троицкая', 'Макарьевский', 'Костромская',
                              'Троицкая|Глинница|Макарьевский|Костромская')", []).unwrap();
        let keep = place(&conn, "Глиница", Some("Макарьевский"), None);
        let gone = place(&conn, "Глинница", Some("Кинешемский"), Some("Котельская"));
        mention(&conn, keep);
        mention(&conn, gone);
        mention(&conn, gone);
        conn.execute_batch("
            INSERT INTO person_index (iof, iof_norm, place, rank, uses) VALUES ('Пётр Сидоров', 'петр сидоров', 'Глиница', NULL, 2);
            INSERT INTO person_index (iof, iof_norm, place, rank, uses) VALUES ('Пётр Сидоров', 'петр сидоров', 'Глинница', NULL, 3);
            INSERT INTO spouse_index (husband_norm, wife_iof, wife_place) VALUES ('петр сидоров', 'Анна', 'Глинница');
            INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count)
                 VALUES ('place', 'global', '', 'Глинница', 'глинница', 4);
            INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count)
                 VALUES ('surname', 'parish', 'Троицкая|Глинница|Макарьевский|Костромская', 'Сидоров', 'сидоров', 5);
        ").unwrap();

        let got = merge(&conn, keep, gone).unwrap();
        assert_eq!((got.kept.as_str(), got.dropped.as_str(), got.entries, got.cases), ("Глиница", "Глинница", 2, 1));
        assert_eq!(got.filled, vec!["волость: Котельская".to_string()]);
        assert_eq!(got.differs.len(), 1, "{:?}", got.differs);
        assert!(got.differs[0].contains("Кинешемский"));

        assert_eq!(num(&conn, "SELECT count(*) FROM place"), 1);
        assert_eq!(num(&conn, &format!("SELECT count(*) FROM person_mention WHERE place_id = {keep}")), 3);
        assert_eq!(num(&conn, "SELECT uses FROM person_index WHERE place = 'Глиница'"), 5);
        assert_eq!(num(&conn, "SELECT count(*) FROM person_index WHERE place = 'Глинница'"), 0);
        assert_eq!(num(&conn, "SELECT count(*) FROM spouse_index WHERE wife_place = 'Глиница'"), 1);
        assert_eq!(num(&conn, "SELECT count(*) FROM usage_stat WHERE kind = 'place' AND value = 'Глиница'"), 1);
        let (village, key): (String, String) = conn
            .query_row("SELECT village, parish_key FROM mk_case WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(village, "Глиница");
        assert_eq!(key, "Троицкая|Глиница|Макарьевский|Костромская");
        assert_eq!(num(&conn, "SELECT count(*) FROM usage_stat WHERE scope = 'parish' AND scope_key LIKE '%|Глиница|%'"), 1);
        assert_eq!(num(&conn, "SELECT count(*) FROM place_merged WHERE old_norm = 'глинница' AND into_norm = 'глиница'"), 1);
        assert_eq!(num(&conn, "SELECT count(*) FROM place_renamed WHERE old_norm = 'глинница'"), 1);
        assert_eq!(num(&conn, "SELECT volost = 'Котельская' AND full_location LIKE '%Котельская волость%' FROM place"), 1);

        // Набранное название ушедшего — подсказка оставшегося (place_check в main.rs).
        let hint: String = conn.query_row(&statement("place_merged_target").unwrap(),
                                          rusqlite::named_params! { ":name_norm": "глинница" }, |r| r.get(0)).unwrap();
        assert_eq!(hint, "Глиница");

        // Тот же пункт с собой не объединяется; ушедшего больше нет.
        assert!(merge(&conn, keep, keep).is_err());
        assert!(merge(&conn, keep, gone).is_err());
    }

    #[test]
    fn prefix_twin_and_chain() {
        // Двойник «Шишкино» / «с.Шишкино» (с 03.11.2026 название хранится с
        // приставкой, как набрано).
        let conn = base();
        let a = place(&conn, "Шишкино", None, None);
        let b = place(&conn, "с.Шишкино", Some("Макарьевский"), None);
        mention(&conn, a);
        let got = merge(&conn, b, a).unwrap();
        assert_eq!((got.kept.as_str(), got.entries), ("с.Шишкино", 1));
        assert!(got.filled.is_empty() && got.differs.is_empty(), "{got:?}");

        // «Шишкино → с.Шишкино», затем «с.Шишкино → Шишкино-2»: старое
        // объединение идёт дальше, к живому пункту.
        let c = place(&conn, "Шишкино-2", None, None);
        merge(&conn, c, b).unwrap();
        assert_eq!(num(&conn, "SELECT count(*) FROM place_merged WHERE into_norm = 'шишкино-2'"), 2);
        assert_eq!(num(&conn, &format!("SELECT count(*) FROM person_mention WHERE place_id = {c}")), 1);
    }

    #[test]
    fn other_parish_follows() {
        // Второй приход: у него свой «Глинница», объединение пришло сверкой.
        let conn = base();
        let keep = place(&conn, "Глиница", None, None);
        let gone = place(&conn, "Глинница", None, None);
        mention(&conn, gone);
        conn.execute_batch("INSERT INTO place_merged (old_norm, into_norm) VALUES ('глинница', 'глиница');").unwrap();
        assert_eq!(apply_merges(&conn).unwrap(), 1);
        assert_eq!(num(&conn, &format!("SELECT count(*) FROM person_mention WHERE place_id = {keep}")), 1);
        assert_eq!(num(&conn, "SELECT count(*) FROM place WHERE name_norm = 'глинница'"), 0);
        // Повторный прогон ничего не делает.
        assert_eq!(apply_merges(&conn).unwrap(), 0);

        // Село дела без строки пункта в этом приходе — тоже переводится
        // (ревьюер 03.11.2026).
        conn.execute_batch("UPDATE mk_case SET village = 'Глинница', parish_key = '|Глинница||' WHERE id = 1;").unwrap();
        assert_eq!(apply_merges(&conn).unwrap(), 1);
        assert_eq!(num(&conn, "SELECT count(*) FROM mk_case WHERE village = 'Глиница' AND parish_key = '|Глиница||'"), 1);
        assert_eq!(apply_merges(&conn).unwrap(), 0);

        // Оставшегося нет нигде — записи остаются при своём пункте.
        let lone = place(&conn, "Воспицы", None, None);
        mention(&conn, lone);
        conn.execute_batch("INSERT INTO place_merged (old_norm, into_norm) VALUES ('воспицы', 'воспица');").unwrap();
        assert_eq!(apply_merges(&conn).unwrap(), 0);
        assert_eq!(num(&conn, "SELECT count(*) FROM place WHERE name_norm = 'воспицы'"), 1);
    }
}
