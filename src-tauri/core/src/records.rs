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
    /// Имя в книге не указано: первым словом стоит системное «***» (или
    /// другие знаки без букв). Сверять не с чем — форма так и пишет.
    pub name_missing: bool,
}

/// Системное слово «имя в книге не указано» (спека 2026-10-05, часть Б, п. 1).
/// Роман ставил в Excel звёздочки; в записи всегда хранится именно так.
pub const NO_NAME: &str = "***";

/// Имени в книге нет: слово из одних знаков («***», «*», «—», «?») или «Имя» —
/// заготовка ячейки в Excel-индексаторе.
///
/// Одно правило и для набора, и для импорта (06.10.2026; раньше импорт считал
/// по-своему). Цифра — не «имени нет»: это скорее опечатка, и её должен
/// увидеть человек — в форме окном сверки, при импорте строкой в списке на
/// сверку. Копия для окна — `noNameWord` в src/names.ts: правишь одно — правь
/// другое (scripts/test_names.mjs сверяет те же примеры).
pub fn is_no_name(word: &str) -> bool {
    let word = word.trim();
    !word.is_empty() && (!word.chars().any(|c| c.is_alphanumeric()) || word.to_lowercase() == "имя")
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
        name_missing: false,
    };
    if tokens.is_empty() {
        return Ok(out);
    }
    let missing = is_no_name(tokens[0]);

    let alias_find = statement("alias_find")?;
    let alias = |kind: &str, word: &str| -> Result<Option<(Option<String>, Option<String>)>, String> {
        conn.query_row(&alias_find,
                       rusqlite::named_params! { ":kind": kind, ":form_norm": normalize_name(word) },
                       |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
            .map_err(|e| e.to_string())
    };

    out.first_name = Some(if missing { NO_NAME.to_string() } else { tokens[0].to_string() });
    if missing {
        // Имени нет — сверять нечего; пол подскажет отчество, если оно есть.
        out.known_name = true;
        out.name_missing = true;
    }
    // Сначала соответствие, заведённое человеком: оно сильнее словаря.
    // Целевое имя ищется в словаре как обычное — с полом и основой.
    let mut lookup_word = tokens[0].to_string();
    match if missing { None } else { alias("name", tokens[0])? } {
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

/// Итог «Сохранить дело».
#[derive(Serialize, Debug)]
pub struct CaseSaved {
    pub id: i64,
    /// saved — дело поправлено; created — году заведено новое дело; exists —
    /// у набранного года уже есть другое дело, ничего не записано: окно
    /// спросит, заменять ли его реквизиты.
    pub status: String,
    /// Реквизиты того дела («Ф.56 Оп.31 Д.18») — для вопроса.
    pub existing: Option<String>,
}

/// «Сохранить дело» (спека 2026-10-03, п. 1). Роман: «дать пользователю
/// возможность сначала изменить архивный шифр …, а уже затем выбрать или
/// изменить Год». Год тот же — правится открытое дело; год новый — ему
/// заводится своё дело с набранными реквизитами, прежний год не меняется;
/// год другой и его дело уже есть — только с согласия (`overwrite`).
pub fn save_case(conn: &Connection, case: &Case, overwrite: bool) -> Result<CaseSaved, String> {
    let e = |e: rusqlite::Error| e.to_string();
    let loaded_year: Option<i64> = if case.id > 0 {
        conn.query_row(&statement("case_year_of")?, rusqlite::named_params! { ":id": case.id }, |r| r.get(0))
            .optional().map_err(e)?.flatten()
    } else {
        None
    };
    let other: Option<i64> = match case.year {
        Some(year) => conn
            .query_row(&statement("case_for_year")?, rusqlite::named_params! { ":year": year }, |r| r.get(0))
            .optional().map_err(e)?,
        None => None,
    };
    let (id, status) = match (other, case.year, loaded_year) {
        // У набранного года есть своё дело, и это не открытое.
        (Some(o), _, _) if o != case.id => {
            if !overwrite {
                let brief: String = conn
                    .query_row(&statement("case_brief")?, rusqlite::named_params! { ":id": o }, |r| r.get(0))
                    .map_err(e)?;
                return Ok(CaseSaved { id: o, status: "exists".into(), existing: Some(brief) });
            }
            (o, "saved")
        }
        // Новый год при открытом деле другого года — новое дело.
        (None, Some(year), Some(was)) if year != was => {
            let id: i64 = conn.query_row(&statement("case_new_id")?, [], |r| r.get(0)).map_err(e)?;
            (id, "created")
        }
        _ => (if case.id > 0 { case.id } else { 1 }, "saved"),
    };
    let parish = case.parish_key();
    conn.execute(&statement("case_upsert")?, rusqlite::named_params! {
        ":id": id, ":archive": case.archive, ":fond": case.fond, ":opis": case.opis,
        ":delo": case.delo, ":church": case.church, ":village": case.village,
        ":uyezd": case.uyezd, ":guberniya": case.guberniya, ":year": case.year,
        ":parish_key": parish, ":indexer": case.indexer,
    }).map_err(e)?;
    // Церковь, село, уезд, губерния, индексатор — свойства прихода: на
    // дела всех лет. Фонд, опись и дело остаются у своего года.
    conn.execute(&statement("case_spread_parish")?, rusqlite::named_params! {
        ":id": id, ":archive": case.archive, ":church": case.church, ":village": case.village,
        ":uyezd": case.uyezd, ":guberniya": case.guberniya,
        ":parish_key": parish, ":indexer": case.indexer,
    }).map_err(e)?;
    // Архив, церковь, уезд, губерния — в справочники, как звания при
    // сохранении записи. Заказчик 21.09.2026: «должна сохраниться
    // возможность добавить свой [архив], если его нет в списке, и чтобы
    // он добавлялся в базу». Перечни помечены autoextend в lookup_kind.
    for (kind, value) in [("archive", &case.archive), ("church", &case.church),
                          ("uyezd", &case.uyezd), ("guberniya", &case.guberniya)] {
        if let Some(v) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            remember(conn, kind, v, id, &parish)?;
        }
    }
    Ok(CaseSaved { id, status: status.into(), existing: None })
}

/// Первое заполнение частот фамилий: записи, набранные или импортированные до
/// появления подсказки фамилий, должны в неё попасть. Делается один раз — пока
/// в частотах нет ни одной фамилии; дальше их наращивает сохранение записи.
pub fn seed_surnames(conn: &Connection) -> Result<usize, String> {
    let e = |e: rusqlite::Error| e.to_string();
    let has: bool = conn
        .query_row(&statement("usage_has")?, rusqlite::named_params! { ":kind": "surname" }, |r| r.get(0))
        .map_err(e)?;
    if has {
        return Ok(0);
    }
    let parish: String = conn
        .query_row(&statement("case_parish_key")?, rusqlite::named_params! { ":id": 0 },
                   |r| r.get::<_, Option<String>>(0))
        .optional().map_err(e)?.flatten().unwrap_or_default();
    let rows: Vec<(String, i64)> = {
        let mut stmt = conn.prepare(&statement("surname_counts")?).map_err(e)?;
        let found = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(e)?;
        found.collect::<Result<Vec<_>, _>>().map_err(e)?
    };
    let set = statement("usage_set")?;
    // Одной транзакцией: оборвись заполнение посередине, «частоты уже есть»
    // было бы правдой, и остаток фамилий не досчитался бы никогда (ревьюер).
    let tx = conn.unchecked_transaction().map_err(e)?;
    for (surname, n) in &rows {
        for (scope, key) in [("parish", parish.as_str()), ("global", "")] {
            tx.execute(&set, rusqlite::named_params! {
                ":kind": "surname", ":scope": scope, ":scope_key": key,
                ":value": surname, ":value_norm": normalize(surname), ":count": n,
            }).map_err(e)?;
        }
    }
    tx.commit().map_err(e)?;
    Ok(rows.len())
}

/// Набранное в карточке пункта — в перечни (губерния, уезд, волость), чтобы
/// следующая карточка их подсказала (Роман 02.10.2026). Без частот: это не
/// подсказка по привычке, а список.
pub fn extend_lookup(conn: &Connection, kind: &str, value: Option<&str>) -> Result<(), String> {
    let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) else { return Ok(()) };
    conn.execute(&statement("lookup_extend")?, rusqlite::named_params! {
        ":kind": kind, ":value": v, ":value_norm": normalize(v),
    }).map(|_| ()).map_err(|e| e.to_string())
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
    /// Запись без года, а дела, которое назвала форма, уже нет: запись легла
    /// в первое дело прихода. Форма скажет об этом — молча нельзя.
    pub fallback_case: bool,
    /// Год дела, к которому привязана запись без года (для того же сообщения).
    pub fallback_year: Option<i64>,
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
        .query_row(&statement("case_parish_key")?, rusqlite::named_params! { ":id": entry.case_id },
                   |r| r.get::<_, Option<String>>(0))
        .optional()
        .map_err(|e| e.to_string())?
        .flatten()
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
        let mut fallback: Option<Option<i64>> = None;
        let (case_id, new_case_year) = match entry.rite_year.or(entry.event_year) {
            Some(year) => {
                let (id, created) = case_for_year(conn, year)?;
                (id, created.then_some(year))
            }
            // Записи без года дело назначает форма; если его уже убрали
            // («Убрать дело этого года»), берём первое дело прихода — иначе
            // сохранение упало бы на внешнем ключе. Не молча: `fallback`.
            None => {
                let found: Option<(i64, Option<i64>)> = conn
                    .query_row(&statement("case_or_first")?, rusqlite::named_params! { ":id": entry.case_id },
                               |r| Ok((r.get(0)?, r.get(1)?)))
                    .optional().map_err(|e| e.to_string())?;
                if let Some((id, year)) = found {
                    if id != entry.case_id {
                        fallback = Some(year);
                    }
                }
                (found.map(|f| f.0).unwrap_or(entry.case_id), None)
            }
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
            // «***» (имя не указано) — не имя: в частоты и память персон не идёт.
            let nameless = person.first_name.as_deref().map(str::trim) == Some(NO_NAME);
            if let Some(name) = person.first_name.as_deref().map(str::trim).filter(|v| !v.is_empty() && !nameless) {
                remember(conn, "first_name", name, case_id, parish)?;
            }
            // Фамилии прихода — для подсказки третьим словом ИОФ (Роман
            // 05.10.2026: «чтобы не плодить дубли вроде „Томилин“ и „Тамилин“»).
            // Причт не считаем: он повторяется в каждой записи, и фамилия
            // священника стояла бы первой на свою букву (ревьюер).
            if let Some(surname) = person.surname.as_deref().map(str::trim)
                .filter(|v| !v.is_empty() && !person.role_code.starts_with("clergy"))
            {
                remember(conn, "surname", surname, case_id, parish)?;
            }
            if let Some(patr) = person.patronymic.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                remember(conn, "patronymic", patr, case_id, parish)?;
            }

            // Причт — в свою память, чтобы его можно было выбрать списком,
            // а не набирать заново в каждом деле.
            if person.role_code.starts_with("clergy") {
                let iof = person_iof(person);
                if !iof.is_empty() && !nameless {
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
            if !iof.is_empty() && person.role_code != "deceased" && !nameless {
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
            // Супруг без имени («***») в память пар не идёт: иначе выбор мужа
            // подставлял бы жену со звёздочками (ревьюер 05.10.2026).
            if !husband.is_empty() && !wife.is_empty() && !no_first_name(f) && !no_first_name(m) {
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
            if !husband.is_empty() && !wife.is_empty() && !no_first_name(g) && !no_first_name(b) {
                conn.execute(&statement("spouse_remember")?, rusqlite::named_params! {
                    ":husband_norm": normalize(&husband), ":wife_iof": wife,
                    ":wife_place": g.place, ":wife_rank": Option::<String>::None,
                }).map_err(|e| e.to_string())?;
            }
        }
        Ok(Saved { id: entry_id, case_id, new_case_year, fallback_case: fallback.is_some(),
                   fallback_year: fallback.flatten() })
    }
}

/// Имя персоны — системное «***» (в книге не указано).
fn no_first_name(p: &PersonInput) -> bool {
    p.first_name.as_deref().map(str::trim) == Some(NO_NAME)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// «Сохранить дело»: три ветки спеки 2026-10-03, п. 1, и сценарий Романа —
    /// сначала реквизиты и год, потом запись.
    #[test]
    fn case_free_order() {
        let seed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../resources/seed.sqlite");
        if !seed.exists() {
            eprintln!("нет resources/seed.sqlite — тест дела пропущен");
            return;
        }
        let path = std::env::temp_dir().join(format!("genmetric-case-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::copy(&seed, &path).unwrap();
        let conn = Connection::open(&path).unwrap();
        let case = |id: i64, year: Option<i64>, delo: &str| Case {
            id, archive: Some("ГА".into()), fond: Some("56".into()), opis: Some("31".into()),
            delo: Some(delo.into()), church: Some("Никольская".into()), village: Some("Тестово".into()),
            uyezd: None, guberniya: None, year, indexer: None,
        };
        let fod = |year: i64| -> String {
            conn.query_row("SELECT delo FROM mk_case WHERE year = ?1", [year], |r| r.get(0)).unwrap()
        };
        let count = || -> i64 { conn.query_row("SELECT count(*) FROM mk_case", [], |r| r.get(0)).unwrap() };

        // Первое дело — без года, потом с годом: то же дело.
        let first = save_case(&conn, &case(0, None, "11"), false).unwrap();
        assert_eq!((first.id, first.status.as_str()), (1, "saved"));
        let same = save_case(&conn, &case(1, Some(1897), "11"), false).unwrap();
        assert_eq!((same.id, same.status.as_str(), count()), (1, "saved", 1));

        // Сценарий Романа: поправил дело, поставил новый год, сохранил.
        let new = save_case(&conn, &case(1, Some(1898), "12"), false).unwrap();
        assert_eq!((new.status.as_str(), count()), ("created", 2));
        assert_eq!((fod(1897), fod(1898)), ("11".to_string(), "12".to_string()), "прежний год не тронут");

        // Запись нового года ложится в это дело, копии не заводится.
        let entry = EntryInput {
            id: None, case_id: 1, section: 2, page: None, no_male: Some(1), no_female: None,
            event_day: Some(1), event_month: Some(2), event_year: Some(1898),
            rite_day: None, rite_month: None, rite_year: None, note: None, uncertain: None, persons: vec![],
        };
        let saved = save_entry(&conn, &entry).unwrap();
        assert_eq!((saved.case_id, saved.new_case_year, count()), (new.id, None, 2));

        // Год другого дела: без согласия — вопрос, ничего не записано.
        let asked = save_case(&conn, &case(new.id, Some(1897), "99"), false).unwrap();
        assert_eq!((asked.status.as_str(), asked.existing.as_deref()), ("exists", Some("Ф.56 Оп.31 Д.11")));
        assert_eq!((fod(1897), fod(1898)), ("11".to_string(), "12".to_string()));
        let forced = save_case(&conn, &case(new.id, Some(1897), "99"), true).unwrap();
        assert_eq!((forced.id, forced.status.as_str(), fod(1897), fod(1898)), (1, "saved", "99".to_string(), "12".to_string()));

        // Пустой год уходит, год с записями и последнее дело — нет.
        save_case(&conn, &case(1, Some(1987), "1"), false).unwrap();
        let drop = |year: i64| conn.execute(&statement("case_delete_empty").unwrap(),
                                             rusqlite::named_params! { ":year": year }).unwrap();
        assert_eq!((drop(1987), drop(1898), count()), (1, 0, 2));
        drop(1897);
        assert_eq!((count(), drop(1898)), (1, 0), "последнее дело прихода не убирается");

        std::mem::drop(conn);
        let _ = std::fs::remove_file(path);
    }
    /// «Имя не указано» и фамилии прихода (спека 2026-10-05, часть Б).
    #[test]
    fn no_name_and_surnames() {
        let seed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../resources/seed.sqlite");
        if !seed.exists() {
            eprintln!("нет resources/seed.sqlite — тест пропущен");
            return;
        }
        let path = std::env::temp_dir().join(format!("genmetric-noname-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::copy(&seed, &path).unwrap();
        let conn = Connection::open(&path).unwrap();
        for stub in ["*** Иванова Петрова", "* Иванова Петрова", "— Иванова Петрова", "Имя Иванова Петрова"] {
            let p = parse_iof_in(&conn, stub).unwrap();
            assert!(p.known_name && p.name_missing, "{stub}");
            assert_eq!((p.first_name.as_deref(), p.first_name_modern.as_deref()), (Some("***"), None), "{stub}");
            assert_eq!((p.patronymic.as_deref(), p.surname.as_deref(), p.gender.as_deref()),
                       (Some("Иванова"), Some("Петрова"), Some("Ж")), "{stub}");
        }
        let plain = parse_iof_in(&conn, "Мария Иванова").unwrap();
        assert!(plain.known_name && !plain.name_missing);
        // Те же примеры — в scripts/test_names.mjs для копии правила в окне.
        for word in ["***", "*", "—", "?", "-", "Имя", "имя", "ИМЯ"] {
            assert!(is_no_name(word), "{word}");
        }
        for word in ["Иван", "2", "", "Имярек", "Им", "N", "*а"] {
            assert!(!is_no_name(word), "{word}");
        }

        let case = Case { id: 0, archive: None, fond: None, opis: None, delo: None, church: Some("Ц".into()),
                          village: Some("Тестово".into()), uyezd: None, guberniya: None, year: Some(1897), indexer: None };
        save_case(&conn, &case, false).unwrap();
        let person = |role: &str, first: &str, surname: &str| PersonInput {
            role_code: role.into(), sort_order: 10, surname: Some(surname.into()), first_name: Some(first.into()),
            patronymic: Some("Иванова".into()), surname_modern: None, first_name_modern: None,
            patronymic_modern: Some("Ивановна".into()), maiden_surname: None, gender: Some("Ж".into()),
            rank: None, confession: None, place: None, note: None, uncertain: None, age_years: None,
            marriage_order: None, kinship: None, age_months: None, age_weeks: None, age_days: None,
            age_text: None, death_cause: None,
        };
        let entry = EntryInput {
            id: None, case_id: 1, section: 1, page: None, no_male: None, no_female: Some(1),
            event_day: None, event_month: None, event_year: Some(1897), rite_day: None, rite_month: None,
            rite_year: Some(1897), note: None, uncertain: None,
            persons: vec![person("mother", "***", "Томилина"), person("godparent1", "Анна", "Томилина")],
        };
        save_entry(&conn, &entry).unwrap();
        let num = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(num("SELECT count(*) FROM person_index WHERE iof LIKE '***%'"), 0, "«***» в память персон не идёт");
        assert_eq!(num("SELECT count(*) FROM spouse_index WHERE wife_iof LIKE '***%'"), 0, "…и в память пар");
        assert_eq!(num("SELECT count(*) FROM usage_stat WHERE kind = 'first_name' AND value = '***'"), 0);
        assert_eq!(num("SELECT count FROM usage_stat WHERE kind = 'surname' AND scope = 'parish' AND value = 'Томилина'"), 2);
        assert_eq!(seed_surnames(&conn).unwrap(), 0, "частоты фамилий уже есть — второй раз не заполняются");
        // Приход, набранный до подсказки фамилий: частоты считаются по записям.
        conn.execute("DELETE FROM usage_stat WHERE kind = 'surname'", []).unwrap();
        assert_eq!(seed_surnames(&conn).unwrap(), 1);
        assert_eq!(num("SELECT count FROM usage_stat WHERE kind = 'surname' AND scope = 'parish' AND value_norm = 'томилина'"), 2);
        // В выгрузке Familio имени у «***» нет.
        crate::export::familio_rows(&conn).unwrap();
        assert_eq!(num("SELECT count(*) FROM x_person WHERE role_code = 'mother' AND first_m IS NULL"), 1);
        assert_eq!(num("SELECT count(*) FROM x_person WHERE role_code = 'godparent1' AND first_m = 'Анна'"), 1);
        // Запись без года с убранным делом не падает.
        let mut loose = EntryInput { id: None, case_id: 999, section: 2, page: None, no_male: None, no_female: None,
            event_day: None, event_month: None, event_year: None, rite_day: None, rite_month: None, rite_year: None,
            note: None, uncertain: None, persons: vec![] };
        let moved = save_entry(&conn, &loose).unwrap();
        assert_eq!((moved.case_id, moved.fallback_case, moved.fallback_year), (1, true, Some(1897)));
        // Своё дело на месте — оговорки нет.
        loose.case_id = 1;
        let own = save_entry(&conn, &loose).unwrap();
        assert_eq!((own.case_id, own.fallback_case, own.fallback_year), (1, false, None));
        std::mem::drop(conn);
        let _ = std::fs::remove_file(path);
    }
}
