//! Очистка неиспользуемых значений справочников (З3.4, спека
//! 2026-11-04-ochistka-spravochnikov-i-dolg.md).
//!
//! Роман 20.10.2026: опечатки («Павлоа», «Владимир Ремов») «намертво оседают
//! в справочниках … При этом в самой базе (в реальных записях) их больше
//! нет». С 20.10 правка записи сама снимает свой вклад в память
//! (`records::forget_entry`); здесь — старый мусор, по кнопке.
//!
//! Занятым считается значение, которое стоит хоть в одной записи, деле или
//! карточке пункта открытого прихода — по тому же правилу, каким сохранение
//! пополняет память, и по ключу без регистра и дореформенных букв
//! (`normalize_words`: «крестьянинъ» занимает «крестьянин»). Память из архива
//! Excel записей не имеет по определению: строки, которых не касались после
//! загрузки архива, не предлагаются. Перечни общие для всех приходов —
//! значение перечня предлагается, только если оно не занято и в других.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Serialize;

use crate::records::NO_NAME;
use crate::text::normalize_words;

fn s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Значение, которое можно убрать.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Unused {
    /// Ключ для удаления: что это и где.
    pub key: String,
    /// Группа в окне: «Имена», «Причт»…
    pub group: &'static str,
    /// Как написано.
    pub value: String,
    /// Подробности: перечень, НП и звание персоны, жена.
    pub detail: String,
    /// Сколько раз подсказка его помнит.
    pub uses: i64,
}

/// Группа окна и группа «занятости» для вида значения. None — вид, который
/// очистка не трогает.
fn kind_groups(kind: &str) -> Option<(&'static str, &'static str, &'static str)> {
    // (группа окна, группа занятости, название перечня для подробностей)
    Some(match kind {
        "first_name" => ("Имена", "first_name", "имя"),
        "patronymic" => ("Отчества", "patronymic", "отчество"),
        "surname" => ("Фамилии", "surname", "фамилия"),
        "rank_m" => ("Звания", "rank", "звание мужчины"),
        "rank_f" => ("Звания", "rank", "звание женщины"),
        "rank_clergy" => ("Звания", "rank", "звание причта"),
        "place" => ("Населённые пункты", "place", "населённый пункт"),
        "archive" => ("Прочие перечни", "archive", "архив"),
        "church" => ("Прочие перечни", "church", "церковь"),
        "uyezd" => ("Прочие перечни", "uyezd", "уезд"),
        "guberniya" => ("Прочие перечни", "guberniya", "губерния"),
        "volost" => ("Прочие перечни", "volost", "волость"),
        "np_type" => ("Прочие перечни", "np_type", "тип пункта"),
        "confession" => ("Прочие перечни", "confession", "вероисповедание"),
        "kinship" => ("Прочие перечни", "kinship", "родство"),
        "marriage_order" => ("Прочие перечни", "marriage_order", "каким браком"),
        "death_cause" => ("Прочие перечни", "death_cause", "причина смерти"),
        _ => return None,
    })
}

/// Что занято в приходе: значения по группам и память людей.
#[derive(Default)]
struct Used {
    values: HashMap<&'static str, HashSet<String>>,
    /// ИОФ → (НП, звание) его упоминаний.
    persons: HashMap<String, HashSet<(String, String)>>,
    clergy: HashSet<(String, String)>,
    spouses: HashSet<(String, String)>,
}

impl Used {
    fn add(&mut self, group: &'static str, v: Option<&str>) {
        if let Some(v) = v.map(str::trim).filter(|v| !v.is_empty()) {
            self.values.entry(group).or_default().insert(normalize_words(v));
        }
    }
    fn has(&self, group: &str, v: &str) -> bool {
        self.values.get(group).is_some_and(|set| set.contains(&normalize_words(v)))
    }
    /// Персона памяти занята: тот же ИОФ, НП и звание; пустое НП или звание
    /// памяти — любое (родственникам жениха и невесты НП проставило позже
    /// обновление, а память осталась без него — это тот же человек).
    fn has_person(&self, who: &str, place: &str, rank: &str) -> bool {
        let (place, rank) = (normalize_words(place), normalize_words(rank));
        self.persons.get(&normalize_words(who)).is_some_and(|seen| seen.iter().any(|(p, r)|
            (place.is_empty() || *p == place) && (rank.is_empty() || *r == rank)))
    }
}

fn iof(first: &Option<String>, patr: &Option<String>, surname: &Option<String>) -> String {
    [first, patr, surname].iter()
        .filter_map(|v| v.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .collect::<Vec<_>>().join(" ")
}

/// Занятое в приходе `conn` — тем же правилом, каким сохранение пополняет
/// память (обратное — `records::forget_entry`).
fn used(conn: &Connection) -> Result<Used, String> {
    let mut u = Used::default();
    struct M { entry: i64, role: String, first: Option<String>, patr: Option<String>, surname: Option<String>,
               rank: Option<String>, place: Option<String> }
    let mut by_entry: HashMap<i64, Vec<M>> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT m.entry_id, m.role_code, m.first_name, m.patronymic, m.surname, m.rank, p.name,
                    m.confession, m.kinship, m.marriage_order, m.death_cause
               FROM person_mention m LEFT JOIN place p ON p.id = m.place_id").map_err(s)?;
        let mut rows = stmt.query([]).map_err(s)?;
        while let Some(r) = rows.next().map_err(s)? {
            let m = M { entry: r.get(0).map_err(s)?, role: r.get(1).map_err(s)?, first: r.get(2).map_err(s)?,
                        patr: r.get(3).map_err(s)?, surname: r.get(4).map_err(s)?, rank: r.get(5).map_err(s)?,
                        place: r.get(6).map_err(s)? };
            u.add("first_name", m.first.as_deref());
            u.add("patronymic", m.patr.as_deref());
            u.add("surname", m.surname.as_deref());
            u.add("rank", m.rank.as_deref());
            u.add("place", m.place.as_deref());
            for (k, g) in [(7, "confession"), (8, "kinship"), (9, "marriage_order"), (10, "death_cause")] {
                let v: Option<String> = r.get(k).map_err(s)?;
                u.add(g, v.as_deref());
            }
            by_entry.entry(m.entry).or_default().push(m);
        }
    }
    let rank = |m: &M| normalize_words(m.rank.as_deref().unwrap_or(""));
    for people in by_entry.values() {
        for m in people {
            let who = iof(&m.first, &m.patr, &m.surname);
            if who.is_empty() || m.first.as_deref().map(str::trim) == Some(NO_NAME) {
                continue;
            }
            if m.role.starts_with("clergy") {
                u.clergy.insert((normalize_words(&who), rank(m)));
            }
            // Причт тоже: импорт из Excel клал его и в память персон
            // («Александр Рождественский, священник» — 1922 раза у Романа).
            u.persons.entry(normalize_words(&who)).or_default()
                .insert((normalize_words(m.place.as_deref().unwrap_or("")), rank(m)));
        }
        for (h, w) in [("father", "mother"), ("groom", "bride")] {
            let (Some(h), Some(w)) = (people.iter().find(|m| m.role == h), people.iter().find(|m| m.role == w)) else { continue };
            u.spouses.insert((normalize_words(&iof(&h.first, &h.patr, &h.surname)),
                              normalize_words(&iof(&w.first, &w.patr, &w.surname))));
        }
    }
    {
        let mut stmt = conn.prepare("SELECT archive, church, village, uyezd, guberniya FROM mk_case").map_err(s)?;
        let mut rows = stmt.query([]).map_err(s)?;
        while let Some(r) = rows.next().map_err(s)? {
            for (k, g) in [(0, "archive"), (1, "church"), (2, "place"), (3, "uyezd"), (4, "guberniya")] {
                let v: Option<String> = r.get(k).map_err(s)?;
                u.add(g, v.as_deref());
            }
        }
    }
    {
        // Карточки пунктов не трогаем: их название и поля — занятые.
        let mut stmt = conn.prepare("SELECT name, np_type, uyezd, guberniya, volost FROM place").map_err(s)?;
        let mut rows = stmt.query([]).map_err(s)?;
        while let Some(r) = rows.next().map_err(s)? {
            for (k, g) in [(0, "place"), (1, "np_type"), (2, "uyezd"), (3, "guberniya"), (4, "volost")] {
                let v: Option<String> = r.get(k).map_err(s)?;
                u.add(g, v.as_deref());
            }
        }
    }
    Ok(u)
}

/// Время сборки загруженного в приход архива подсказок: строки памяти, не
/// тронутые с тех пор, — из архива.
fn archive_stamp(conn: &Connection) -> Result<Option<String>, String> {
    let v: Option<String> = conn
        .query_row("SELECT value FROM main.setting WHERE key = 'archive_loaded'", [], |r| r.get(0))
        .optional().map_err(s)?;
    let Some(v) = v else { return Ok(None) };
    // Время неизвестно — бережём всю память.
    let Some(at) = v.rsplit_once(" @ ").map(|(_, at)| at.trim()).filter(|at| *at != "?") else {
        return Ok(Some("9999".to_string()));
    };
    // Запас в сутки: архив пишет время сборки до строк памяти, и у строк оно
    // бывает на секунды позже (db/tools/build_archive.py, datetime('now') в
    // каждом запросе). Тронутое человеком в первые сутки — тоже бережём.
    let later: Option<String> = conn.query_row("SELECT datetime(?1, '+1 day')", [at], |r| r.get(0)).map_err(s)?;
    Ok(Some(later.unwrap_or_else(|| "9999".to_string())))
}

/// Внутреннее: что именно уйдёт по ключу.
enum Part {
    /// Значения частот (все написания с этим ключом) и строки перечня.
    Value { kind: String, spellings: Vec<String>, lookup: Vec<(i64, String)> },
    Person(i64),
    Clergy(i64),
    Spouse(i64),
}

/// Ключ значения в окне — одинаковый между поиском и удалением.
fn value_key(kind: &str, norm: &str) -> String {
    format!("v\u{1f}{kind}\u{1f}{norm}")
}

/// Неиспользуемое в открытом приходе. `others` — файлы других приходов:
/// значение перечня, занятое там, не предлагается. None (другие приходы
/// неизвестны: нет общего файла, файл прихода пропал) или другой приход не
/// прочитался — перечни не предлагаются вовсе (бережём; ревьюер 04.11.2026).
pub fn find(conn: &Connection, others: Option<&[PathBuf]>) -> Result<Vec<Unused>, String> {
    Ok(scan(conn, others)?.into_iter().map(|(u, _)| u).collect())
}

fn scan(conn: &Connection, others: Option<&[PathBuf]>) -> Result<Vec<(Unused, Part)>, String> {
    let own = used(conn)?;
    let mut elsewhere: Option<Vec<Used>> = others.map(|_| Vec::new());
    for path in others.unwrap_or_default() {
        let read = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .map_err(s)
            .and_then(|c| used(&c));
        match (read, elsewhere.as_mut()) {
            (Ok(u), Some(list)) => list.push(u),
            _ => elsewhere = None,
        }
    }
    let stamp = archive_stamp(conn)?;
    let from_archive = |last: &Option<String>| match (&stamp, last) {
        (None, _) => false,
        (Some(_), None) => true,
        (Some(at), Some(last)) => last.as_str() <= at.as_str(),
    };

    // Значения: частоты и перечни, по (вид, ключ).
    struct V { value: String, uses: i64, spellings: Vec<String>, lookup: Vec<(i64, String)>, archive: bool }
    let mut values: Vec<((String, String), V)> = Vec::new();
    let mut at: HashMap<(String, String), usize> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT kind, value, max(count), max(last_used_at),
                    coalesce(sum(CASE WHEN scope = 'global' THEN count END), 0),
                    coalesce(sum(CASE WHEN scope = 'parish' THEN count END), 0)
               FROM usage_stat GROUP BY kind, value").map_err(s)?;
        let mut rows = stmt.query([]).map_err(s)?;
        while let Some(r) = rows.next().map_err(s)? {
            let (kind, value, uses, last, global, parish): (String, String, i64, Option<String>, i64, i64) =
                (r.get(0).map_err(s)?, r.get(1).map_err(s)?, r.get(2).map_err(s)?, r.get(3).map_err(s)?,
                 r.get(4).map_err(s)?, r.get(5).map_err(s)?);
            let Some((_, group, _)) = kind_groups(&kind) else { continue };
            if own.has(group, &value) {
                continue;
            }
            let key = (kind.clone(), normalize_words(&value));
            let i = *at.entry(key.clone()).or_insert_with(|| {
                values.push((key, V { value: value.clone(), uses: 0, spellings: Vec::new(), lookup: Vec::new(), archive: false }));
                values.len() - 1
            });
            let v = &mut values[i].1;
            v.uses += uses.max(0);
            v.spellings.push(value);
            // Архив пишет только охват «вся база»: его там больше, чем у
            // прихода, — значит, это вклад Excel, даже если строку потом
            // трогал набор и время у неё новое (ревьюер 04.11.2026: «Ефим» ×49).
            v.archive |= from_archive(&last) || (stamp.is_some() && global > parish);
        }
    }
    if let Some(elsewhere) = &elsewhere {
        let mut stmt = conn.prepare("SELECT id, kind, value, value_norm FROM main.lookup WHERE origin = 'user'").map_err(s)?;
        let mut rows = stmt.query([]).map_err(s)?;
        while let Some(r) = rows.next().map_err(s)? {
            let (id, kind, value, norm): (i64, String, String, String) =
                (r.get(0).map_err(s)?, r.get(1).map_err(s)?, r.get(2).map_err(s)?, r.get(3).map_err(s)?);
            let Some((_, group, _)) = kind_groups(&kind) else { continue };
            if own.has(group, &value) || elsewhere.iter().any(|u| u.has(group, &value)) {
                continue;
            }
            let key = (kind.clone(), normalize_words(&value));
            let i = *at.entry(key.clone()).or_insert_with(|| {
                values.push((key, V { value: value.clone(), uses: 0, spellings: Vec::new(), lookup: Vec::new(), archive: false }));
                values.len() - 1
            });
            values[i].1.lookup.push((id, norm));
        }
    }
    let mut out = Vec::new();
    for ((kind, norm), v) in values {
        if v.archive {
            continue;
        }
        let (group, _, title) = kind_groups(&kind).expect("вид проверен выше");
        let detail = if group == "Прочие перечни" || group == "Звания" { title.to_string() } else { String::new() };
        out.push((Unused { key: value_key(&kind, &norm), group, value: v.value, detail, uses: v.uses },
                  Part::Value { kind, spellings: v.spellings, lookup: v.lookup }));
    }

    // Память людей.
    {
        let mut stmt = conn.prepare("SELECT id, iof, place, rank, uses, last_used_at FROM person_index").map_err(s)?;
        let mut rows = stmt.query([]).map_err(s)?;
        while let Some(r) = rows.next().map_err(s)? {
            let (id, who, place, rank, uses, last): (i64, String, Option<String>, Option<String>, i64, Option<String>) =
                (r.get(0).map_err(s)?, r.get(1).map_err(s)?, r.get(2).map_err(s)?, r.get(3).map_err(s)?,
                 r.get(4).map_err(s)?, r.get(5).map_err(s)?);
            let (place, rank) = (place.unwrap_or_default(), rank.unwrap_or_default());
            // Вклад архива в память людей от набора не отделить: в приходе с
            // архивом — только строки, которые помнят один раз.
            if from_archive(&last) || (stamp.is_some() && uses > 1) || own.has_person(&who, &place, &rank) {
                continue;
            }
            let detail = [place.trim(), rank.trim()].iter().filter(|v| !v.is_empty()).copied().collect::<Vec<_>>().join(", ");
            out.push((Unused { key: format!("p\u{1f}{id}"), group: "Персоны в подсказке", value: who, detail, uses },
                      Part::Person(id)));
        }
    }
    {
        let mut stmt = conn.prepare("SELECT id, iof, rank, uses, last_used_at FROM clergy_index").map_err(s)?;
        let mut rows = stmt.query([]).map_err(s)?;
        while let Some(r) = rows.next().map_err(s)? {
            let (id, who, rank, uses, last): (i64, String, Option<String>, i64, Option<String>) =
                (r.get(0).map_err(s)?, r.get(1).map_err(s)?, r.get(2).map_err(s)?, r.get(3).map_err(s)?, r.get(4).map_err(s)?);
            let rank = rank.unwrap_or_default();
            if from_archive(&last) || (stamp.is_some() && uses > 1)
                || own.clergy.contains(&(normalize_words(&who), normalize_words(&rank))) {
                continue;
            }
            out.push((Unused { key: format!("c\u{1f}{id}"), group: "Причт", value: who, detail: rank.trim().to_string(), uses },
                      Part::Clergy(id)));
        }
    }
    {
        let mut stmt = conn.prepare("SELECT id, husband_norm, wife_iof, uses, last_used_at FROM spouse_index").map_err(s)?;
        let mut rows = stmt.query([]).map_err(s)?;
        while let Some(r) = rows.next().map_err(s)? {
            let (id, husband, wife, uses, last): (i64, String, String, i64, Option<String>) =
                (r.get(0).map_err(s)?, r.get(1).map_err(s)?, r.get(2).map_err(s)?, r.get(3).map_err(s)?, r.get(4).map_err(s)?);
            if from_archive(&last) || (stamp.is_some() && uses > 1)
                || own.spouses.contains(&(normalize_words(&husband), normalize_words(&wife))) {
                continue;
            }
            // Муж хранится ключом поиска (строчными) — с заглавных для показа.
            let husband_shown = husband.split_whitespace().map(|w| {
                let mut c = w.chars();
                c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
            }).collect::<Vec<_>>().join(" ");
            out.push((Unused { key: format!("s\u{1f}{id}"), group: "Пары «муж — жена»", value: husband_shown,
                               detail: format!("жена {wife}"), uses },
                      Part::Spouse(id)));
        }
    }
    Ok(out)
}

/// Убрать выбранное (`keys` из `find`). Список считается заново: уходит
/// только то, что и сейчас не занято. Перечни — через `lookup_dropped`:
/// сверка с общим файлом (вызывает тот, кто зовёт) уберёт их и оттуда.
/// Значение перечня уходит и из перечней других приходов: иначе их сверка
/// вернула бы его в общий файл, а оттуда — сюда (ревьюер 04.11.2026). Там
/// оно не занято — это проверено при поиске. Возвращает, сколько значений
/// убрано, и что не удалось в других приходах.
pub fn remove(conn: &Connection, keys: &[String], others: Option<&[PathBuf]>) -> Result<(usize, Vec<String>), String> {
    let wanted: HashSet<&str> = keys.iter().map(String::as_str).collect();
    let now = scan(conn, others)?;
    let tx = conn.unchecked_transaction().map_err(s)?;
    let mut n = 0;
    let mut dropped: Vec<(String, String)> = Vec::new();
    for (u, part) in now {
        if !wanted.contains(u.key.as_str()) {
            continue;
        }
        match part {
            Part::Value { kind, spellings, lookup } => {
                for v in &spellings {
                    tx.execute("DELETE FROM usage_stat WHERE kind = ?1 AND value = ?2", rusqlite::params![kind, v]).map_err(s)?;
                }
                for (id, norm) in &lookup {
                    tx.execute("INSERT OR IGNORE INTO main.lookup_dropped (kind, value_norm) VALUES (?1, ?2)",
                               rusqlite::params![kind, norm]).map_err(s)?;
                    tx.execute("DELETE FROM main.lookup WHERE id = ?1", [id]).map_err(s)?;
                    dropped.push((kind.clone(), norm.clone()));
                }
            }
            Part::Person(id) => { tx.execute("DELETE FROM person_index WHERE id = ?1", [id]).map_err(s)?; }
            Part::Clergy(id) => { tx.execute("DELETE FROM clergy_index WHERE id = ?1", [id]).map_err(s)?; }
            Part::Spouse(id) => { tx.execute("DELETE FROM spouse_index WHERE id = ?1", [id]).map_err(s)?; }
        }
        n += 1;
    }
    tx.commit().map_err(s)?;
    let mut problems = Vec::new();
    if !dropped.is_empty() {
        for path in others.unwrap_or_default() {
            let done = Connection::open(path).map_err(s).and_then(|o| {
                for (kind, norm) in &dropped {
                    o.execute("DELETE FROM lookup WHERE kind = ?1 AND value_norm = ?2 AND origin = 'user'",
                              rusqlite::params![kind, norm]).map_err(s)?;
                }
                Ok(())
            });
            if let Err(e) = done {
                problems.push(format!("{}: {e}", path.display()));
            }
        }
    }
    Ok((n, problems))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::normalize;

    fn base() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../../db/schema.sql")).unwrap();
        conn.execute_batch(
            "INSERT INTO role (code, title, section, sort_order) VALUES
               ('father', 'Отец', 1, 1), ('mother', 'Мать', 1, 2), ('clergy_1', 'Священник', 0, 3);
             INSERT INTO mk_case (id, village) VALUES (1, 'Бухарино');
             INSERT INTO entry (id, case_id, section) VALUES (1, 1, 1);
             INSERT INTO person_mention (entry_id, role_code, sort_order, first_name, patronymic, surname, rank)
               VALUES (1, 'father', 1, 'Павел', 'Иванов', 'Сидоров', 'крестьянинъ'),
                      (1, 'mother', 2, 'Мария', 'Петрова', NULL, NULL),
                      (1, 'clergy_1', 3, 'Владимир', NULL, 'Реморов', 'священник');").unwrap();
        conn
    }

    fn usage(conn: &Connection, kind: &str, value: &str, last: &str) {
        for (scope, key) in [("case", "1"), ("parish", "п"), ("global", "")] {
            conn.execute("INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count, last_used_at)
                          VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
                         rusqlite::params![kind, scope, key, value, normalize(value), last]).unwrap();
        }
    }

    fn num(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn typos_go_used_stays() {
        let conn = base();
        usage(&conn, "first_name", "Павел", "2026-10-01 10:00:00");
        usage(&conn, "first_name", "Павлоа", "2026-10-01 10:00:00");
        usage(&conn, "rank_m", "крестьянин", "2026-10-01 10:00:00");
        usage(&conn, "rank_m", "мещанн", "2026-10-01 10:00:00");
        conn.execute_batch(
            "INSERT INTO lookup (kind, value, value_norm, origin) VALUES ('rank_m', 'мещанн', 'мещанн', 'user'),
                                                                   ('rank_m', 'крестьянин', 'крестьянин', 'user');
             INSERT INTO person_index (iof, iof_norm, place, rank, uses, last_used_at) VALUES
               ('Павел Иванов Сидоров', 'павел иванов сидоров', '', 'крестьянин', 1, '2026-10-01'),
               ('Павлоа Иванов Сидоров', 'павлоа иванов сидоров', '', 'крестьянин', 1, '2026-10-01'),
               ('Павел Иванов Сидоров', 'павел иванов сидоров', '', '', 1, '2026-10-01'),
               ('Павел Иванов Сидоров', 'павел иванов сидоров', 'Бухарино', 'крестьянин', 1, '2026-10-01');
             INSERT INTO clergy_index (iof, iof_norm, rank, uses, last_used_at) VALUES
               ('Владимир Реморов', 'владимир реморов', 'священник', 3, '2026-10-01'),
               ('Владимир Ремов', 'владимир ремов', 'священник', 1, '2026-10-01');
             INSERT INTO spouse_index (husband_norm, wife_iof, uses, last_used_at) VALUES
               ('павел иванов сидоров', 'Мария Петрова', 1, '2026-10-01'),
               ('павлоа иванов сидоров', 'Мария Петрова', 1, '2026-10-01');").unwrap();
        let found = find(&conn, Some(&[])).unwrap();
        let shown: Vec<&str> = found.iter().map(|u| u.value.as_str()).collect();
        assert!(shown.contains(&"Павлоа") && shown.contains(&"мещанн") && shown.contains(&"Владимир Ремов")
                && shown.contains(&"Павлоа Иванов Сидоров") && shown.contains(&"Павлоа Иванов Сидоров"), "{shown:?}");
        for used in ["Павел", "крестьянин", "Владимир Реморов"] {
            assert!(!shown.contains(&used), "занятое «{used}» предложено: {shown:?}");
        }
        // «Павел Иванов Сидоров»: без НП — занят (любое НП), с «Бухарино» — нет
        // (в записи он без НП).
        let pavel: Vec<&str> = found.iter().filter(|u| u.value == "Павел Иванов Сидоров").map(|u| u.detail.as_str()).collect();
        assert_eq!(pavel, vec!["Бухарино, крестьянин"]);
        assert_eq!(found.iter().filter(|u| u.group == "Пары «муж — жена»").count(), 1);
        let keys: Vec<String> = found.iter().map(|u| u.key.clone()).collect();
        assert_eq!(remove(&conn, &keys, Some(&[])).unwrap().0, found.len());
        assert_eq!(num(&conn, "SELECT count(*) FROM usage_stat WHERE value IN ('Павлоа', 'мещанн')"), 0);
        assert_eq!(num(&conn, "SELECT count(*) FROM usage_stat WHERE value IN ('Павел', 'крестьянин')"), 6);
        assert_eq!(num(&conn, "SELECT count(*) FROM lookup WHERE value = 'мещанн'"), 0);
        assert_eq!(num(&conn, "SELECT count(*) FROM lookup_dropped WHERE value_norm = 'мещанн'"), 1, "сверка уберёт из общего");
        assert_eq!(num(&conn, "SELECT count(*) FROM person_index"), 2, "пустое звание памяти — любое; другое НП — не он");
        assert_eq!(num(&conn, "SELECT count(*) FROM clergy_index"), 1);
        assert_eq!(num(&conn, "SELECT count(*) FROM spouse_index"), 1);
        assert!(find(&conn, Some(&[])).unwrap().is_empty());
    }

    #[test]
    fn dropped_lookup_leaves_other_parishes() {
        // Ревьюер 04.11.2026: значение, оставшееся в перечне другого прихода,
        // его сверка вернула бы в общий файл, а оттуда — сюда.
        let conn = base();
        conn.execute("INSERT INTO lookup (kind, value, value_norm, origin) VALUES ('church', 'Никольскя', 'никольскя', 'user')", []).unwrap();
        let other = std::env::temp_dir().join(format!("genmetric-cleanup-drop-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&other);
        {
            let o = Connection::open(&other).unwrap();
            o.execute_batch(include_str!("../../../db/schema.sql")).unwrap();
            o.execute("INSERT INTO lookup (kind, value, value_norm, origin) VALUES ('church', 'Никольскя', 'никольскя', 'user')", []).unwrap();
        }
        let others = vec![other.clone()];
        let keys: Vec<String> = find(&conn, Some(&others)).unwrap().into_iter()
            .filter(|u| u.value == "Никольскя").map(|u| u.key).collect();
        assert_eq!(keys.len(), 1);
        let (n, problems) = remove(&conn, &keys, Some(&others)).unwrap();
        assert_eq!((n, problems.len()), (1, 0));
        let left: i64 = Connection::open(&other).unwrap()
            .query_row("SELECT count(*) FROM lookup WHERE value = 'Никольскя'", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0, "и из перечня другого прихода");
        let _ = std::fs::remove_file(&other);
    }

    #[test]
    fn archive_memory_and_other_parish_stay() {
        let conn = base();
        conn.execute("INSERT INTO setting (key, value) VALUES ('archive_loaded', 'Индексатор.xlsm @ 2026-09-13 12:20:08')", []).unwrap();
        usage(&conn, "surname", "Скворцов", "2026-09-13 09:48:50");
        usage(&conn, "surname", "Скворцоф", "2026-10-02 08:00:00");
        usage(&conn, "surname", "Скворцовъ", "2026-09-13 12:20:11");
        conn.execute_batch(
            "INSERT INTO person_index (iof, iof_norm, place, rank, uses, last_used_at) VALUES
               ('Иван Михайлов Скворцов', 'иван михайлов скворцов', 'Борисоглебское', 'диакон', 12, '2026-09-13 09:48:50'),
               ('Иван Михайлов Скворцов', 'иван михайлов скворцов', 'Борисоглебское', 'дьякон', 1, NULL);
             INSERT INTO lookup (kind, value, value_norm, origin) VALUES ('death_cause', 'от оспы', 'от оспы', 'user');").unwrap();
        // Строка архива, тронутая набором после загрузки (время новое), —
        // по-прежнему архив: в охвате «вся база» её больше, чем у прихода.
        conn.execute_batch(
            "INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count, last_used_at) VALUES
               ('first_name', 'global', '', 'Ефим', 'ефим', 49, '2026-09-28 10:00:00'),
               ('first_name', 'parish', 'п', 'Ефим', 'ефим', 1, '2026-09-28 10:00:00');
             INSERT INTO person_index (iof, iof_norm, place, rank, uses, last_used_at) VALUES
               ('Ефим Иванов', 'ефим иванов', 'Голодаево', 'крестьянин', 17, '2026-09-28 10:00:00'),
               ('Ефимм Иванов', 'ефимм иванов', 'Голодаево', 'крестьянин', 1, '2026-09-28 10:00:00');").unwrap();
        // Другой приход: там «от оспы» стоит в записи.
        let other = std::env::temp_dir().join(format!("genmetric-cleanup-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&other);
        {
            let o = Connection::open(&other).unwrap();
            o.execute_batch(include_str!("../../../db/schema.sql")).unwrap();
            o.execute_batch("INSERT INTO role (code, title, section, sort_order) VALUES ('deceased', 'Умерший', 3, 1);
                             INSERT INTO mk_case (id) VALUES (1); INSERT INTO entry (id, case_id, section) VALUES (1, 1, 3);
                             INSERT INTO person_mention (entry_id, role_code, sort_order, first_name, death_cause)
                               VALUES (1, 'deceased', 1, 'Анна', 'от оспы');").unwrap();
        }
        let shown: Vec<String> = find(&conn, Some(&[other.clone()])).unwrap().into_iter().map(|u| u.value).collect();
        let mut shown = shown;
        shown.sort();
        assert_eq!(shown, vec!["Ефимм Иванов".to_string(), "Скворцоф".to_string()],
                   "архив (и тронутый позже), занятое в другом приходе — не предлагаются");
        // Другой приход не прочитался — перечни не предлагаются вовсе.
        let missing = std::env::temp_dir().join("genmetric-cleanup-нет-такого.sqlite");
        conn.execute("INSERT INTO lookup (kind, value, value_norm, origin) VALUES ('church', 'Никольская', 'никольская', 'user')", []).unwrap();
        assert!(find(&conn, Some(&[])).unwrap().iter().any(|u| u.value == "Никольская"));
        assert!(!find(&conn, Some(&[missing])).unwrap().iter().any(|u| u.value == "Никольская"));
        assert!(!find(&conn, None).unwrap().iter().any(|u| u.value == "Никольская"), "другие приходы неизвестны");
        let _ = std::fs::remove_file(&other);
    }
}

#[cfg(test)]
mod real {
    /// На настоящей базе: `GENMETRIC_CLEANUP_DB=/путь/копия.sqlite cargo test -p
    /// genmetric-core cleanup_real -- --ignored --nocapture` — что предложила бы очистка.
    #[test]
    #[ignore]
    fn cleanup_real() {
        let Ok(path) = std::env::var("GENMETRIC_CLEANUP_DB") else { return };
        let conn = rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        // Другие приходы — через запятую в GENMETRIC_CLEANUP_OTHERS.
        let others: Vec<std::path::PathBuf> = std::env::var("GENMETRIC_CLEANUP_OTHERS").unwrap_or_default()
            .split(',').filter(|p| !p.is_empty()).map(std::path::PathBuf::from).collect();
        let found = super::find(&conn, Some(&others)).unwrap();
        let mut groups: std::collections::BTreeMap<&str, Vec<String>> = Default::default();
        for u in &found {
            groups.entry(u.group).or_default().push(format!("{} [{}] ×{}", u.value, u.detail, u.uses));
        }
        for (g, items) in groups {
            eprintln!("{g}: {}", items.len());
            for i in items.iter().take(12) {
                eprintln!("    {i}");
            }
        }
    }
}
