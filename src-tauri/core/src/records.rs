//! Запись метрической книги: разбор ИОФ и сохранение со всеми персонами.
//!
//! Живёт в крейте без окна, чтобы импорт из Excel шёл **тем же путём**, что и
//! набор руками: тот же разбор имени, те же пополнение справочников и память
//! персон (спека 2026-10-02, п. 4.3). Программа оборачивает это в команды.

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::statement;
use crate::text::{normalize, normalize_name, normalize_words};

#[derive(Serialize)]
pub struct ParsedIof {
    pub first_name: Option<String>,
    pub first_name_modern: Option<String>,
    pub patronymic: Option<String>,
    pub patronymic_modern: Option<String>,
    pub surname: Option<String>,
    pub gender: Option<String>,
    pub father_name: Option<String>,
    pub known_name: bool,
    /// Имя опознано по соответствию, заведённому человеком в окне сверки
    /// («Пискарь» → «Кесарь»): форма подставит целевое имя в поле и допишет
    /// в примечание «Имя в документе: Пискарь» — как при первом решении.
    pub name_alias: Option<String>,
    /// То же для отчества.
    pub patr_alias: Option<String>,
    /// Второе слово не опознано как отчество, но похоже на него по окончанию
    /// и за ним есть ещё слово — форма предложит сверить как отчество.
    pub patr_unknown: Option<String>,
}

/// Похоже ли слово на отчество по окончанию — чтобы не сверять как отчество
/// фамилию, стоящую второй в записи без отчества.
pub fn looks_like_patronymic(word: &str) -> bool {
    let w = normalize_name(word);
    ["ов", "ев", "ин", "ова", "ева", "ина", "ич", "на", "ых", "их"]
        .iter().any(|e| w.ends_with(e))
}

pub fn parse_iof_in(conn: &Connection, text: &str) -> Result<ParsedIof, String> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let mut out = ParsedIof {
        first_name: None,
        first_name_modern: None,
        patronymic: None,
        patronymic_modern: None,
        surname: None,
        gender: None,
        father_name: None,
        known_name: false,
        name_alias: None,
        patr_alias: None,
        patr_unknown: None,
    };
    if tokens.is_empty() {
        return Ok(out);
    }

    let alias_find = statement("alias_find")?;
    let alias = |kind: &str, word: &str| -> Result<Option<(Option<String>, Option<String>)>, String> {
        conn.query_row(&alias_find,
                       rusqlite::named_params! { ":kind": kind, ":form_norm": normalize_name(word) },
                       |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
            .map_err(|e| e.to_string())
    };

    out.first_name = Some(tokens[0].to_string());
    // Сначала соответствие, заведённое человеком: оно сильнее словаря.
    // Целевое имя ищется в словаре как обычное — с полом и основой.
    let mut lookup_word = tokens[0].to_string();
    match alias("name", tokens[0])? {
        Some((Some(target), _)) => {
            out.name_alias = Some(target.clone());
            lookup_word = target;
        }
        Some((None, gender)) => {
            // «Новое имя»: словарь его не знает, но человек сказал, что оно есть.
            out.known_name = true;
            out.gender = gender;
            // Современное — без конечного «ъ»: «Жданъ» → «Ждан» (ревьюер 23.09.2026).
            out.first_name_modern = Some(tokens[0].trim_end_matches('ъ').to_string());
        }
        None => {}
    }
    if !out.known_name {
        // priority 0 — заголовочное написание, 1 — вариант: точное совпадение
        // с самостоятельным именем важнее совпадения с вариантом другого.
        let head: Option<(String, Option<String>, Option<String>)> = conn
            .query_row(
                "SELECT d.name, d.base_name, d.gender
                   FROM name_form f JOIN name_dict d ON d.id = f.name_id
                  WHERE f.kind IN ('name','variant') AND f.form_norm = ?1
                  ORDER BY f.priority LIMIT 1",
                [normalize_name(&lookup_word)],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some((name, base, gender)) = head {
            out.known_name = true;
            out.gender = gender;
            out.first_name_modern = Some(base.unwrap_or(name));
        }
    }

    let mut rest = &tokens[1..];
    if let Some(first_rest) = rest.first() {
        let mut patr_word = first_rest.to_string();
        // Соответствие отчества без цели — «Это не отчество»: человек сказал,
        // что второе слово — часть фамилии; больше не спрашивать (техдолг,
        // ревьюер 23.09 и проверяющий 24.09.2026).
        let mut not_patr = false;
        match alias("patr", first_rest)? {
            Some((Some(target), _)) => {
                out.patr_alias = Some(target.clone());
                patr_word = target;
            }
            Some((None, _)) => not_patr = true,
            None => {}
        }
        let patr: Option<(String, String, String)> = conn
            .query_row(
                "SELECT d.name,
                        CASE WHEN f.kind LIKE '%_m' THEN d.patr_m ELSE d.patr_f END,
                        CASE WHEN f.kind LIKE '%_m' THEN 'М' ELSE 'Ж' END
                   FROM name_form f JOIN name_dict d ON d.id = f.name_id
                  WHERE f.kind LIKE 'patr%' AND f.form_norm = ?1
                  ORDER BY f.priority LIMIT 1",
                [normalize_name(&patr_word)],
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
        } else if !not_patr && looks_like_patronymic(first_rest) {
            // «Иван Пискарев Сидоров» и «Иван Пискарев» без фамилии: второе
            // слово не отчество по словарю, но стоит на месте отчества и
            // выглядит как оно — форма спросит. Без фамилии тоже (Роман
            // 25.09.2026: «считает, что это фамилия»); если это правда
            // фамилия — «Это не отчество» запомнит ответ.
            out.patr_unknown = Some(first_rest.to_string());
        }
    }
    if !rest.is_empty() {
        out.surname = Some(rest.join(" "));
    }
    Ok(out)
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Case {
    pub id: i64,
    pub archive: Option<String>,
    pub fond: Option<String>,
    pub opis: Option<String>,
    pub delo: Option<String>,
    pub church: Option<String>,
    pub village: Option<String>,
    pub uyezd: Option<String>,
    pub guberniya: Option<String>,
    pub year: Option<i64>,
    pub indexer: Option<String>,
}

impl Case {
    /// Ключ прихода связывает годы одного прихода: по нему переносится
    /// накопленная статистика подсказок. Индексируют именно приходами,
    /// год за годом, поэтому на второй год подсказки уже почти всегда попадают.
    pub fn parish_key(&self) -> String {
        let part = |v: &Option<String>| v.clone().unwrap_or_default();
        format!("{}|{}|{}|{}", part(&self.church), part(&self.village),
                part(&self.uyezd), part(&self.guberniya))
    }
}

#[derive(Deserialize)]
pub struct PersonInput {
    pub role_code: String,
    pub sort_order: i64,
    pub surname: Option<String>,
    pub first_name: Option<String>,
    pub patronymic: Option<String>,
    pub surname_modern: Option<String>,
    pub first_name_modern: Option<String>,
    pub patronymic_modern: Option<String>,
    pub maiden_surname: Option<String>,
    pub gender: Option<String>,
    pub rank: Option<String>,
    pub confession: Option<String>,
    pub place: Option<String>,
    pub note: Option<String>,
    pub uncertain: Option<String>,
    // Браки (25.09.2026). У рождений не приходят — serde отдаёт None.
    #[serde(default)]
    pub age_years: Option<i64>,
    #[serde(default)]
    pub marriage_order: Option<String>,
    #[serde(default)]
    pub kinship: Option<String>,
    // Смерти (27.09.2026): возраст как в книге и разобранный, причина смерти.
    #[serde(default)]
    pub age_months: Option<i64>,
    #[serde(default)]
    pub age_weeks: Option<i64>,
    #[serde(default)]
    pub age_days: Option<i64>,
    #[serde(default)]
    pub age_text: Option<String>,
    #[serde(default)]
    pub death_cause: Option<String>,
}

#[derive(Deserialize)]
pub struct EntryInput {
    pub id: Option<i64>,
    pub case_id: i64,
    pub section: i64,
    pub page: Option<String>,
    pub no_male: Option<i64>,
    pub no_female: Option<i64>,
    pub event_day: Option<i64>,
    pub event_month: Option<i64>,
    pub event_year: Option<i64>,
    pub rite_day: Option<i64>,
    pub rite_month: Option<i64>,
    pub rite_year: Option<i64>,
    pub note: Option<String>,
    pub uncertain: Option<String>,
    pub persons: Vec<PersonInput>,
}

/// Сохранение записи со всеми упомянутыми персонами.
///
/// Попутно происходит три вещи, ради которых всё и затевалось: населённые
/// пункты заводятся по первому упоминанию, введённые вручную звания попадают
/// в справочник, а частота использования растёт — на ней держится порядок
/// подсказок.
#[derive(Serialize)]
pub struct Saved {
    pub id: i64,
    /// Дело, к которому привязана запись, — дело её года книги.
    pub case_id: i64,
    /// Год, которому при этом сохранении заведено новое дело (копией
    /// прежнего): форма напомнит проверить фонд, опись и дело.
    pub new_case_year: Option<i64>,
}

/// Дело года книги: есть — оно; нет — дело без года получает этот год; нет и
/// такого — копия дела, с которым работали (вторым значением — true).
pub fn case_for_year(conn: &Connection, year: i64) -> Result<(i64, bool), String> {
    let find = statement("case_for_year")?;
    let get = |conn: &Connection| -> Result<Option<i64>, String> {
        conn.query_row(&find, rusqlite::named_params! { ":year": year }, |r| r.get(0))
            .optional().map_err(|e| e.to_string())
    };
    if let Some(id) = get(conn)? {
        return Ok((id, false));
    }
    let adopted = conn.execute(&statement("case_adopt_year")?, rusqlite::named_params! { ":year": year })
        .map_err(|e| e.to_string())?;
    if adopted > 0 {
        if let Some(id) = get(conn)? {
            return Ok((id, false));
        }
    }
    conn.execute(&statement("case_copy_for_year")?, rusqlite::named_params! { ":year": year })
        .map_err(|e| e.to_string())?;
    match get(conn)? {
        Some(id) => Ok((id, true)),
        None => Err("дело не заведено: сначала заполните и сохраните экран «Дело»".into()),
    }
}

pub fn save_entry(conn: &Connection, entry: &EntryInput) -> Result<Saved, String> {
    let parish: String = conn
        .query_row("SELECT parish_key FROM mk_case ORDER BY (id = ?1) DESC, id LIMIT 1", [entry.case_id], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();

    conn.execute_batch("BEGIN").map_err(|e| e.to_string())?;
    match save_entry_in_tx(conn, entry, &parish) {
        Ok(saved) => {
            conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
            Ok(saved)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// Само сохранение — без своей транзакции: импорт из Excel кладёт тысячи
/// записей одной общей (спека 2026-10-02, п. 4.5). `parish` — ключ прихода
/// для частот подсказок.
pub fn save_entry_in_tx(conn: &Connection, entry: &EntryInput, parish: &str) -> Result<Saved, String> {
    {
        // Запись привязывается к делу своего года книги (год обряда, у браков
        // — год венчания), а не к тому, что прислала форма: дело — на год.
        let (case_id, new_case_year) = match entry.rite_year.or(entry.event_year) {
            Some(year) => {
                let (id, created) = case_for_year(conn, year)?;
                (id, created.then_some(year))
            }
            None => (entry.case_id, None),
        };
        let entry_id = match entry.id {
            Some(id) => {
                conn.execute(&statement("entry_update")?, rusqlite::named_params! {
                    ":id": id, ":case_id": case_id, ":page": entry.page, ":no_male": entry.no_male,
                    ":no_female": entry.no_female, ":event_day": entry.event_day,
                    ":event_month": entry.event_month, ":event_year": entry.event_year,
                    ":rite_day": entry.rite_day, ":rite_month": entry.rite_month,
                    ":rite_year": entry.rite_year, ":note": entry.note,
                    ":uncertain": entry.uncertain,
                }).map_err(|e| e.to_string())?;
                id
            }
            None => {
                conn.execute(&statement("entry_insert")?, rusqlite::named_params! {
                    ":case_id": case_id, ":section": entry.section, ":page": entry.page,
                    ":no_male": entry.no_male, ":no_female": entry.no_female,
                    ":event_day": entry.event_day, ":event_month": entry.event_month,
                    ":event_year": entry.event_year, ":rite_day": entry.rite_day,
                    ":rite_month": entry.rite_month, ":rite_year": entry.rite_year,
                    ":note": entry.note, ":uncertain": entry.uncertain,
                    ":created_by": Option::<String>::None,
                }).map_err(|e| e.to_string())?;
                conn.last_insert_rowid()
            }
        };

        conn.execute(&statement("mentions_clear")?,
                     rusqlite::named_params! { ":entry_id": entry_id })
            .map_err(|e| e.to_string())?;

        for person in &entry.persons {
            let place_id = match person.place.as_deref().map(str::trim) {
                Some(name) if !name.is_empty() => Some(place_id_for(conn, name)?),
                _ => None,
            };

            conn.execute(&statement("mention_insert")?, rusqlite::named_params! {
                ":entry_id": entry_id, ":role_code": person.role_code,
                ":sort_order": person.sort_order, ":surname": person.surname,
                ":first_name": person.first_name, ":patronymic": person.patronymic,
                ":surname_modern": person.surname_modern,
                ":first_name_modern": person.first_name_modern,
                ":patronymic_modern": person.patronymic_modern,
                ":maiden_surname": person.maiden_surname, ":gender": person.gender,
                ":rank": person.rank, ":confession": person.confession,
                ":place_id": place_id, ":note": person.note, ":uncertain": person.uncertain,
                ":birth_year_from": Option::<i64>::None, ":birth_year_to": Option::<i64>::None,
                ":age_years": person.age_years, ":marriage_order": person.marriage_order,
                ":kinship": person.kinship,
                ":age_months": person.age_months, ":age_weeks": person.age_weeks,
                ":age_days": person.age_days, ":age_text": person.age_text,
                ":death_cause": person.death_cause,
            }).map_err(|e| e.to_string())?;
            if let Some(v) = person.marriage_order.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                remember(conn, "marriage_order", v, case_id, parish)?;
            }
            if let Some(v) = person.kinship.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                remember(conn, "kinship", v, case_id, parish)?;
            }
            if let Some(v) = person.death_cause.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                remember(conn, "death_cause", v, case_id, parish)?;
            }

            // Звание — в справочник и в статистику. Перечень выбирается
            // по полу: у женщин свой список, это разные перечни.
            if let Some(rank) = person.rank.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                // У причта свой перечень — из него и подсказка ClergyBlock
                // (ревьюер #37: звания причта засоряли мужские).
                let kind = if person.role_code.starts_with("clergy") { "rank_clergy" }
                           else if person.gender.as_deref() == Some("Ж") { "rank_f" } else { "rank_m" };
                remember(conn, kind, rank, case_id, parish)?;
            }
            if let Some(place) = person.place.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                remember(conn, "place", place, case_id, parish)?;
            }
            if let Some(name) = person.first_name.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                remember(conn, "first_name", name, case_id, parish)?;
            }
            if let Some(patr) = person.patronymic.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                remember(conn, "patronymic", patr, case_id, parish)?;
            }

            // Причт — в свою память, чтобы его можно было выбрать списком,
            // а не набирать заново в каждом деле.
            if person.role_code.starts_with("clergy") {
                let iof = person_iof(person);
                if !iof.is_empty() {
                    conn.execute(&statement("clergy_remember")?, rusqlite::named_params! {
                        ":iof": iof, ":iof_norm": normalize(&iof), ":rank": person.rank,
                    }).map_err(|e| e.to_string())?;
                }
            }

            // Персона целиком — вместе с населённым пунктом и званием.
            // Заказчик 17.08.2026: выбор персоны должен заполнять все три
            // поля разом, а не заставлять набирать каждое.
            // Умерший — нет: живым в следующих записях он не встретится, а в
            // подсказках отцов и восприемников мешал бы (ревьюер 27.09.2026).
            let iof = person_iof(person);
            if !iof.is_empty() && person.role_code != "deceased" {
                conn.execute(&statement("person_remember")?, rusqlite::named_params! {
                    ":iof": iof, ":iof_norm": normalize(&iof),
                    ":place": person.place, ":rank": person.rank, ":gender": person.gender,
                }).map_err(|e| e.to_string())?;
            }
        }

        // Кто чья жена: чтобы выбор отца заполнял мать.
        let father = entry.persons.iter().find(|p| p.role_code == "father");
        let mother = entry.persons.iter().find(|p| p.role_code == "mother");
        if let (Some(f), Some(m)) = (father, mother) {
            let husband = person_iof(f);
            let wife = person_iof(m);
            if !husband.is_empty() && !wife.is_empty() {
                conn.execute(&statement("spouse_remember")?, rusqlite::named_params! {
                    ":husband_norm": normalize(&husband), ":wife_iof": wife,
                    ":wife_place": m.place, ":wife_rank": m.rank,
                }).map_err(|e| e.to_string())?;
            }
        }
        // Венчание — тоже пара: в рождениях после него жена подставится
        // по мужу. Жить она будет у мужа — НП жениха; звание невесты
        // («дочь-девица») жене не годится — пусто, форма поставит своё.
        let groom = entry.persons.iter().find(|p| p.role_code == "groom");
        let bride = entry.persons.iter().find(|p| p.role_code == "bride");
        if let (Some(g), Some(b)) = (groom, bride) {
            let husband = person_iof(g);
            let wife = person_iof(b);
            if !husband.is_empty() && !wife.is_empty() {
                conn.execute(&statement("spouse_remember")?, rusqlite::named_params! {
                    ":husband_norm": normalize(&husband), ":wife_iof": wife,
                    ":wife_place": g.place, ":wife_rank": Option::<String>::None,
                }).map_err(|e| e.to_string())?;
            }
        }
        Ok(Saved { id: entry_id, case_id, new_case_year })
    }
}

/// Собирает ИОФ из частей — в том порядке, в каком он записан в книге.
pub fn person_iof(p: &PersonInput) -> String {
    [&p.first_name, &p.patronymic, &p.surname]
        .iter()
        .filter_map(|v| v.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Находит населённый пункт по названию или заводит новый.
pub fn place_id_for(conn: &Connection, name: &str) -> Result<i64, String> {
    let norm = normalize(name);
    if let Some(id) = conn
        .query_row(&statement("place_find")?, rusqlite::named_params! { ":name_norm": norm },
                   |r| r.get::<_, i64>(0))
        .optional()
        .map_err(|e| e.to_string())?
    {
        return Ok(id);
    }
    conn.execute(&statement("place_insert")?,
                 rusqlite::named_params! { ":name": name, ":name_norm": norm })
        .map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

/// Запоминает введённое значение: пополняет справочник и наращивает частоту
/// в трёх охватах — дело, приход, вся база. Именно в таком порядке потом
/// выдаются подсказки.
pub fn remember(conn: &Connection, kind: &str, value: &str, case_id: i64, parish: &str)
    -> Result<(), String>
{
    // Звание в старой орфографии, которое в перечне уже есть по-современному,
    // перечень не пополняет и частоту наращивает прежнему («крестьянинъ» →
    // «крестьянин»). В записи остаётся как набрано.
    let canonical: Option<String> = if kind.starts_with("rank") {
        conn.query_row(&statement("lookup_by_norm")?,
                       rusqlite::named_params! { ":kind": kind, ":value_norm": normalize_words(value) },
                       |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())?
    } else {
        None
    };
    let value = canonical.as_deref().unwrap_or(value);
    let norm = normalize(value);
    conn.execute(&statement("lookup_extend")?, rusqlite::named_params! {
        ":kind": kind, ":value": value, ":value_norm": norm,
    }).map_err(|e| e.to_string())?;

    let bump = statement("usage_bump")?;
    for (scope, key) in [("case", case_id.to_string()), ("parish", parish.to_string()),
                         ("global", String::new())] {
        conn.execute(&bump, rusqlite::named_params! {
            ":kind": kind, ":scope": scope, ":scope_key": key,
            ":value": value, ":value_norm": norm,
        }).map_err(|e| e.to_string())?;
    }
    Ok(())
}
