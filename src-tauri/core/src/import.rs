//! Импорт записей из Excel-индексатора (спека 2026-10-02, п. 4).
//!
//! Роман 02.10.2026: «взять свой старый Excel-файл, загрузить его в GenMetric,
//! чтобы старая база стала отправной точкой. Далее я продолжаю вносить новые
//! записи уже через интерфейс».
//!
//! Читаются листы записей индексатора — «1», «2», «3» (в нашей выгрузке в
//! Excel они же называются «Рождения», «Браки», «Смерти»): строка 1 — шапка,
//! строка 2 — подписи, с третьей — записи. Из листа «МК» берётся только
//! родство родственника умершего («умершего отец»): на листе «3» его нет.
//!
//! Каждая запись сохраняется **тем же путём, что и набранная руками**
//! (`records::save_entry_in_tx`): тот же разбор ИОФ, пункты заводятся, звания
//! попадают в перечни, память персон и частоты подсказок растут. Поэтому
//! после импорта подсказки «прогреты», а выгрузка не отличает импортированное
//! от набранного.

use std::collections::{BTreeMap, HashMap};

use rusqlite::Connection;
use serde::Serialize;

use crate::age::parse_age;
use crate::records::{parse_iof_in, save_entry_in_tx, Case, EntryInput, PersonInput};
use crate::statement;
use crate::xlsx::read_sheet;

type Row = BTreeMap<u32, String>;
type Sheet = BTreeMap<u32, Row>;

/// Что нашлось в файле — до импорта: человек видит, тот ли это файл.
#[derive(Serialize, Debug, Default)]
pub struct Inspect {
    pub village: String,
    pub church: String,
    pub indexer: String,
    pub births: usize,
    pub marriages: usize,
    pub deaths: usize,
    pub years: Vec<i64>,
}

/// Итог импорта — на экран (спека, п. 4.4): пропущенная строка — не молча.
#[derive(Serialize, Debug, Default)]
pub struct ImportReport {
    pub births: usize,
    pub marriages: usize,
    pub deaths: usize,
    pub persons: i64,
    pub places: i64,
    pub years: Vec<i64>,
    /// Упоминаний с именем, которого нет в словаре: перенесены как в Excel.
    pub unknown_names: usize,
    /// Первые из таких имён (разные), с числом упоминаний.
    pub unknown_examples: Vec<String>,
    /// Упоминаний, где второе слово похоже на отчество, но в словаре его нет:
    /// оно перенесено как часть фамилии (в форме программа спросила бы).
    pub unknown_patronymics: usize,
    pub unknown_patronymic_examples: Vec<String>,
    /// «лист 2, строка 8: …» — почему строка не перенесена.
    pub skipped: Vec<String>,
    /// Строки, перенесённые с оговоркой (год в дате не совпал с годом книги).
    pub notes: Vec<String>,
}

const SHEETS: [(&str, &str, &str); 3] = [("1", "Рождения", "рождения"), ("2", "Браки", "бракосочетания"), ("3", "Смерти", "смерти")];

// ----------------------------------------------------------------------------
//  Ячейки
// ----------------------------------------------------------------------------

/// Текст ячейки. Число `read_sheet` отдаёт с «#»: «#873», «#1.5».
fn cell(row: &Row, col: u32) -> String {
    let v = row.get(&col).map(|s| s.trim()).unwrap_or("");
    match v.strip_prefix('#') {
        Some(n) => match n.parse::<f64>() {
            Ok(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
            _ => n.replace('.', ","),
        },
        None => v.split_whitespace().collect::<Vec<_>>().join(" "),
    }
}

fn opt(row: &Row, col: u32) -> Option<String> {
    Some(cell(row, col)).filter(|v| !v.is_empty() && v != "-")
}

/// Название пункта. Ноль в текстовой колонке — след пустой формулы Excel,
/// а не название.
fn place(row: &Row, col: u32) -> Option<String> {
    opt(row, col).filter(|v| v != "0")
}

/// Имени в книге нет, а в ячейке — заглушка: «***», «?», «Имя». У умершего
/// это запись «личность не установлена» (в форме — флажок).
fn no_name(iof: &str) -> bool {
    !iof.chars().any(|c| c.is_alphabetic()) || iof.trim().to_lowercase() == "имя"
}

fn int(row: &Row, col: u32) -> Option<i64> {
    cell(row, col).parse::<i64>().ok()
}

/// Дата индексатора: «05.01.1886», «?.06.1887», «??.??.1887». Дата после
/// 1900 года Excel мог сохранить числом — тогда это счёт дней.
fn date(row: &Row, col: u32) -> (Option<i64>, Option<i64>, Option<i64>) {
    let raw = row.get(&col).map(|s| s.trim()).unwrap_or("");
    if let Some(n) = raw.strip_prefix('#') {
        if let Ok(serial) = n.parse::<f64>() {
            // Счёт дней Excel от 30.12.1899 → гражданская дата. До 1 марта
            // 1900 года Excel считает на день иначе (у него есть 29.02.1900).
            let days = serial.floor() as i64;
            let z = days + if days < 61 { 1 } else { 0 } - 25569 + 719468;
            let era = z.div_euclid(146097);
            let doe = z.rem_euclid(146097);
            let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
            return (Some(d), Some(m), Some(y));
        }
    }
    let parts: Vec<&str> = raw.split('.').map(str::trim).collect();
    if parts.len() != 3 {
        return (None, None, None);
    }
    let num = |s: &str, max: i64| s.parse::<i64>().ok().filter(|n| *n >= 1 && *n <= max);
    (num(parts[0], 31), num(parts[1], 12), num(parts[2], 9999))
}

/// «Ф.56 Оп.31 Д.12» → фонд, опись, дело.
fn split_fod(text: &str) -> (Option<String>, Option<String>, Option<String>) {
    let clean = |s: &str| Some(s.trim().trim_matches(',').trim().to_string()).filter(|v| !v.is_empty());
    let (mut fond, mut opis, mut delo) = (None, None, None);
    let op = text.find("Оп.");
    let d = text.rfind("Д.").filter(|p| op.map(|o| *p > o).unwrap_or(true));
    if let Some(f) = text.find("Ф.") {
        let end = op.or(d).unwrap_or(text.len());
        if end > f {
            fond = clean(&text[f + "Ф.".len()..end]);
        }
    }
    if let Some(o) = op {
        opis = clean(&text[o + "Оп.".len()..d.unwrap_or(text.len())]);
    }
    if let Some(p) = d {
        delo = clean(&text[p + "Д.".len()..]);
    }
    (fond, opis, delo)
}

/// «Церковь: … Село: … Уезд: … Губерния: …» → четыре части.
fn split_mk(text: &str) -> [Option<String>; 4] {
    let marks = ["Церковь:", "Село:", "Уезд:", "Губерния:"];
    let pos: Vec<Option<usize>> = marks.iter().map(|m| text.find(m)).collect();
    let mut out: [Option<String>; 4] = [None, None, None, None];
    for i in 0..4 {
        if let Some(start) = pos[i] {
            let from = start + marks[i].len();
            let to = pos.iter().flatten().filter(|p| **p > start).min().copied().unwrap_or(text.len());
            out[i] = Some(text[from..to].trim().to_string()).filter(|v| !v.is_empty());
        }
    }
    out
}

/// Пол по родству — как `kinGender` в формах.
fn kin_gender(kin: &str) -> Option<String> {
    let k = kin.trim().to_lowercase();
    if ["отец", "брат", "дядя", "дед", "супруг", "муж", "сын"].contains(&k.as_str()) {
        Some("М".into())
    } else if ["мать", "сестра", "тетка", "тётка", "бабка", "супруга", "жена", "дочь"].contains(&k.as_str()) {
        Some("Ж".into())
    } else {
        None
    }
}

fn is_kinship(word: &str) -> bool {
    kin_gender(word).is_some()
}

// ----------------------------------------------------------------------------
//  Чтение файла
// ----------------------------------------------------------------------------

struct Book {
    sheets: [Sheet; 3],
    /// (№ пп на листе «3») → родство родственника умершего, из листа «МК».
    death_kin: HashMap<i64, String>,
    indexer: String,
}

fn load(bytes: &[u8]) -> Result<Book, String> {
    let mut sheets: Vec<Sheet> = Vec::new();
    for (i, (name, ours, word)) in SHEETS.iter().enumerate() {
        let sheet = read_sheet(bytes, name)
            .or_else(|_| read_sheet(bytes, ours))
            .map_err(|_| format!("это не индексатор метрических книг: в файле нет листа «{name}»"))?;
        // Шапка: «ИОФ» над колонкой O и подпись раздела в колонке J второй строки.
        let head = |r: u32, c: u32| sheet.get(&r).map(|row| cell(row, c)).unwrap_or_default();
        if head(1, 15) != "ИОФ" || !head(2, 10).to_lowercase().contains(word) {
            return Err(format!(
                "лист «{name}» не похож на лист индексатора: в шапке ждём «ИОФ» в колонке O и «{word}» в колонке J, \
                 а там «{}» и «{}»", head(1, 15), head(2, 10)));
        }
        let _ = i;
        sheets.push(sheet);
    }
    let mut death_kin = HashMap::new();
    if let Ok(mk) = read_sheet(bytes, "МК") {
        for (r, row) in &mk {
            if *r < 3 || cell(row, 19) != "3" {
                continue;
            }
            let role = cell(row, 4);
            if let (Some(kin), Some(n)) = (role.strip_prefix("умершего "), int(row, 14)) {
                if !kin.trim().is_empty() {
                    death_kin.insert(n, kin.trim().to_string());
                }
            }
        }
    }
    // «Проиндексировал: Роман Чистов 2026/8/24» — последней строкой листа.
    let mut indexer = String::new();
    for sheet in &sheets {
        for row in sheet.values().rev().take(3) {
            for c in 1..=6 {
                if let Some(rest) = cell(row, c).strip_prefix("Проиндексировал:") {
                    let words: Vec<&str> = rest.split_whitespace()
                        .filter(|w| !w.chars().any(|ch| ch.is_ascii_digit()))
                        .collect();
                    if indexer.is_empty() {
                        indexer = words.join(" ");
                    }
                }
            }
        }
    }
    let sheets: [Sheet; 3] = sheets.try_into().map_err(|_| "листов должно быть три".to_string())?;
    Ok(Book { sheets, death_kin, indexer })
}

/// Строка записи: с третьей, с номером в колонке A или хоть с одним именем.
fn is_record(r: u32, row: &Row) -> bool {
    r >= 3 && (int(row, 1).is_some() || [12u32, 15, 21].iter().any(|c| opt(row, *c).is_some()))
        && !(1..=6).any(|c| cell(row, c).starts_with("Проиндексировал:"))
}

fn book_year(section: usize, row: &Row) -> Option<i64> {
    int(row, 5).filter(|y| (1500..=2100).contains(y))
        .or_else(|| if section == 1 { date(row, 10).2 } else { date(row, 11).2.or(date(row, 10).2) })
}

pub fn inspect(bytes: &[u8]) -> Result<Inspect, String> {
    let book = load(bytes)?;
    let mut out = Inspect { indexer: book.indexer.clone(), ..Default::default() };
    let mut years = std::collections::BTreeSet::new();
    for (i, sheet) in book.sheets.iter().enumerate() {
        let mut n = 0;
        for (r, row) in sheet {
            if !is_record(*r, row) {
                continue;
            }
            n += 1;
            if let Some(y) = book_year(i, row) {
                years.insert(y);
            }
            if out.village.is_empty() {
                let mk = split_mk(&cell(row, 4));
                out.church = mk[0].clone().unwrap_or_default();
                out.village = mk[1].clone().unwrap_or_default();
            }
        }
        match i {
            0 => out.births = n,
            1 => out.marriages = n,
            _ => out.deaths = n,
        }
    }
    out.years = years.into_iter().collect();
    if out.births + out.marriages + out.deaths == 0 {
        return Err("в файле нет ни одной записи".into());
    }
    Ok(out)
}

// ----------------------------------------------------------------------------
//  Перенос
// ----------------------------------------------------------------------------

struct Importer<'a> {
    conn: &'a Connection,
    unknown: BTreeMap<String, usize>,
    unknown_total: usize,
    unknown_patr: BTreeMap<String, usize>,
    unknown_patr_total: usize,
}

struct Who {
    role: &'static str,
    order: i64,
    iof: Option<String>,
    place: Option<String>,
    rank: Option<String>,
    confession: Option<String>,
    note: Option<String>,
    gender: Option<String>,
}

impl Importer<'_> {
    /// Персона — как `payload()` формы: разбор ИОФ той же функцией, что поле.
    fn person(&mut self, w: Who) -> Result<PersonInput, String> {
        let mut iof = w.iof.clone().unwrap_or_default();
        // Девичья фамилия — из скобок: «Анна Иванова (Петрова)».
        let mut maiden = None;
        if let (Some(a), Some(b)) = (iof.find('('), iof.rfind(')')) {
            if b > a {
                maiden = Some(iof[a + 1..b].trim().to_string()).filter(|v| !v.is_empty());
                iof = format!("{} {}", iof[..a].trim(), iof[b + 1..].trim()).trim().to_string();
            }
        }
        let parsed = parse_iof_in(self.conn, &iof)?;
        if !iof.is_empty() && !parsed.known_name {
            self.unknown_total += 1;
            *self.unknown.entry(parsed.first_name.clone().unwrap_or_default()).or_default() += 1;
        }
        if let Some(word) = &parsed.patr_unknown {
            self.unknown_patr_total += 1;
            *self.unknown_patr.entry(word.clone()).or_default() += 1;
        }
        Ok(PersonInput {
            role_code: w.role.to_string(),
            sort_order: w.order,
            surname: parsed.surname,
            first_name: parsed.first_name,
            patronymic: parsed.patronymic,
            surname_modern: None,
            first_name_modern: parsed.first_name_modern,
            patronymic_modern: parsed.patronymic_modern,
            maiden_surname: maiden,
            gender: w.gender.or(parsed.gender),
            rank: w.rank,
            confession: w.confession,
            place: w.place,
            note: w.note,
            uncertain: None,
            age_years: None,
            marriage_order: None,
            kinship: None,
            age_months: None,
            age_weeks: None,
            age_days: None,
            age_text: None,
            death_cause: None,
        })
    }

    /// Причт: три тройки колонок AU…BC — ИОФ, звание, примечание.
    fn clergy(&mut self, row: &Row, persons: &mut Vec<PersonInput>) -> Result<(), String> {
        for (i, c) in [47u32, 50, 53].iter().enumerate() {
            if let Some(iof) = opt(row, *c) {
                let role = ["clergy1", "clergy2", "clergy3"][i];
                persons.push(self.person(Who {
                    role, order: 100 + 10 * i as i64, iof: Some(iof), place: None,
                    rank: opt(row, c + 1), confession: None, note: opt(row, c + 2), gender: Some("М".into()),
                })?);
            }
        }
        Ok(())
    }
}

/// «Первым браком; примечание» → (каким браком, примечание). Так же пишет
/// наша выгрузка в Excel; у индексатора в колонке только «каким браком».
fn split_first(text: Option<String>, is_first: impl Fn(&str) -> bool) -> (Option<String>, Option<String>) {
    let Some(text) = text else { return (None, None) };
    let (head, tail) = match text.split_once(';') {
        Some((h, t)) => (h.trim().to_string(), Some(t.trim().to_string()).filter(|v| !v.is_empty())),
        None => (text.clone(), None),
    };
    if is_first(&head) { (Some(head), tail) } else { (None, Some(text)) }
}

/// Переносит записи файла в открытую базу прихода. Вызывается внутри
/// транзакции вызывающего (parish::create_with): ошибка — ничего не остаётся.
pub fn import_into(conn: &Connection, bytes: &[u8]) -> Result<ImportReport, String> {
    let book = load(bytes)?;
    let mut report = ImportReport::default();
    let count = |sql: &str| -> Result<i64, String> { conn.query_row(sql, [], |r| r.get(0)).map_err(|e| e.to_string()) };
    let places_before = count("SELECT count(*) FROM place")?;

    // Дела по годам: у индексатора реквизиты стоят в каждой строке, у нас —
    // дело на год книги. Берётся первое непустое значение года.
    let mut cases: BTreeMap<i64, Case> = BTreeMap::new();
    for (i, sheet) in book.sheets.iter().enumerate() {
        for (r, row) in sheet {
            if !is_record(*r, row) {
                continue;
            }
            let Some(year) = book_year(i, row) else { continue };
            let case = cases.entry(year).or_insert_with(|| Case { year: Some(year), ..Default::default() });
            let mk = split_mk(&cell(row, 4));
            let (fond, opis, delo) = split_fod(&cell(row, 3));
            let fill = |slot: &mut Option<String>, v: Option<String>| {
                if slot.is_none() {
                    *slot = v;
                }
            };
            fill(&mut case.archive, opt(row, 2));
            fill(&mut case.fond, fond);
            fill(&mut case.opis, opis);
            fill(&mut case.delo, delo);
            let [church, village, uyezd, guberniya] = mk;
            fill(&mut case.church, church);
            fill(&mut case.village, village);
            fill(&mut case.uyezd, uyezd);
            fill(&mut case.guberniya, guberniya);
        }
    }
    if cases.is_empty() {
        return Err("в файле нет ни одной записи с годом".into());
    }
    // Церковь, село, уезд, губерния — свойства прихода: год без них (пустая
    // колонка «МК») получает их от соседнего.
    let parish_of = cases.values().find(|c| c.village.is_some() || c.church.is_some()).cloned().unwrap_or_default();
    let indexer = Some(book.indexer.clone()).filter(|v| !v.is_empty());
    let upsert = statement("case_upsert")?;
    let mut parish_key = String::new();
    let mut first_case = 0;
    for (n, case) in cases.values_mut().enumerate() {
        if case.village.is_none() && case.church.is_none() {
            case.church = parish_of.church.clone();
            case.village = parish_of.village.clone();
            case.uyezd = parish_of.uyezd.clone();
            case.guberniya = parish_of.guberniya.clone();
        }
        case.indexer = indexer.clone();
        case.id = n as i64 + 1;
        if n == 0 {
            parish_key = case.parish_key();
            first_case = case.id;
        }
        conn.execute(&upsert, rusqlite::named_params! {
            ":id": case.id, ":archive": case.archive, ":fond": case.fond, ":opis": case.opis,
            ":delo": case.delo, ":church": case.church, ":village": case.village, ":uyezd": case.uyezd,
            ":guberniya": case.guberniya, ":year": case.year, ":parish_key": case.parish_key(),
            ":indexer": case.indexer,
        }).map_err(|e| e.to_string())?;
    }
    report.years = cases.keys().copied().collect();

    let mut imp = Importer {
        conn, unknown: BTreeMap::new(), unknown_total: 0, unknown_patr: BTreeMap::new(), unknown_patr_total: 0,
    };
    for (i, sheet) in book.sheets.iter().enumerate() {
        let section = i as i64 + 1;
        let name = SHEETS[i].0;
        for (r, row) in sheet {
            if !is_record(*r, row) {
                continue;
            }
            let year = book_year(i, row);
            let (ed, em, ey) = date(row, 10);
            let (rd, rm, ry) = date(row, 11);
            // Дата, которую не понять или которой не бывает, — не молча.
            for (c, parts) in [(10u32, (ed, em, ey)), (11, (rd, rm, ry))] {
                let raw = cell(row, c);
                if raw.is_empty() || (section == 2 && c == 11) {
                    continue;
                }
                let odd = match parts {
                    (None, None, None) => Some("не разобрана — в записи даты нет"),
                    (Some(d), Some(m), _) if d > [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][m as usize - 1] =>
                        Some("такого дня в месяце нет — перенесена как есть"),
                    (_, _, Some(y)) if !(1500..=2100).contains(&y) => Some("год вне разумных пределов — перенесена как есть"),
                    _ => None,
                };
                if let Some(why) = odd {
                    report.notes.push(format!("лист «{name}», строка {r}: дата «{raw}» {why}"));
                }
            }
            let mut persons: Vec<PersonInput> = Vec::new();
            let who = |role: &'static str, order: i64, c: u32, gender: Option<&str>| Who {
                role, order, iof: opt(row, c + 2), place: place(row, c), rank: opt(row, c + 1),
                confession: opt(row, c + 3), note: opt(row, c + 4), gender: gender.map(str::to_string),
            };
            // Восприемники и поручители: ИОФ, НП, звание, примечание.
            let four = |role: &'static str, order: i64, c: u32| Who {
                role, order, iof: opt(row, c), place: place(row, c + 1), rank: opt(row, c + 2),
                confession: None, note: opt(row, c + 3), gender: None,
            };
            let (mut no_male, mut no_female) = (int(row, 8), int(row, 9));
            // Звание, место или примечание без имени: в форме персона без ИОФ
            // не сохраняется, поэтому у рождений они уходят в примечание
            // записи (оно в форме есть), у браков и смертей — в отчёт.
            let mut orphans: Vec<String> = Vec::new();
            let mut orphan = |title: &str, iof: u32, cols: &[u32]| {
                if opt(row, iof).is_some() {
                    return;
                }
                let parts: Vec<String> = cols.iter().filter_map(|c| opt(row, *c)).collect();
                if !parts.is_empty() {
                    orphans.push(format!("{title}: {}", parts.join(", ")));
                }
            };
            match section {
                1 => {
                    // Пол ребёнка — по колонке счёта; счёта нет — по имени.
                    let sex = if no_female.is_some() { Some("Ж") } else if no_male.is_some() { Some("М") } else { None };
                    if let Some(child) = opt(row, 12) {
                        persons.push(imp.person(Who {
                            role: "child", order: 10, iof: Some(child), place: None, rank: None,
                            confession: None, note: None, gender: sex.map(str::to_string),
                        })?);
                    }
                    if opt(row, 15).is_some() {
                        persons.push(imp.person(who("father", 20, 13, Some("М")))?);
                    }
                    orphan("отец", 15, &[13, 14, 17]);
                    if opt(row, 21).is_some() {
                        persons.push(imp.person(who("mother", 30, 19, Some("Ж")))?);
                    }
                    orphan("мать", 21, &[19, 20, 23]);
                    for (k, c) in [31u32, 35, 39, 43].iter().enumerate() {
                        orphan(["восприемник 1", "восприемник 2", "восприемник 3", "восприемник 4"][k], *c, &[c + 1, c + 2, c + 3]);
                        if opt(row, *c).is_some() {
                            let role = ["godparent1", "godparent2", "godparent3", "godparent4"][k];
                            persons.push(imp.person(four(role, 40 + 10 * k as i64, *c))?);
                        }
                    }
                }
                2 => {
                    no_female = None;
                    let order_word = |s: &str| s.to_lowercase().ends_with("браком");
                    for (role, order, c, gender) in [("groom", 10, 13u32, "М"), ("bride", 20, 19, "Ж")] {
                        if opt(row, c + 2).is_none() {
                            continue;
                        }
                        let mut w = who(role, order, c, Some(gender));
                        let (marriage_order, note) = split_first(w.note.take(), order_word);
                        w.note = note;
                        let mut p = imp.person(w)?;
                        p.marriage_order = marriage_order;
                        p.age_years = int(row, c + 5);
                        persons.push(p);
                    }
                    // Родственник пишется и без имени — как в форме: «отец»
                    // стоит у всех записей, а имя — у немногих.
                    for (role, order, c) in [("groom_relative", 30, 25u32), ("bride_relative", 50, 28)] {
                        let kinship = opt(row, c);
                        if kinship.is_none() && opt(row, c + 1).is_none() {
                            continue;
                        }
                        let mut p = imp.person(Who {
                            role, order, iof: opt(row, c + 1), place: None, rank: opt(row, c + 2),
                            confession: None, note: None,
                            gender: kinship.as_deref().and_then(kin_gender),
                        })?;
                        p.kinship = kinship;
                        persons.push(p);
                    }
                    let witness = [(31u32, "witness1", 60), (35, "witness2", 70), (39, "witness3", 80),
                                   (43, "witness4", 90), (56, "witness5", 92), (60, "witness6", 94)];
                    for (k, (c, role, order)) in witness.into_iter().enumerate() {
                        orphan(["поручитель 1", "поручитель 2", "поручитель 3", "поручитель 4", "поручитель 5", "поручитель 6"][k],
                               c, &[c + 1, c + 2]); // «по жениху» без имени — заготовка, не данные
                        if opt(row, c).is_some() {
                            persons.push(imp.person(four(role, order, c))?);
                        }
                    }
                }
                _ => {
                    let sex = if no_female.is_some() { Some("Ж") } else if no_male.is_some() { Some("М") } else { None };
                    let mut w = who("deceased", 10, 13, sex);
                    if let Some(stub) = w.iof.clone().filter(|v| no_name(v)) {
                        w.iof = None;
                        report.notes.push(format!(
                            "лист «{name}», строка {r}: вместо имени умершего «{stub}» — записано как «личность не установлена»"));
                    }
                    // «Прим.» умершего у индексатора — «от чего умер».
                    let (cause, note) = split_first(w.note.take(), |_| true);
                    w.note = note;
                    let has_deceased = w.iof.is_some() || w.rank.is_some() || cause.is_some() || opt(row, 18).is_some();
                    if has_deceased {
                        let mut p = imp.person(w)?;
                        p.death_cause = cause;
                        if let Some(age) = opt(row, 18) {
                            // Возраст — как в книге и разобранный, тем же
                            // разбором, что в форме (age.rs = age.ts).
                            if let Some(a) = parse_age(&age) {
                                p.age_years = a.years;
                                p.age_months = a.months;
                                p.age_weeks = a.weeks;
                                p.age_days = a.days;
                            }
                            p.age_text = Some(age);
                        }
                        persons.push(p);
                    }
                    orphan("родственник умершего", 21, &[19, 20, 23]);
                    if opt(row, 21).is_some() {
                        let mut w = who("deceased_relative", 40, 19, None);
                        // Родство: в нашей выгрузке — первым в «Прим.»; у
                        // индексатора — на листе «МК»; нет нигде — «отец».
                        let (kin_note, note) = split_first(w.note.take(), is_kinship);
                        let known = kin_note.or_else(|| int(row, 1).and_then(|n| book.death_kin.get(&n).cloned()));
                        w.note = note;
                        w.gender = None;
                        let mut p = imp.person(w)?;
                        // Родство говорит одно, имя — другое («отец Татьяна»):
                        // переносим как в Excel, но не молча.
                        if let (Some(kin), Some(by_name)) = (known.as_deref(), p.gender.clone()) {
                            if let Some(by_kin) = kin_gender(kin) {
                                if by_kin != by_name {
                                    report.notes.push(format!(
                                        "лист «{name}», строка {r}: родство «{kin}», а имя «{}» — {} — перенесено как в Excel, проверьте",
                                        p.first_name.clone().unwrap_or_default(),
                                        if by_name == "Ж" { "женское" } else { "мужское" }));
                                }
                                p.gender = Some(by_kin);
                            }
                        }
                        // Родства нет нигде — по полу имени: женщине «отец»
                        // не пишем (ревьюер 02.10.2026), и говорим об этом.
                        let kinship = known.unwrap_or_else(|| {
                            let guess = if p.gender.as_deref() == Some("Ж") { "мать" } else { "отец" };
                            report.notes.push(format!(
                                "лист «{name}», строка {r}: родство родственника умершего не указано — записано «{guess}»"));
                            guess.to_string()
                        });
                        if p.gender.is_none() {
                            p.gender = kin_gender(&kinship);
                        }
                        p.kinship = Some(kinship);
                        persons.push(p);
                    }
                }
            }
            let _ = &mut orphan;
            if persons.is_empty() {
                // Строка с номером, но без единого имени — заготовка индексатора.
                if (2..=55).any(|c| c != 4 && c != 5 && c != 6 && opt(row, c).is_some()) {
                    report.skipped.push(format!("лист «{name}», строка {r}: нет ни одного имени — не перенесена"));
                }
                continue;
            }
            imp.clergy(row, &mut persons)?;

            // Год книги — колонка «Год»; у рождений и смертей это год обряда
            // (как в форме), у браков — год венчания.
            let (event_year, rite_year) = if section == 2 {
                (ey.or(year), None)
            } else {
                (ey.or(year), year.or(ry))
            };
            if event_year.is_none() && rite_year.is_none() {
                report.notes.push(format!(
                    "лист «{name}», строка {r}: нет ни года, ни дат — запись перенесена без года; \
                     в списках по годам её не видно, в выгрузке она идёт только со всем приходом"));
            }
            let in_date = if section == 2 { ey } else { ry };
            if let (Some(y), Some(d)) = (year, in_date) {
                if y != d {
                    report.notes.push(format!(
                        "лист «{name}», строка {r}: год в дате {d}, а год книги {y} — запись отнесена к {}",
                        if section == 2 { d } else { y }));
                }
            }
            let entry_note = if section == 1 && !orphans.is_empty() { Some(orphans.join("; ")) } else { None };
            if !orphans.is_empty() {
                report.notes.push(if section == 1 {
                    format!("лист «{name}», строка {r}: «{}» — без имени; перенесено в примечание записи", orphans.join("; "))
                } else {
                    format!("лист «{name}», строка {r}: «{}» — без имени; не перенесено", orphans.join("; "))
                });
            }
            let entry = EntryInput {
                id: None,
                case_id: first_case,
                section,
                page: opt(row, 7),
                no_male: no_male.take(),
                no_female,
                event_day: ed,
                event_month: em,
                event_year,
                rite_day: if section == 2 { None } else { rd },
                rite_month: if section == 2 { None } else { rm },
                rite_year,
                note: entry_note,
                uncertain: None,
                persons,
            };
            save_entry_in_tx(conn, &entry, &parish_key)
                .map_err(|e| format!("лист «{name}», строка {r}: {e}"))?;
            match section {
                1 => report.births += 1,
                2 => report.marriages += 1,
                _ => report.deaths += 1,
            }
        }
    }

    report.persons = count("SELECT count(*) FROM person_mention WHERE role_code NOT LIKE 'clergy%'")?;
    report.places = count("SELECT count(*) FROM place")? - places_before;
    let top = |map: BTreeMap<String, usize>| -> Vec<String> {
        let mut names: Vec<(String, usize)> = map.into_iter().collect();
        names.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        names.iter().take(20).map(|(n, k)| if *k > 1 { format!("{n} ({k})") } else { n.clone() }).collect()
    };
    report.unknown_names = imp.unknown_total;
    report.unknown_examples = top(imp.unknown);
    report.unknown_patronymics = imp.unknown_patr_total;
    report.unknown_patronymic_examples = top(imp.unknown_patr);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn splits() {
        assert_eq!(split_fod("Ф.56 Оп.31 Д.12"), (Some("56".into()), Some("31".into()), Some("12".into())));
        assert_eq!(split_fod("Ф. 56, Оп. 31"), (Some("56".into()), Some("31".into()), None));
        assert_eq!(split_fod(""), (None, None, None));
        let mk = split_mk("Церковь: Христорождественская Село: Борисоглебское Уезд: Макарьевский Губерния: Костромская");
        assert_eq!(mk[1].as_deref(), Some("Борисоглебское"));
        assert_eq!(mk[3].as_deref(), Some("Костромская"));
        let row: Row = [(10, "?.06.1887".to_string()), (11, "#1".to_string()), (7, "#873".to_string()),
                        (18, "#1.5".to_string())].into_iter().collect();
        assert_eq!(date(&row, 10), (None, Some(6), Some(1887)));
        assert_eq!(date(&row, 11), (Some(1), Some(1), Some(1900)));
        let serial: Row = [(10, "#59".to_string()), (11, "#11689".to_string())].into_iter().collect();
        assert_eq!(date(&serial, 10), (Some(28), Some(2), Some(1900)));
        assert_eq!(date(&serial, 11), (Some(1), Some(1), Some(1932)));
        assert_eq!(cell(&row, 7), "873");
        assert_eq!(cell(&row, 18), "1,5");
        assert!(no_name("***") && no_name("Имя") && no_name("?") && !no_name("Иван") && !no_name("Имярек"));
        let zero: Row = [(13, "#0".to_string()), (14, "Кнышево".to_string())].into_iter().collect();
        assert_eq!((place(&zero, 13), place(&zero, 14)), (None, Some("Кнышево".to_string())));
    }

    fn seed() -> Option<PathBuf> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../resources/seed.sqlite");
        p.exists().then_some(p)
    }

    fn fresh(tag: &str, seed: &Path) -> (PathBuf, Connection) {
        let path = std::env::temp_dir().join(format!("genmetric-import-{tag}-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::copy(seed, &path).unwrap();
        let conn = Connection::open(&path).unwrap();
        (path, conn)
    }

    /// Синтетический индексатор (scripts/make_indexer_fixture.py) → импорт →
    /// проверки по колонкам.
    #[test]
    fn imports_fixture() {
        let Some(seed) = seed() else {
            eprintln!("нет resources/seed.sqlite — тест импорта пропущен");
            return;
        };
        let bytes = include_bytes!("../../../db/fixtures/indexer.xlsx");
        let seen = inspect(bytes).unwrap();
        assert_eq!((seen.births, seen.marriages, seen.deaths), (5, 2, 4), "{seen:?}");
        assert_eq!(seen.village, "Никольское");
        assert_eq!(seen.indexer, "Тест Тестов");
        assert_eq!(seen.years, vec![1889, 1890]);

        let (path, conn) = fresh("fixture", &seed);
        conn.execute_batch("BEGIN").unwrap();
        let rep = import_into(&conn, bytes).unwrap();
        conn.execute_batch("COMMIT").unwrap();
        assert_eq!((rep.births, rep.marriages, rep.deaths), (4, 2, 4), "{rep:?}");
        assert_eq!(rep.skipped.len(), 1, "{:?}", rep.skipped);
        assert!(rep.skipped[0].contains("лист «1», строка 7"), "{:?}", rep.skipped);
        assert_eq!(rep.unknown_names, 1, "{:?}", rep.unknown_examples);
        assert_eq!(rep.unknown_examples, vec!["Жданко".to_string()]);

        let text = |sql: &str| -> String {
            conn.query_row(sql, [], |r| r.get::<_, Option<String>>(0)).unwrap().unwrap_or_default()
        };
        let num = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };

        // Дела — по годам, со своими реквизитами; приход — один.
        assert_eq!(text("SELECT group_concat(year || ':' || fond || '/' || opis || '/' || delo, ' ') FROM (SELECT * FROM mk_case ORDER BY year)"),
                   "1889:1/2/11 1890:1/2/12");
        assert_eq!(num("SELECT count(DISTINCT parish_key) FROM mk_case"), 1);
        assert_eq!(text("SELECT indexer FROM mk_case LIMIT 1"), "Тест Тестов");
        assert_eq!(num("SELECT count(*) FROM entry e JOIN mk_case c ON c.id = e.case_id
                         WHERE coalesce(e.rite_year, e.event_year) <> c.year"), 0);

        // Рождение девочки: счёт в женской колонке, даты, родители, восприемник, причт.
        let girl = "SELECT e.id FROM entry e JOIN person_mention p ON p.entry_id = e.id
                     WHERE e.section = 1 AND p.role_code = 'child' AND p.first_name = 'Татьяна'";
        assert_eq!(text(&format!("SELECT page || '|' || coalesce(no_male, '-') || '|' || no_female || '|' || event_day || '.' || event_month || '.' || event_year || '|' || rite_day || '.' || rite_month || '.' || rite_year FROM entry WHERE id = ({girl})")),
                   "873|-|1|5.1.1889|6.1.1889");
        let role = |role: &str, col: &str| text(&format!(
            "SELECT {col} FROM person_mention p LEFT JOIN place pl ON pl.id = p.place_id WHERE p.entry_id = ({girl}) AND p.role_code = '{role}'"));
        assert_eq!(role("child", "p.gender"), "Ж");
        assert_eq!(role("father", "p.first_name || ' ' || p.patronymic || '|' || p.patronymic_modern || '|' || p.rank || '|' || pl.name || '|' || p.confession"),
                   "Никита Алексеев|Алексеевич|крестьянин|Тестово Малое|православного");
        assert_eq!(role("mother", "p.first_name || '|' || p.maiden_surname || '|' || p.gender"), "Евлампия|Сидорова|Ж");
        assert_eq!(role("godparent1", "p.first_name || '|' || p.note"), "Александр|того же дому");
        assert_eq!(role("clergy2", "p.first_name || ' ' || p.surname || '|' || p.rank"), "Пётр Троицкий|пономарь");
        // «?.06.1890» — день неизвестен; мальчик — в мужской колонке.
        assert_eq!(text("SELECT coalesce(event_day, '?') || '.' || event_month || '.' || event_year || '|' || no_male FROM entry e
                          WHERE section = 1 AND EXISTS (SELECT 1 FROM person_mention p WHERE p.entry_id = e.id AND p.first_name = 'Григорий' AND p.role_code = 'child')"),
                   "?.6.1890|1");
        // Имя вне словаря перенесено как в Excel.
        assert_eq!(num("SELECT count(*) FROM person_mention WHERE first_name = 'Жданко' AND first_name_modern IS NULL"), 1);

        // Брак: каким браком, лет, родственники с родством, поручители со стороной.
        assert_eq!(text("SELECT p.marriage_order || '|' || p.age_years FROM person_mention p WHERE p.role_code = 'groom' AND p.first_name = 'Михаил'"),
                   "Первым браком|22");
        assert_eq!(text("SELECT group_concat(role_code || ':' || kinship || ':' || coalesce(first_name, '-'), ' ') FROM (SELECT * FROM person_mention WHERE role_code IN ('groom_relative', 'bride_relative') ORDER BY entry_id, sort_order)"),
                   "groom_relative:отец:- bride_relative:отец:Савелий groom_relative:отец:- bride_relative:мать:-");
        assert_eq!(text("SELECT group_concat(note, ' / ') FROM (SELECT note FROM person_mention WHERE role_code LIKE 'witness%' ORDER BY entry_id, sort_order)"),
                   "по жениху / по невесте / по жениху");
        assert_eq!(num("SELECT count(*) FROM entry WHERE section = 2 AND no_female IS NOT NULL OR section = 2 AND rite_year IS NOT NULL"), 0);

        // Смерть: причина, возраст как в книге и разобранный, родство из «МК».
        let dead = |name: &str, col: &str| text(&format!(
            "SELECT {col} FROM person_mention p WHERE p.role_code = 'deceased' AND coalesce(p.first_name, '') = '{name}'"));
        assert_eq!(dead("Акилина", "death_cause || '|' || age_text || '|' || age_years || '|' || gender"), "старость|64|64|Ж");
        assert_eq!(dead("Васса", "age_text || '|' || age_months || '|' || coalesce(age_years, '-')"), "2 мес|2|-");
        assert_eq!(dead("", "rank || '|' || gender || '|' || death_cause"), "тело неизвестного человека|М|утонул");
        assert_eq!(text("SELECT group_concat(kinship || ':' || first_name || ':' || gender || ':' || coalesce(note, '-'), ' ') FROM (SELECT * FROM person_mention WHERE role_code = 'deceased_relative' ORDER BY entry_id)"),
                   "мать:Дарья:Ж:- отец:Евдоким:М:проживающий в селе");

        // Тот же путь, что набор руками: память персон, жён, причта, перечни.
        assert!(num("SELECT count(*) FROM person_index WHERE iof = 'Никита Алексеев'") == 1);
        assert_eq!(num("SELECT count(*) FROM person_index WHERE iof = 'Акилина Сергеева'"), 0, "умерший в память персон не идёт");
        assert!(num("SELECT count(*) FROM spouse_index") >= 2);
        assert_eq!(num("SELECT count(*) FROM clergy_index WHERE iof = 'Иоанн Преображенский'"), 1);
        assert_eq!(num("SELECT count(*) FROM lookup WHERE kind = 'death_cause' AND value = 'утонул'"), 1);
        assert!(rep.places >= 1 && num("SELECT count(*) FROM place WHERE name = 'Новое Тестово'") == 1);
        assert!(num("SELECT count(*) FROM usage_stat WHERE kind = 'rank_m' AND scope = 'parish'") >= 1);

        drop(conn);
        let _ = std::fs::remove_file(path);

        // Не индексатор — понятная ошибка, а не пустой приход.
        let err = inspect(include_bytes!("../../../db/export/familio_template.xlsx")).unwrap_err();
        assert!(err.contains("не индексатор"), "{err}");
    }

    /// «Золотая» проверка на файле заказчика (спека 2026-10-02, п. 5) — только
    /// у разработчика, файла в репозитории нет:
    ///     GENMETRIC_IMPORT_XLSM=…/Индексатор.xlsm cargo test -p genmetric-core --release import_golden -- --ignored --nocapture
    /// Импорт → строки выгрузки в Familio → сверка с листами `f - 1…3` файла
    /// по каждой колонке.
    #[test]
    #[ignore]
    fn import_golden() {
        let seed = seed().expect("resources/seed.sqlite");
        let file = std::env::var("GENMETRIC_IMPORT_XLSM").expect("GENMETRIC_IMPORT_XLSM");
        let bytes = std::fs::read(&file).unwrap();
        let (path, conn) = fresh("golden", &seed);
        let t = std::time::Instant::now();
        conn.execute_batch("BEGIN").unwrap();
        let rep = import_into(&conn, &bytes).unwrap();
        conn.execute_batch("COMMIT").unwrap();
        println!("импорт: {:?}; рождений {}, браков {}, смертей {}, персон {}, новых пунктов {}, годы {:?}",
                 t.elapsed(), rep.births, rep.marriages, rep.deaths, rep.persons, rep.places, rep.years);
        println!("не сверено имён: {} → {:?}", rep.unknown_names, rep.unknown_examples);
        println!("пропущено: {:?}", rep.skipped);
        println!("оговорки ({}): {:?}", rep.notes.len(), rep.notes.iter().take(10).collect::<Vec<_>>());
        // 1. Строго: что импортировано, то и выгружается в Excel — сверка с
        //    исходными листами «1», «2», «3» по каждой колонке.
        let back = crate::export::excel_rows(&conn).unwrap();
        let mut worst: f64 = 100.0;
        for (i, sheet) in ["1", "2", "3"].iter().enumerate() {
            let theirs = read_sheet(&bytes, sheet).unwrap();
            let data: Vec<&Row> = theirs.iter().filter(|(r, row)| is_record(**r, row)).map(|(_, row)| row).collect();
            println!("\n===== туда-обратно, лист «{sheet}»: в файле строк {}, у нас {}", data.len(), back[i].len());
            assert_eq!(data.len(), back[i].len());
            for c in 1..=55u32 {
                let (mut same, mut both_empty, mut diff) = (0, 0, Vec::new());
                for (n, row) in data.iter().enumerate() {
                    let a = cell(row, c);
                    let a = if a == "-" { String::new() } else { a };
                    let mut b = back[i][n].get(c as usize - 1).cloned().flatten().unwrap_or_default();
                    // Наша запись того же: неизвестный день — «??», у него «?».
                    if c == 10 || c == 11 { b = b.replace("??", "?"); }
                    // «Прим.» родственника умершего у нас начинается с родства
                    // (спека #39, В7) — у индексатора родство на листе «МК».
                    if i == 2 && c == 23 {
                        b = b.split_once(';').map(|x| x.1.trim().to_string()).unwrap_or_default();
                    }
                    if a.is_empty() && b.is_empty() { both_empty += 1; }
                    else if a == b { same += 1; }
                    else { diff.push(format!("№{}: «{a}» ≠ «{b}»", n + 1)); }
                }
                let filled = data.len() - both_empty;
                if filled == 0 { continue; }
                // Доля строк листа, где колонка совпала.
                let rate = 100.0 * (same + both_empty) as f64 / data.len() as f64;
                if !diff.is_empty() {
                    println!("{:>3} совпало {:>4} из {:>4} ({:>5.1}%)  расхождений {}: {}", crate::xlsx::col_letters(c),
                             same, filled, rate, diff.len(), diff.iter().take(8).cloned().collect::<Vec<_>>().join("; "));
                }
                // № пп — наш сквозной счёт (у индексатора в нумерации пропуск).
                if c != 1 { worst = worst.min(rate); }
            }
        }
        println!("\nтуда-обратно: худшая колонка совпала на {worst:.1}%");
        assert!(worst >= 99.0, "порог спеки — не ниже 99 % в каждой колонке");

        // 2. Сверка с листами выгрузки индексатора `f - 1…3` (где мы намеренно
        //    не как он — спека #39, В5 — расхождения ожидаемы).
        let ours = crate::export::familio_rows(&conn).unwrap();
        let first_row: u32 = std::env::var("GOLDEN_FIRST_ROW").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
        let show: usize = std::env::var("GOLDEN_SHOW").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
        for (i, sheet) in ["f - 1", "f - 2", "f - 3"].iter().enumerate() {
            let theirs = read_sheet(&bytes, sheet).unwrap();
            let head: Vec<String> = (1..=130).map(|c| {
                (1..first_row).rev().filter_map(|r| theirs.get(&r).map(|row| cell(row, c))).find(|v| !v.is_empty()).unwrap_or_default()
            }).collect();
            let data: Vec<&Row> = theirs.iter().filter(|(r, row)| **r >= first_row && int(row, 1).is_some()).map(|(_, row)| row).collect();
            println!("\n===== {sheet}: у индексатора строк {}, у нас {}", data.len(), ours[i].len());
            let width = ours[i].first().map(|r| r.len()).unwrap_or(0) as u32;
            for c in 1..=width {
                let (mut same, mut both_empty, mut diff) = (0, 0, Vec::new());
                for (n, row) in data.iter().enumerate() {
                    let a = cell(row, c);
                    let b = ours[i].get(n).and_then(|r| r.get(c as usize - 1).cloned().flatten()).unwrap_or_default();
                    if a.is_empty() && b.is_empty() { both_empty += 1; }
                    else if a == b { same += 1; }
                    else { diff.push(format!("№{}: «{a}» ≠ «{b}»", n + 1)); }
                }
                let filled = data.len() - both_empty;
                if filled == 0 { continue; }
                println!("{:>3} {:<28} совпало {:>4} из {:>4} ({:>5.1}%){}",
                         crate::xlsx::col_letters(c), head[c as usize - 1].chars().take(28).collect::<String>(),
                         same, filled, 100.0 * same as f64 / filled as f64,
                         if diff.is_empty() { String::new() } else { format!("  расхождений {}: {}", diff.len(), diff.iter().take(show).cloned().collect::<Vec<_>>().join("; ")) });
            }
        }
        assert_eq!((rep.births, rep.marriages, rep.deaths), (1085, 180, 824));
        drop(conn);
        // GOLDEN_KEEP=путь — оставить импортированную базу, чтобы посмотреть глазами.
        if let Ok(keep) = std::env::var("GOLDEN_KEEP") {
            std::fs::copy(&path, keep).unwrap();
        }
        let _ = std::fs::remove_file(path);
    }

    /// Наша выгрузка в Excel — те же листы: что выгружено, то и импортируется.
    #[test]
    fn imports_own_export() {
        let Some(seed) = seed() else { return };
        let (path_a, a) = fresh("own-a", &seed);
        a.execute_batch("BEGIN").unwrap();
        import_into(&a, include_bytes!("../../../db/fixtures/indexer.xlsx")).unwrap();
        a.execute_batch("COMMIT").unwrap();
        let (bytes, _) = crate::export::excel_bytes(&a).unwrap();
        let (path_b, b) = fresh("own-b", &seed);
        b.execute_batch("BEGIN").unwrap();
        let rep = import_into(&b, &bytes).unwrap();
        b.execute_batch("COMMIT").unwrap();
        assert_eq!((rep.births, rep.marriages, rep.deaths), (4, 2, 4), "{rep:?}");
        let dump = |c: &Connection| -> Vec<String> {
            let mut st = c.prepare(
                "SELECT e.section || '|' || coalesce(e.page, '') || '|' || coalesce(e.no_male, '') || '|' || coalesce(e.no_female, '')
                        || '|' || coalesce(e.event_day, '') || '.' || coalesce(e.event_month, '') || '.' || coalesce(e.event_year, '')
                        || '|' || coalesce(e.rite_day, '') || '.' || coalesce(e.rite_month, '') || '.' || coalesce(e.rite_year, '')
                        || '|' || p.role_code || '|' || coalesce(p.first_name, '') || ' ' || coalesce(p.patronymic, '') || ' ' || coalesce(p.surname, '')
                        || '|' || coalesce(p.rank, '') || '|' || coalesce(pl.name, '') || '|' || coalesce(p.note, '')
                        || '|' || coalesce(p.kinship, '') || '|' || coalesce(p.marriage_order, '') || '|' || coalesce(p.age_text, p.age_years, '')
                        || '|' || coalesce(p.death_cause, '') || '|' || coalesce(c.delo, '')
                   FROM entry e JOIN person_mention p ON p.entry_id = e.id JOIN mk_case c ON c.id = e.case_id
                   LEFT JOIN place pl ON pl.id = p.place_id
                  ORDER BY e.section, e.id, p.sort_order").unwrap();
            let rows = st.query_map([], |r| r.get::<_, String>(0)).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        let (da, db) = (dump(&a), dump(&b));
        assert_eq!(da.len(), db.len());
        for (x, y) in da.iter().zip(db.iter()) {
            assert_eq!(x, y);
        }
        drop((a, b));
        let _ = std::fs::remove_file(path_a);
        let _ = std::fs::remove_file(path_b);
    }
}
