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
    /// Фамилия есть, и ни её, ни её формы другого рода в приходе ещё не
    /// набирали: форма покажет «такой фамилии в приходе ещё не было» — так
    /// видна опечатка («Тамилин» при «Томилин»).
    pub surname_new: bool,
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
///
/// 06.10.2026 Роман расширил список: «общепринятые текстовые сокращения,
/// означающие отсутствие данных или нечитаемый текст» — `NO_NAME_WORDS`,
/// без учёта регистра, с точкой на конце или без.
pub fn is_no_name(word: &str) -> bool {
    let word = word.trim();
    if word.is_empty() {
        return false;
    }
    if !word.chars().any(|c| c.is_alphanumeric()) {
        return true;
    }
    let lower = word.to_lowercase();
    NO_NAME_WORDS.contains(&lower.trim_end_matches('.'))
}

/// Слова-заглушки вместо имени. Тот же список — в `src/names.ts`.
pub const NO_NAME_WORDS: &[&str] = &["имя", "нрзб", "н/д", "неизвестно", "неизв", "нет", "б/и"];

/// Мужская и женская формы одной фамилии — ключи поиска обеих: «томилин» и
/// «томилина», «томский» и «томская». У фамилии без родового окончания
/// («шевченко») обе одинаковы. Копия для окна — `feminineSurname` в
/// src/names.ts (там нужна только женская форма).
pub fn surname_pair(surname: &str) -> (String, String) {
    let n = normalize(surname);
    let n = n.strip_suffix('ъ').map(str::to_string).unwrap_or(n);
    let cut = |s: &str, k: usize| -> String { s.chars().take(s.chars().count().saturating_sub(k)).collect() };
    let ends = |tails: &[&str]| tails.iter().any(|t| n.ends_with(t));
    if ends(&["ова", "ева", "ина", "ына"]) {
        (cut(&n, 1), n)
    } else if ends(&["ов", "ев", "ин", "ын"]) {
        (n.clone(), format!("{n}а"))
    } else if ends(&["ская", "цкая"]) {
        (format!("{}ий", cut(&n, 2)), n)
    } else if ends(&["ский", "цкий", "ской", "цкой", "ый", "ой"]) {
        (n.clone(), format!("{}ая", cut(&n, 2)))
    } else if ends(&["ая"]) {
        // «Белая» — «Белый» или «Толстая» — «Толстой»: мужскую форму не
        // угадать, пара ищется по обеим (вторая — в запросе, через ключ ниже).
        (format!("{}ый", cut(&n, 2)), n)
    } else {
        (n.clone(), n)
    }
}

/// Самое частое звание роли и пола в приходе; None — таких записей ещё нет.
pub fn default_rank(conn: &Connection, role: &str, gender: Option<&str>) -> Result<Option<String>, String> {
    conn.query_row(&statement("rank_default")?,
                   rusqlite::named_params! { ":role": role, ":gender": gender }, |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())
}

/// Звание матери в записи о рождении — оно же попадает в память персон.
pub const WIFE_RANK: &str = "законная жена его";

/// Звание жены по званию мужа — для подсказки персон вне поля матери (Роман
/// 06.10.2026): «заменять на „крестьянская жена“… если возможно реализовать
/// показ помимо крестьянской также и мещанской и солдатской жены… Если звание
/// мужа необычное, программа не должна пытаться сама выдумать статус. Пусть
/// она просто предлагает „жена“». Слова, по которым узнаётся сословие, — из
/// его перечня званий. Умерший муж — вдова, это уже «необычное».
pub fn wife_rank(husband_rank: Option<&str>) -> &'static str {
    let r = normalize(husband_rank.unwrap_or(""));
    if r.is_empty() || r.contains("умерш") {
        return "жена";
    }
    // «Отставной» и «запасной» сами по себе не значат «солдат» («отставной
    // фельдшер», «отставной церковник» — необычное, просто «жена»); военным
    // делает слово при них или «в запас армии».
    const SOLDIER: [&str; 10] = ["солдат", "рядов", "унтер", "бомбардир", "фейерверкер", "ефрейтор",
                                 "нижний чин", "нижнего чина", "военной служб", "запас армии"];
    if SOLDIER.iter().any(|w| r.contains(w)) {
        "солдатская жена"
    } else if r.contains("крестьян") {
        "крестьянская жена"
    } else if r.contains("мещан") {
        "мещанская жена"
    } else {
        "жена"
    }
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
        surname_new: false,
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
        let surname = rest.join(" ");
        let (a, b) = surname_pair(&surname);
        // «Толстая»: мужская форма — «Толстый» или «Толстой».
        let c = match a.strip_suffix("ый") {
            Some(stem) if b.ends_with("ая") => format!("{stem}ой"),
            _ => a.clone(),
        };
        let known: bool = conn
            .query_row(&statement("surname_known")?, rusqlite::named_params! { ":a": a, ":b": b, ":c": c }, |r| r.get(0))
            .map_err(|e| e.to_string())?;
        out.surname_new = !known;
        out.surname = Some(surname);
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

/// Первое заполнение частот там, где они до 06.10.2026 не велись:
/// вероисповедание (по записям) и поля карточки пункта (по пунктам). Один раз
/// на приход — отметка `usage_seeded` в его настройках; дальше частоты
/// наращивает сохранение. Уже имеющиеся частоты (уезд и губерния дела) не
/// занижаются: берётся большее. Записи и пункты не меняются.
pub fn seed_usage(conn: &Connection) -> Result<(), String> {
    const MARK: &str = "usage_seeded";
    const VERSION: &str = "1";
    let e = |e: rusqlite::Error| e.to_string();
    let done: Option<String> = conn
        .query_row(&statement("setting_get")?, rusqlite::named_params! { ":key": MARK }, |r| r.get(0))
        .optional().map_err(e)?;
    if done.as_deref() == Some(VERSION) {
        return Ok(());
    }
    let mut rows: Vec<(String, String, i64)> = Vec::new();
    {
        let mut stmt = conn.prepare(&statement("confession_counts")?).map_err(e)?;
        let found = stmt.query_map([], |r| Ok(("confession".to_string(), r.get(0)?, r.get(1)?))).map_err(e)?;
        rows.extend(found.collect::<Result<Vec<_>, _>>().map_err(e)?);
        let mut stmt = conn.prepare(&statement("place_field_counts")?).map_err(e)?;
        let found = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?))).map_err(e)?;
        rows.extend(found.collect::<Result<Vec<_>, _>>().map_err(e)?);
    }
    let parish: String = conn
        .query_row(&statement("case_parish_key")?, rusqlite::named_params! { ":id": 0 },
                   |r| r.get::<_, Option<String>>(0))
        .optional().map_err(e)?.flatten().unwrap_or_default();
    let raise = statement("usage_raise")?;
    // Одной транзакцией вместе с отметкой: оборвись подсчёт посередине —
    // отметки нет, и при следующем открытии он пройдёт заново целиком.
    let tx = conn.unchecked_transaction().map_err(e)?;
    for (kind, value, n) in &rows {
        for (scope, key) in [("parish", parish.as_str()), ("global", "")] {
            tx.execute(&raise, rusqlite::named_params! {
                ":kind": kind, ":scope": scope, ":scope_key": key,
                ":value": value, ":value_norm": normalize(value), ":count": n,
            }).map_err(e)?;
        }
    }
    tx.execute(&statement("setting_put")?, rusqlite::named_params! { ":key": MARK, ":value": VERSION }).map_err(e)?;
    tx.commit().map_err(e)
}

/// Поля карточки нового пункта — в перечни и в частоты: следующая карточка
/// предложит самое частое первым.
pub fn remember_card(conn: &Connection, kind: &str, value: Option<&str>) -> Result<(), String> {
    let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) else { return Ok(()) };
    let parish: String = conn
        .query_row(&statement("case_parish_key")?, rusqlite::named_params! { ":id": 0 },
                   |r| r.get::<_, Option<String>>(0))
        .optional().map_err(|e| e.to_string())?.flatten().unwrap_or_default();
    extend_lookup(conn, kind, Some(v))?;
    let bump = statement("usage_bump")?;
    for (scope, key) in [("parish", parish.as_str()), ("global", "")] {
        conn.execute(&bump, rusqlite::named_params! {
            ":kind": kind, ":scope": scope, ":scope_key": key, ":value": v, ":value_norm": normalize(v),
        }).map_err(|e| e.to_string())?;
    }
    Ok(())
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
    /// Файл скана разворота — имя без пути (схема 11). Окно прежней сборки
    /// его не присылает — тогда пусто.
    #[serde(default)]
    pub scan_file: Option<String>,
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
        // Только имя файла: путь к папке сканов у каждого компьютера свой.
        let scan_file = entry.scan_file.as_deref().map(str::trim).filter(|f| !f.is_empty())
            .map(|f| f.rsplit(['/', '\\']).next().unwrap_or(f).to_string());
        let entry_id = match entry.id {
            Some(id) => {
                conn.execute(&statement("entry_update")?, rusqlite::named_params! {
                    ":id": id, ":case_id": case_id, ":page": entry.page, ":no_male": entry.no_male,
                    ":no_female": entry.no_female, ":event_day": entry.event_day,
                    ":event_month": entry.event_month, ":event_year": entry.event_year,
                    ":rite_day": entry.rite_day, ":rite_month": entry.rite_month,
                    ":rite_year": entry.rite_year, ":note": entry.note,
                    ":uncertain": entry.uncertain, ":scan_file": scan_file,
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
                    ":created_by": Option::<String>::None, ":scan_file": scan_file,
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
            // В память персон, пар и частот идёт написание справочника, а не
            // набранное: Ctrl+Enter прямо из поля НП сохраняет «березовка
            // (нежитино)» как есть — поле привести его не успело, — и одна
            // персона получала в подсказке два написания пункта, а поиск жены
            // по НП мужа промахивался (ревьюер 09.10.2026). Сам пункт записи
            // от клавиши не зависел никогда — он ищется по ключу.
            let place_name = place_spelling(conn, person.place.as_deref())?;

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
            if let Some(place) = place_name.as_deref() {
                remember(conn, "place", place, case_id, parish)?;
            }
            // Вероисповедание — тоже в частоты: без них подсказка шла по
            // алфавиту (Роман 06.10.2026: самое частое первым «абсолютно во
            // всех полях»).
            if let Some(v) = person.confession.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                remember(conn, "confession", v, case_id, parish)?;
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
                    ":place": place_name, ":rank": person.rank, ":gender": person.gender,
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
                // Та же жена, теперь с фамилией, — дополнить строку, не заводить вторую.
                let wife_place = place_spelling(conn, m.place.as_deref())?;
                conn.execute(&statement("spouse_extend")?, rusqlite::named_params! {
                    ":husband_norm": normalize(&husband), ":wife_iof": wife, ":wife_place": wife_place,
                }).map_err(|e| e.to_string())?;
                conn.execute(&statement("spouse_remember")?, rusqlite::named_params! {
                    ":husband_norm": normalize(&husband), ":wife_iof": wife,
                    ":wife_place": wife_place, ":wife_rank": m.rank,
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
                    ":wife_place": place_spelling(conn, g.place.as_deref())?, ":wife_rank": Option::<String>::None,
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

/// Название пункта так, как оно стоит в справочнике: набранное «березовка
/// (нежитино)» → «Берёзовка (Нежитино)». Пункта ещё нет или название пусто —
/// как набрано (без пробелов по краям) или ничего.
pub fn place_spelling(conn: &Connection, typed: Option<&str>) -> Result<Option<String>, String> {
    let Some(name) = typed.map(str::trim).filter(|v| !v.is_empty()) else { return Ok(None) };
    let known: Option<String> = conn
        .query_row(&statement("place_find")?, rusqlite::named_params! { ":name_norm": normalize(name) },
                   |r| r.get(1))
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(Some(known.unwrap_or_else(|| name.to_string())))
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
            crate::seed_missing("тест дела пропущен");
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
            rite_day: None, rite_month: None, rite_year: None, note: None, uncertain: None, scan_file: None, persons: vec![],
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
    /// Жена по мужу с учётом НП, память пар без второй строки, строка
    /// подсказки без НП (спека 2026-10-09, пп. 1, 2, 16). Запросы исполняет
    /// настоящий rusqlite — как в программе.
    #[test]
    fn spouse_by_place_and_person_rows() {
        let seed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../resources/seed.sqlite");
        if !seed.exists() {
            crate::seed_missing("тест пропущен");
            return;
        }
        let path = std::env::temp_dir().join(format!("genmetric-spouse-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::copy(&seed, &path).unwrap();
        let conn = Connection::open(&path).unwrap();
        let case = Case { id: 0, archive: None, fond: None, opis: None, delo: None, church: Some("Ц".into()),
                          village: Some("Тестово".into()), uyezd: None, guberniya: None, year: Some(1897), indexer: None };
        save_case(&conn, &case, false).unwrap();
        let person = |role: &str, first: &str, patr: &str, surname: Option<&str>, place: Option<&str>, sex: &str| PersonInput {
            role_code: role.into(), sort_order: 10, surname: surname.map(Into::into), first_name: Some(first.into()),
            patronymic: Some(patr.into()), surname_modern: None, first_name_modern: None,
            patronymic_modern: None, maiden_surname: None, gender: Some(sex.into()),
            rank: None, confession: None, place: place.map(Into::into), note: None, uncertain: None, age_years: None,
            marriage_order: None, kinship: None, age_months: None, age_weeks: None, age_days: None,
            age_text: None, death_cause: None,
        };
        let birth = |persons: Vec<PersonInput>| EntryInput {
            id: None, case_id: 1, section: 1, page: None, no_male: Some(1), no_female: None,
            event_day: None, event_month: None, event_year: Some(1897), rite_day: None, rite_month: None,
            rite_year: Some(1897), note: None, uncertain: None, scan_file: None, persons,
        };
        // Два тёзки «Иван Капитонов»: в Фетинине жена Анна, в Воспице — Олимпиада (дважды).
        save_entry(&conn, &birth(vec![person("father", "Иван", "Капитонов", None, Some("Фетинино"), "М"),
                                      person("mother", "Анна", "Петрова", None, Some("Фетинино"), "Ж")])).unwrap();
        for _ in 0..2 {
            save_entry(&conn, &birth(vec![person("father", "Иван", "Капитонов", None, Some("Воспица"), "М"),
                                          person("mother", "Олимпиада", "Иванова", None, Some("Воспица"), "Ж")])).unwrap();
        }
        let wife = |place: Option<&str>| -> Option<String> {
            conn.query_row(&statement("spouse_lookup").unwrap(),
                           rusqlite::named_params! { ":husband_norm": normalize("Иван Капитонов"), ":place": place },
                           |r| r.get(0)).optional().unwrap()
        };
        assert_eq!(wife(Some("Фетинино")).as_deref(), Some("Анна Петрова"), "жена из пункта мужа, а не самая частая");
        assert_eq!(wife(Some("Воспица")).as_deref(), Some("Олимпиада Иванова"));
        assert_eq!(wife(Some("Малово")), None, "в этом пункте жены нет — чужую не подставляем");
        assert_eq!(wife(None), None, "НП мужа неизвестен, жёны из разных пунктов — не угадываем");

        // Жена получила фамилию отца — строка памяти дополняется, второй нет.
        save_entry(&conn, &birth(vec![person("father", "Пётр", "Сидоров", Some("Томилин"), Some("Малово"), "М"),
                                      person("mother", "Мария", "Иванова", None, Some("Малово"), "Ж")])).unwrap();
        save_entry(&conn, &birth(vec![person("father", "Пётр", "Сидоров", Some("Томилин"), Some("Малово"), "М"),
                                      person("mother", "Мария", "Иванова", Some("Томилина"), Some("Малово"), "Ж")])).unwrap();
        // Тёзка мужа из другого пункта с женой-тёзкой: чужую строку не трогаем.
        save_entry(&conn, &birth(vec![person("father", "Пётр", "Сидоров", Some("Томилин"), Some("Воспица"), "М"),
                                      person("mother", "Мария", "Иванова", Some("Орлова"), Some("Воспица"), "Ж")])).unwrap();
        let rows: Vec<(String, i64)> = conn
            .prepare("SELECT wife_iof, uses FROM spouse_index WHERE husband_norm = ?1 AND wife_place = 'Малово'").unwrap()
            .query_map([normalize("Пётр Сидоров Томилин")], |r| Ok((r.get(0)?, r.get(1)?))).unwrap()
            .map(Result::unwrap).collect();
        assert_eq!(rows, vec![("Мария Иванова Томилина".to_string(), 2)], "одна жена, одна строка");

        // Название пункта набрано по-своему (Ctrl+Enter прямо из поля НП): в
        // память персон и пар идёт написание справочника.
        save_entry(&conn, &birth(vec![person("father", "Семён", "Иванов", None, Some("воспица"), "М"),
                                      person("mother", "Дарья", "Петрова", None, Some(" ВОСПИЦА "), "Ж")])).unwrap();
        let spelled = |sql: &str| -> String { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(spelled("SELECT place FROM person_index WHERE iof = 'Семён Иванов'"), "Воспица");
        assert_eq!(spelled("SELECT wife_place FROM spouse_index WHERE wife_iof = 'Дарья Петрова'"), "Воспица");
        assert_eq!(wife(Some("Воспица")).as_deref(), Some("Олимпиада Иванова"), "прежние пары не задеты");

        // Подсказка: строка без НП спрятана, когда у того же ИОФ есть строка с НП.
        for (place, rank) in [("", "псаломщик"), ("Борисоглебское", "псаломщик"), ("", "дьячок")] {
            conn.execute("INSERT INTO person_index (iof, iof_norm, place, rank, gender, uses) VALUES (?1, ?2, ?3, ?4, 'М', 3)",
                         rusqlite::params!["Александр Флегонтов Златоустовский",
                                           normalize("Александр Флегонтов Златоустовский"), place, rank]).unwrap();
        }
        conn.execute("INSERT INTO person_index (iof, iof_norm, place, rank, gender, uses) VALUES ('Александр Рождественский', ?1, '', 'священник', 'М', 1)",
                     [normalize("Александр Рождественский")]).unwrap();
        for (block, sql) in [("person_suggest", statement("person_suggest").unwrap()),
                             ("person_suggest_infant", statement("person_suggest_infant").unwrap())] {
            let got: Vec<(String, Option<String>, i64)> = conn
                .prepare(&sql).unwrap()
                .query_map(rusqlite::named_params! { ":prefix": "александр%", ":limit": 10, ":gender": "М" },
                           |r| Ok((r.get(0)?, r.get(1)?, r.get(4)?))).unwrap()
                .map(Result::unwrap).collect();
            assert_eq!(got, vec![
                ("Александр Флегонтов Златоустовский".to_string(), Some("Борисоглебское".to_string()), 9),
                ("Александр Рождественский".to_string(), None, 1),
            ], "{block}");
        }
        drop(conn);
        let _ = std::fs::remove_file(&path);
    }

    /// «Имя не указано» и фамилии прихода (спека 2026-10-05, часть Б).
    #[test]
    fn no_name_and_surnames() {
        let seed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../resources/seed.sqlite");
        if !seed.exists() {
            crate::seed_missing("тест пропущен");
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
        for word in ["***", "*", "—", "?", "-", "Имя", "имя", "ИМЯ", "_", "...", "нрзб", "Нрзб.", "н/д", "Н/Д",
                     "неизвестно", "Неизв", "неизв.", "нет", "Нет", "б/и", "Б/и"] {
            assert!(is_no_name(word), "{word}");
        }
        for word in ["Иван", "2", "", "Имярек", "Им", "N", "*а", "Нета", "Неизвестнов", "нд"] {
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
            rite_year: Some(1897), note: None, uncertain: None, scan_file: None,
            persons: vec![person("mother", "***", "Томилина"), person("godparent1", "Анна", "Томилина")],
        };
        save_entry(&conn, &entry).unwrap();
        let num = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(num("SELECT count(*) FROM person_index WHERE iof LIKE '***%'"), 0, "«***» в память персон не идёт");
        assert_eq!(num("SELECT count(*) FROM spouse_index WHERE wife_iof LIKE '***%'"), 0, "…и в память пар");
        assert_eq!(num("SELECT count(*) FROM usage_stat WHERE kind = 'first_name' AND value = '***'"), 0);
        assert_eq!(num("SELECT count FROM usage_stat WHERE kind = 'surname' AND scope = 'parish' AND value = 'Томилина'"), 2);
        let surnames_then = num("SELECT count(*) FROM person_mention WHERE surname = 'Томилина'");
        assert_eq!(surnames_then, 2);
        assert_eq!(seed_surnames(&conn).unwrap(), 0, "частоты фамилий уже есть — второй раз не заполняются");
        // Приход, набранный до подсказки фамилий: частоты считаются по записям.
        conn.execute("DELETE FROM usage_stat WHERE kind = 'surname'", []).unwrap();
        assert_eq!(seed_surnames(&conn).unwrap(), 1);
        assert_eq!(num("SELECT count FROM usage_stat WHERE kind = 'surname' AND scope = 'parish' AND value_norm = 'томилина'"), 2);
        // Отметка «такой фамилии ещё не было»: набранная фамилия известна в
        // обеих формах, опечатка — нет.
        assert!(!parse_iof_in(&conn, "Иван Петров Томилин").unwrap().surname_new, "мужская форма набранной «Томилина»");
        assert!(!parse_iof_in(&conn, "Анна Иванова Томилина").unwrap().surname_new);
        assert!(parse_iof_in(&conn, "Иван Петров Тамилин").unwrap().surname_new, "опечатка видна");
        assert!(!parse_iof_in(&conn, "Иван Петров").unwrap().surname_new, "нет фамилии — нет отметки");
        for known in ["Толстой", "Белый", "Ивановъ"] {
            conn.execute("INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count) VALUES ('surname', 'global', '', ?1, ?2, 1)",
                         rusqlite::params![known, normalize(known)]).unwrap();
        }
        for typed in ["Анна Иванова Толстая", "Анна Иванова Белая", "Иван Петров Иванов", "Анна Иванова Иванова"] {
            assert!(!parse_iof_in(&conn, typed).unwrap().surname_new, "{typed}");
        }
        conn.execute("DELETE FROM usage_stat WHERE kind = 'surname' AND value IN ('Толстой', 'Белый', 'Ивановъ')", []).unwrap();
        assert_eq!(surname_pair("Томский"), ("томский".into(), "томская".into()));
        assert_eq!(surname_pair("Томская"), ("томский".into(), "томская".into()));
        assert_eq!(surname_pair("Ивановъ"), ("иванов".into(), "иванова".into()));
        assert_eq!(surname_pair("Шевченко"), ("шевченко".into(), "шевченко".into()));
        assert_eq!(surname_pair("Толстой"), ("толстой".into(), "толстая".into()));
        assert_eq!(surname_pair("Белая"), ("белый".into(), "белая".into()));
        // Звание по умолчанию — по роли и полу, не по общим частотам перечня.
        let ranked = |role: &str, first: &str, gender: &str, rank: &str| PersonInput {
            rank: Some(rank.into()), gender: Some(gender.into()), confession: Some("православного".into()),
            ..person(role, first, "Томилина")
        };
        for (i, people) in [
            vec![ranked("mother", "Анна", "Ж", "законная жена его"), ranked("godparent1", "Мария", "Ж", "крестьянская девица")],
            vec![ranked("mother", "Дарья", "Ж", "законная жена его"), ranked("godparent1", "Иван", "М", "крестьянин")],
            vec![ranked("mother", "Анна", "Ж", "законная жена его"), ranked("godparent2", "Мария", "Ж", "крестьянская девица")],
        ].into_iter().enumerate() {
            save_entry(&conn, &EntryInput { id: None, case_id: 1, section: 1, page: None, no_male: None,
                no_female: Some(i as i64 + 2), event_day: None, event_month: None, event_year: Some(1897),
                rite_day: None, rite_month: None, rite_year: Some(1897), note: None, uncertain: None, scan_file: None, persons: people }).unwrap();
        }
        assert_eq!(default_rank(&conn, "godparent%", Some("Ж")).unwrap().as_deref(), Some("крестьянская девица"));
        assert_eq!(default_rank(&conn, "godparent%", Some("М")).unwrap().as_deref(), Some("крестьянин"));
        assert_eq!(default_rank(&conn, "witness%", Some("М")).unwrap(), None, "поручителей ещё нет — подставлять нечего");
        // Вероисповедание идёт в частоты; у набранного раньше — считается один
        // раз на приход, имеющиеся частоты не занижаются, строки шаблона Excel
        // в частоты не идут.
        assert_eq!(num("SELECT count FROM usage_stat WHERE kind = 'confession' AND scope = 'parish' AND value = 'православного'"), 6);
        conn.execute("DELETE FROM usage_stat WHERE kind = 'confession'", []).unwrap();
        conn.execute_batch(
            "INSERT INTO place (id, name, name_norm, np_type, guberniya, uyezd, volost, origin)
             VALUES (7001, 'Лодзь', 'лодзь', 'г.', 'Петровская', 'Лодзинский', NULL, 'seed'),
                    (7002, 'Выселки Тестовые', 'выселки тестовые', 'д.', 'Костромская', 'Макарьевский', 'Завражная', 'user');
             UPDATE person_mention SET place_id = 7001 WHERE id = (SELECT min(id) FROM person_mention);
             INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count)
             VALUES ('uyezd', 'global', '', 'Макарьевский', 'макарьевский', 5);").unwrap();
        seed_usage(&conn).unwrap();
        assert_eq!(num("SELECT count FROM usage_stat WHERE kind = 'confession' AND scope = 'global' AND value = 'православного'"), 6);
        assert_eq!(num("SELECT count FROM usage_stat WHERE kind = 'uyezd' AND scope = 'global' AND value = 'Макарьевский'"), 5,
                   "частота от «Сохранить дело» не занижена подсчётом по пунктам");
        assert_eq!(num("SELECT count(*) FROM usage_stat WHERE kind = 'volost' AND value = 'Завражная'"), 2, "уезд был — волость всё равно посчитана");
        assert_eq!(num("SELECT count(*) FROM usage_stat WHERE value IN ('Петровская', 'Лодзинский')"), 0, "строки шаблона в частоты не идут");
        assert_eq!(num("SELECT count(*) FROM usage_stat WHERE kind = 'np_type' AND value = 'г.'"), 2, "а тип занятого пункта — идёт");
        conn.execute("DELETE FROM usage_stat WHERE kind = 'confession'", []).unwrap();
        seed_usage(&conn).unwrap();
        assert_eq!(num("SELECT count(*) FROM usage_stat WHERE kind = 'confession'"), 0, "второй раз не заполняется");
        conn.execute("UPDATE person_mention SET place_id = NULL WHERE place_id = 7001", []).unwrap();
        // Поля карточки пункта: частота растёт, самое частое — первым.
        for _ in 0..2 {
            remember_card(&conn, "volost", Some("Завражная")).unwrap();
        }
        remember_card(&conn, "volost", Some("Абрамовская")).unwrap();
        assert_eq!(num("SELECT count FROM usage_stat WHERE kind = 'volost' AND scope = 'global' AND value = 'Завражная'"), 3);
        // В выгрузке Familio имени у «***» нет.
        crate::export::familio_rows(&conn).unwrap();
        assert_eq!(num("SELECT count(*) FROM x_person WHERE role_code = 'mother' AND first_m IS NULL"), 1);
        assert_eq!(num("SELECT count(*) FROM x_person WHERE role_code = 'godparent1' AND first_m = 'Анна'"), 1);
        // Запись без года с убранным делом не падает.
        let mut loose = EntryInput { id: None, case_id: 999, section: 2, page: None, no_male: None, no_female: None,
            event_day: None, event_month: None, event_year: None, rite_day: None, rite_month: None, rite_year: None,
            note: None, uncertain: None, scan_file: None, persons: vec![] };
        let moved = save_entry(&conn, &loose).unwrap();
        assert_eq!((moved.case_id, moved.fallback_case, moved.fallback_year), (1, true, Some(1897)));
        // Своё дело на месте — оговорки нет.
        loose.case_id = 1;
        let own = save_entry(&conn, &loose).unwrap();
        assert_eq!((own.case_id, own.fallback_case, own.fallback_year), (1, false, None));
        std::mem::drop(conn);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn wife_rank_by_husband() {
        // Примеры — из перечня званий заказчика.
        for (husband, wife) in [
            (Some("крестьянин"), "крестьянская жена"), (Some("крестьянский сын"), "крестьянская жена"),
            (Some("Крестьянин того же дому"), "крестьянская жена"), (Some("мещанин"), "мещанская жена"),
            (Some("солдат"), "солдатская жена"), (Some("отставной рядовой"), "солдатская жена"),
            (Some("уволенный в запас армии"), "солдатская жена"), (Some("запасной нижний чин"), "солдатская жена"),
            (Some("отставной унтер-офицер"), "солдатская жена"), (Some("запасной бомбардир наводчик"), "солдатская жена"),
            (Some("состоящий в военной службе"), "солдатская жена"),
            // Необычное звание — программа не выдумывает: просто «жена».
            (Some("священник"), "жена"), (Some("потомственный почетный гражданин"), "жена"),
            (Some("дворянин"), "жена"), (Some("умерший крестьянин"), "жена"), (Some("умерший солдат"), "жена"),
            (Some("отставной фельдшер"), "жена"), (Some("отставной церковник"), "жена"),
            (Some(""), "жена"), (None, "жена"),
        ] {
            assert_eq!(wife_rank(husband), wife, "муж: {husband:?}");
        }
    }
}
