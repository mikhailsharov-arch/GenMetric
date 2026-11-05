//! Дозаполнение фамилий жён по мужьям (З4.4, спека 2026-10-20-familii-zhyon.md).
//!
//! Роман 12.10.2026: «Я начинал индексировать часть метрик, не вбивая фамилии
//! женам… Нужен инструмент (скрипт) для проверки и массового дозаполнения
//! БД». С 06.10 форма сама даёт матери фамилию отца; инструмент — для
//! записей, набранных раньше или без фамилий. Только открытый приход.
//!
//! Пары: отец — мать (рождения); умерший — супруг умершего и умершая —
//! родственник «муж» (смерти; ответ 4.2). Невеста не трогается: у неё
//! девичья фамилия. Правится только пустая фамилия жены — женской формой
//! фамилии мужа, как записано. Другая фамилия жены, двойная фамилия мужа,
//! мужское имя у жены — «Проверьте вручную», без правки (ответы 4.1, 4.3).

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use crate::records::{is_no_name, remember, NO_NAME};
use crate::statement;
use crate::text::{normalize, normalize_name, normalize_words};

fn s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Пара «муж — жена» одной записи.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Pair {
    /// Упоминание жены — ключ для применения.
    pub wife_id: i64,
    pub entry_id: i64,
    /// 1 — рождение, 3 — смерть.
    pub section: i64,
    pub year: Option<i64>,
    pub page: Option<String>,
    pub husband: String,
    pub wife: String,
    /// Что станет фамилией жены (у «Проверьте вручную» — женская форма
    /// фамилии мужа, для сравнения).
    pub proposed: String,
    /// Почему на ручную проверку; пусто — правится.
    pub reason: String,
}

/// Предпросмотр: что правится, что — на ручную проверку, сколько пропущено.
#[derive(Serialize, Debug, Default)]
pub struct Plan {
    pub fill: Vec<Pair>,
    pub check: Vec<Pair>,
    /// Пары без фамилии мужа (пусто, «***») или без имени жены.
    pub skipped: usize,
}

/// Женская форма фамилии мужа как записано: «Сидоровъ» → «Сидорова»,
/// «Томский» → «Томская». Копия правила окна — `feminineSurname` в
/// src/names.ts (подстановка матери при наборе): правишь одно — правь другое.
pub fn feminine(surname: &str) -> String {
    let s = surname.trim();
    let s = s.strip_suffix('ъ').unwrap_or(s);
    let low = s.to_lowercase();
    let ends = |tails: &[&str]| tails.iter().any(|t| low.ends_with(t));
    let cut = |k: usize| -> String { s.chars().take(s.chars().count().saturating_sub(k)).collect() };
    if ends(&["ов", "ев", "ёв", "ин", "ын"]) {
        format!("{s}а")
    } else if ends(&["ский", "цкий", "ской", "цкой", "ый", "ой"]) {
        format!("{}ая", cut(2))
    } else {
        s.to_string()
    }
}

#[derive(Clone)]
struct M {
    id: i64,
    entry: i64,
    section: i64,
    year: Option<i64>,
    page: Option<String>,
    case_id: i64,
    role: String,
    first: Option<String>,
    patr: Option<String>,
    surname: Option<String>,
    surname_modern: Option<String>,
    gender: Option<String>,
    kinship: Option<String>,
    rank: Option<String>,
    place: Option<String>,
}

fn part(v: &Option<String>) -> Option<&str> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

fn iof(m: &M) -> String {
    [part(&m.first), part(&m.patr), part(&m.surname)].into_iter().flatten().collect::<Vec<_>>().join(" ")
}

/// Имя только мужское по словарю («Павел» записан матерью — ошибка набора).
fn male_name(conn: &Connection, first: &str) -> Result<bool, String> {
    // Дореформенное «Іоаннъ» — тот же ключ, что у словаря (ревьюер 05.11.2026).
    let key = normalize_name(first);
    let genders: Vec<String> = conn
        .prepare("SELECT DISTINCT gender FROM name_form WHERE form_norm = ?1 AND gender IS NOT NULL").map_err(s)?
        .query_map([key], |r| r.get(0)).map_err(s)?
        .collect::<Result<_, _>>().map_err(s)?;
    Ok(genders.iter().any(|g| g == "М") && !genders.iter().any(|g| g == "Ж"))
}

/// Супруг в записи о смерти: родство «муж», «жена», «супруг», «супруга».
fn spouse_kinship(k: &Option<String>) -> bool {
    matches!(part(k).map(normalize_name).as_deref(), Some("муж" | "жена" | "супруг" | "супруга"))
}

/// Все пары открытого прихода и сколько пар смертей не разобрать по полу.
fn pairs(conn: &Connection) -> Result<(Vec<(M, M)>, usize), String> {
    let mut stmt = conn.prepare(
        "SELECT m.id, e.id, e.section, coalesce(e.rite_year, e.event_year), e.page, e.case_id, m.role_code,
                m.first_name, m.patronymic, m.surname, m.surname_modern, m.gender, m.kinship, m.rank, p.name
           FROM person_mention m JOIN entry e ON e.id = m.entry_id LEFT JOIN place p ON p.id = m.place_id
          WHERE (e.section = 1 AND m.role_code IN ('father', 'mother'))
             OR (e.section = 3 AND m.role_code IN ('deceased', 'deceased_spouse', 'deceased_relative'))
          ORDER BY e.id, m.sort_order").map_err(s)?;
    let rows: Vec<M> = stmt.query_map([], |r| Ok(M {
        id: r.get(0)?, entry: r.get(1)?, section: r.get(2)?, year: r.get(3)?, page: r.get(4)?, case_id: r.get(5)?,
        role: r.get(6)?, first: r.get(7)?, patr: r.get(8)?, surname: r.get(9)?, surname_modern: r.get(10)?,
        gender: r.get(11)?, kinship: r.get(12)?, rank: r.get(13)?, place: r.get(14)?,
    })).map_err(s)?.collect::<Result<_, _>>().map_err(s)?;
    let mut out = Vec::new();
    let mut unclear = 0;
    let mut i = 0;
    while i < rows.len() {
        let entry = rows[i].entry;
        let mut j = i;
        while j < rows.len() && rows[j].entry == entry {
            j += 1;
        }
        let people = &rows[i..j];
        let find = |role: &str| people.iter().find(|m| m.role == role);
        if people[0].section == 1 {
            if let (Some(f), Some(m)) = (find("father"), find("mother")) {
                out.push((f.clone(), m.clone()));
            }
        } else if let Some(dead) = find("deceased") {
            let spouse = find("deceased_spouse")
                .or_else(|| people.iter().find(|m| m.role == "deceased_relative" && spouse_kinship(&m.kinship)));
            if let Some(sp) = spouse {
                // Кто муж — по полу; пол известен у одного — у другого обратный.
                // Пол противоречит («Ж» у обоих) или неизвестен — не разбираем.
                let g = |m: &M| part(&m.gender).map(str::to_string);
                match (g(dead).as_deref(), g(sp).as_deref()) {
                    (Some("М"), None | Some("Ж")) | (None, Some("Ж")) => out.push((dead.clone(), sp.clone())),
                    (Some("Ж"), None | Some("М")) | (None, Some("М")) => out.push((sp.clone(), dead.clone())),
                    _ => unclear += 1,
                }
            }
        }
        i = j;
    }
    Ok((out, unclear))
}

/// Что правится и что — на ручную проверку.
pub fn plan(conn: &Connection) -> Result<Plan, String> {
    let mut out = Plan::default();
    let (all, unclear) = pairs(conn)?;
    out.skipped += unclear;
    for (h, w) in all {
        let Some(hs) = part(&h.surname) else { out.skipped += 1; continue };
        if hs.contains('*') || is_no_name(hs) || part(&w.first).is_none_or(|f| f == NO_NAME || f.contains('*')) {
            out.skipped += 1;
            continue;
        }
        let proposed = feminine(hs);
        let pair = |reason: &str| Pair {
            wife_id: w.id, entry_id: w.entry, section: w.section, year: w.year, page: w.page.clone(),
            husband: iof(&h), wife: iof(&w), proposed: proposed.clone(), reason: reason.to_string(),
        };
        let wife_surname = part(&w.surname);
        if male_name(conn, part(&w.first).unwrap_or_default())? {
            out.check.push(pair("у жены мужское имя — возможно, ошибка набора"));
        } else if hs.contains(char::is_whitespace) {
            // «Силаев Зимин»: первое слово — краткое отчество (ответ 4.3).
            if wife_surname.is_none_or(|ws| normalize_words(ws) != normalize_words(&proposed)) {
                out.check.push(pair("у мужа двойная фамилия"));
            }
        } else if wife_surname.is_some_and(|ws| ws.contains('*') || is_no_name(ws)) {
            out.check.push(pair("фамилия жены не читается"));
        } else if let Some(ws) = wife_surname {
            let k = normalize_words(ws);
            if k != normalize_words(&proposed) && k != normalize_words(hs) {
                out.check.push(pair("у жены другая фамилия"));
            }
        } else {
            out.fill.push(pair(""));
        }
    }
    Ok(out)
}

/// Дописать фамилии отмеченным жёнам (`wife_ids` из `plan().fill`). План
/// считается заново: правится только то, что и сейчас пусто. Память
/// подсказок — как после сохранения записи: прежний ИОФ жены снимается,
/// новый запоминается; фамилия — в частоты; пара «муж — жена» — с новой
/// фамилией. Вызывающий держит копию базы. Возвращает, сколько исправлено.
pub fn apply(conn: &Connection, wife_ids: &[i64]) -> Result<usize, String> {
    let want: std::collections::HashSet<i64> = wife_ids.iter().copied().collect();
    let fill: Vec<i64> = plan(conn)?.fill.into_iter().map(|p| p.wife_id).filter(|id| want.contains(id)).collect();
    let (all, _) = pairs(conn)?;
    let tx = conn.unchecked_transaction().map_err(s)?;
    let mut n = 0;
    for id in fill {
        let Some((h, w)) = all.iter().find(|(_, w)| w.id == id) else { continue };
        let hs = part(&h.surname).unwrap_or_default();
        let surname = feminine(hs);
        let modern = part(&h.surname_modern).map(feminine);
        let changed = tx.execute(
            "UPDATE person_mention SET surname = ?2, surname_modern = coalesce(nullif(trim(surname_modern), ''), ?3)
              WHERE id = ?1 AND trim(coalesce(surname, '')) = ''",
            rusqlite::params![id, surname, modern]).map_err(s)?;
        if changed == 0 {
            continue;
        }
        // Дата изменения записи — в выгрузку (ревьюер 05.11.2026).
        tx.execute("UPDATE entry SET updated_at = datetime('now') WHERE id = ?1", [w.entry]).map_err(s)?;
        n += 1;
        let old = iof(w);
        let mut now = w.clone();
        now.surname = Some(surname.clone());
        let new = iof(&now);
        // Ключ прихода для частот — как у сохранения записи (`save_entry`).
        let parish: String = tx.query_row(&statement("case_parish_key")?, rusqlite::named_params! { ":id": w.case_id },
                                          |r| r.get::<_, Option<String>>(0))
            .optional().map_err(s)?.flatten().unwrap_or_default();
        remember(&tx, "surname", &surname, w.case_id, &parish)?;
        // Память персон — у всех, кроме умершего (как при сохранении).
        if w.role != "deceased" && !old.contains('*') {
            tx.execute(&statement("person_unremember")?, rusqlite::named_params! {
                ":iof": old, ":place": w.place, ":rank": w.rank,
            }).map_err(s)?;
            tx.execute(&statement("person_drop_empty")?, rusqlite::named_params! { ":iof": old }).map_err(s)?;
            tx.execute(&statement("person_remember")?, rusqlite::named_params! {
                ":iof": new, ":iof_norm": normalize(&new), ":place": w.place, ":rank": w.rank,
                ":gender": w.gender, ":name_surname": part(&w.patr).is_none(),
            }).map_err(s)?;
        }
        // Пара «муж — жена» — только отец и мать (так её пишет сохранение).
        if w.role == "mother" && part(&h.first) != Some(NO_NAME) {
            let husband_norm = normalize(&iof(h));
            let had: Option<(Option<String>, Option<String>)> = tx.query_row(
                "SELECT wife_place, wife_rank FROM spouse_index WHERE husband_norm = ?1 AND wife_iof = ?2",
                rusqlite::params![husband_norm, old], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(s)?;
            if had.is_some() {
                let pair = rusqlite::named_params! { ":husband_norm": husband_norm, ":wife_iof": old };
                tx.execute(&statement("spouse_unremember")?, pair).map_err(s)?;
                tx.execute(&statement("spouse_drop_empty")?, pair).map_err(s)?;
            }
            let (place, rank) = had.unwrap_or((w.place.clone(), w.rank.clone()));
            // Та же жена без фамилии из других записей (невеста в браке,
            // «Пелагея Андреева») — дополнить её строку, а не завести вторую:
            // как при сохранении записи (проверяющий 05.11.2026).
            tx.execute(&statement("spouse_extend")?, rusqlite::named_params! {
                ":husband_norm": husband_norm, ":wife_iof": new, ":wife_place": place.clone().or(w.place.clone()),
            }).map_err(s)?;
            tx.execute(&statement("spouse_remember")?, rusqlite::named_params! {
                ":husband_norm": husband_norm, ":wife_iof": new, ":wife_place": place.or(w.place.clone()),
                ":wife_rank": rank.or(w.rank.clone()),
            }).map_err(s)?;
        }
    }
    tx.commit().map_err(s)?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feminine_forms() {
        for (m, f) in [("Сидоров", "Сидорова"), ("Сидоровъ", "Сидорова"), ("Томский", "Томская"), ("Заболотовский", "Заболотовская"),
                       ("Толстой", "Толстая"), ("Белый", "Белая"), ("Шевченко", "Шевченко"), ("Черных", "Черных"),
                       ("Ильин", "Ильина"), ("Пугачёв", "Пугачёва")] {
            assert_eq!(feminine(m), f, "{m}");
        }
    }

    fn base() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../../db/schema.sql")).unwrap();
        conn.execute_batch(
            "INSERT INTO role (code, title, section, sort_order) VALUES
               ('father', 'Отец', 1, 1), ('mother', 'Мать', 1, 2), ('deceased', 'Умерший', 3, 1),
               ('deceased_spouse', 'Супруг', 3, 2), ('deceased_relative', 'Родственник', 3, 3),
               ('bride', 'Невеста', 2, 2), ('groom', 'Жених', 2, 1);
             INSERT INTO name_dict (id, gender, name, name_norm) VALUES (1, 'М', 'Павел', 'павел'), (2, 'Ж', 'Мария', 'мария');
             INSERT INTO name_form (name_id, form, form_norm, kind, gender) VALUES (1, 'Павел', 'павел', 'name', 'М'), (2, 'Мария', 'мария', 'name', 'Ж');
             INSERT INTO mk_case (id, year) VALUES (1, 1890);").unwrap();
        conn
    }

    fn entry(conn: &Connection, section: i64, people: &[(&str, &str, &str, &str, &str, &str)]) -> i64 {
        conn.execute("INSERT INTO entry (case_id, section, rite_year, page) VALUES (1, ?1, 1890, '5')", [section]).unwrap();
        let e = conn.last_insert_rowid();
        for (k, (role, first, patr, sur, gender, kin)) in people.iter().enumerate() {
            let v = |x: &str| (!x.is_empty()).then(|| x.to_string());
            conn.execute("INSERT INTO person_mention (entry_id, role_code, sort_order, first_name, patronymic, surname, gender, kinship)
                          VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                         rusqlite::params![e, role, k as i64, v(first), v(patr), v(sur), v(gender), v(kin)]).unwrap();
        }
        e
    }

    #[test]
    fn plan_and_apply() {
        let conn = base();
        entry(&conn, 1, &[("father", "Иван", "Петров", "Сидоров", "М", ""), ("mother", "Мария", "Ивановна", "", "Ж", "")]);
        entry(&conn, 1, &[("father", "Иван", "", "Дудин", "М", ""), ("mother", "Пелагея", "", "Киреева Дудина", "Ж", "")]);
        entry(&conn, 1, &[("father", "Петр", "", "Силаев Зимин", "М", ""), ("mother", "Агриппина", "", "", "Ж", "")]);
        entry(&conn, 1, &[("father", "Илья", "", "Ермаков", "М", ""), ("mother", "Павел", "Ильин", "Томилин", "", "")]);
        entry(&conn, 1, &[("father", "Иван", "", "***", "М", ""), ("mother", "Анна", "", "", "Ж", "")]);
        entry(&conn, 1, &[("father", "Иван", "", "Заболотовский", "М", ""), ("mother", "Анна", "", "Заболотовская", "Ж", "")]);
        // Смерти: умерший и супруга; умершая и родственник «муж».
        entry(&conn, 3, &[("deceased", "Яков", "", "Аверин", "М", ""), ("deceased_spouse", "Анна", "", "", "Ж", "")]);
        entry(&conn, 3, &[("deceased", "Евдокия", "", "", "Ж", ""), ("deceased_relative", "Яков", "", "Томский", "М", "супруг")]);
        // Невеста не трогается.
        entry(&conn, 2, &[("groom", "Иван", "", "Сидоров", "М", ""), ("bride", "Мария", "", "", "Ж", "")]);
        conn.execute("INSERT INTO person_index (iof, iof_norm, place, rank, uses) VALUES ('Мария Ивановна', 'мария ивановна', '', '', 1)", []).unwrap();
        conn.execute("INSERT INTO spouse_index (husband_norm, wife_iof, uses) VALUES ('иван петров сидоров', 'Мария Ивановна', 2)", []).unwrap();

        let p = plan(&conn).unwrap();
        let fills: Vec<(&str, &str)> = p.fill.iter().map(|x| (x.wife.as_str(), x.proposed.as_str())).collect();
        assert_eq!(fills, vec![("Мария Ивановна", "Сидорова"), ("Анна", "Аверина"), ("Евдокия", "Томская")]);
        let checks: Vec<&str> = p.check.iter().map(|x| x.reason.as_str()).collect();
        assert_eq!(checks, vec!["у жены другая фамилия", "у мужа двойная фамилия", "у жены мужское имя — возможно, ошибка набора"]);
        assert_eq!(p.skipped, 1, "фамилия мужа «***»");

        let ids: Vec<i64> = p.fill.iter().map(|x| x.wife_id).collect();
        assert_eq!(apply(&conn, &ids).unwrap(), 3);
        let num = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(num("SELECT count(*) FROM person_mention WHERE surname IN ('Сидорова', 'Аверина', 'Томская')"), 3);
        assert_eq!(num("SELECT count(*) FROM person_mention WHERE role_code = 'bride' AND surname IS NULL"), 1, "невеста не тронута");
        assert_eq!(num("SELECT count(*) FROM person_mention WHERE surname = 'Киреева Дудина'"), 1, "другая — не правится");
        assert_eq!(num("SELECT count(*) FROM person_index WHERE iof = 'Мария Ивановна'"), 0, "прежний ИОФ снят");
        assert_eq!(num("SELECT count(*) FROM person_index WHERE iof = 'Мария Ивановна Сидорова'"), 1);
        assert_eq!(num("SELECT count(*) FROM spouse_index WHERE wife_iof = 'Мария Ивановна Сидорова'"), 1);
        assert_eq!(num("SELECT count(*) FROM spouse_index WHERE wife_iof = 'Мария Ивановна'"), 0,
                   "строка той же жены из другой записи (невеста) дополнена, второй жены у мужа нет");
        assert_eq!(num("SELECT count(*) FROM spouse_index WHERE husband_norm = 'иван петров сидоров'"), 1);
        assert_eq!(num("SELECT count(*) FROM person_index WHERE iof = 'Евдокия Томская'"), 0, "умершая — не в памяти персон");
        assert!(num("SELECT count(*) FROM usage_stat WHERE kind = 'surname' AND value = 'Сидорова'") >= 1);
        assert!(plan(&conn).unwrap().fill.is_empty(), "повторный запуск ничего не предлагает");
    }

    /// На настоящей базе: `GENMETRIC_WIVES_DB=/путь/копия.sqlite cargo test -p
    /// genmetric-core wives_real -- --ignored --nocapture` — что предложит окно.
    #[test]
    #[ignore]
    fn wives_real() {
        let Ok(path) = std::env::var("GENMETRIC_WIVES_DB") else { return };
        let conn = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let p = plan(&conn).unwrap();
        eprintln!("правится: {}, вручную: {}, пропущено: {}", p.fill.len(), p.check.len(), p.skipped);
        for x in p.fill.iter().take(5) {
            eprintln!("  + {} — {} → {}", x.husband, x.wife, x.proposed);
        }
        for x in &p.check {
            eprintln!("  ? {} — {} ({})", x.husband, x.wife, x.reason);
        }
    }
}
