//! Слияние приходов: записи из файла прихода помощника — в открытый приход
//! (спека 2026-10-17-sliyanie-prihodov.md; Роман 19 и 20.10.2026).
//!
//! Помощник заводит у себя пустой приход и набирает свои годы; его файл
//! время от времени вливается в приход Романа. Файл помощника только
//! читается. Записи идут тем же путём, что набор и импорт
//! (`records::save_entry_in_tx`): пункты заводятся по названию, память
//! подсказок растёт как при наборе.
//!
//! Что уже слито, помнит таблица `merge_origin` (схема 13): метка прихода
//! помощника + номер записи у него → наша запись и отпечаток его записи.
//! Повторное слияние того же файла ничего не добавляет. Помощник поправил
//! слитую запись — наша не меняется, правка ждёт в списке на сверку
//! («Взять правку»); нашу запись удалили, а у помощника она есть — она не
//! возвращается молча, а тоже ждёт на сверке («Вернуть запись»).
//!
//! Предпросмотр — то же слияние в транзакции, которая откатывается: числа
//! в окне до «Слить» и после него сходятся по построению.

use std::collections::{BTreeMap, HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use crate::db::SCHEMA_VERSION;
use crate::records::{save_entry_in_tx, Case, EntryInput, PersonInput};
use crate::statement;
use crate::text::{normalize, normalize_words, place_clean, strip_place_type};

fn s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Строка таблицы «годы и разделы»: что есть у нас, что в файле, что нового.
#[derive(Serialize, Debug, Default, Clone, PartialEq)]
pub struct YearRow {
    /// None — записи без года.
    pub year: Option<i64>,
    /// Рождения, браки, смерти: у нас до слияния.
    pub ours: [i64; 3],
    /// В файле помощника.
    pub theirs: [i64; 3],
    /// Из них новые — будут (были) добавлены.
    pub added: [i64; 3],
}

/// Что сделает (предпросмотр) или сделало слияние.
#[derive(Serialize, Debug, Default)]
pub struct Report {
    /// Приход файла, как его назвал помощник в делах: «Борисоглебское, Христорождественская».
    pub source: String,
    /// Кто индексировал — из дел файла.
    pub indexers: String,
    pub years: Vec<YearRow>,
    /// Записей в файле.
    pub total: i64,
    /// Добавлено новых.
    pub added: i64,
    /// Пропущено: такая запись у нас уже есть (то же место в книге и те же люди).
    pub duplicates: i64,
    /// Слиты раньше и с тех пор у помощника не менялись.
    pub known: i64,
    /// На сверку: помощник поправил слитую запись.
    pub edits: i64,
    /// На сверку: запись удалена у нас, а у помощника она есть.
    pub deleted: i64,
    /// На сверку: то же место в книге, а люди другие — сохранены обе.
    pub twins: i64,
    /// Слиты раньше, а у помощника их больше нет (у нас остаются).
    pub gone: i64,
    /// Дела, заведённые по файлу: «1890 год — Ф.56 Оп.31 Д.18».
    pub cases_new: Vec<String>,
    /// Пустые реквизиты наших дел, взятые из файла.
    pub case_filled: Vec<String>,
    /// Реквизиты, заполненные по-разному: остаются наши.
    pub case_diffs: Vec<String>,
    /// Новых населённых пунктов.
    pub places_new: i64,
    /// Пустые поля наших карточек, взятые из файла.
    pub place_filled: Vec<String>,
    /// Поля карточек, заполненные по-разному: остаются наши.
    pub place_diffs: Vec<String>,
    /// Связанных со словарём написаний имён, взятых из файла.
    pub aliases: i64,
    /// Что стоит прочитать до «Слить».
    pub warnings: Vec<String>,
}

impl Report {
    /// Сколько строк слияние добавит в список на сверку.
    pub fn to_review(&self) -> i64 {
        self.edits + self.deleted + self.twins
    }
}

// ----------------------------------------------------------------------------
//  Чтение прихода (нашего или файла помощника)
// ----------------------------------------------------------------------------

/// Тот ли это файл: приход GenMetric той же схемы, с меткой, и не наш же.
/// Возвращает метку прихода помощника.
pub fn check_source(conn: &Connection, src: &Connection) -> Result<String, String> {
    let tables: i64 = src
        .query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table'
                     AND name IN ('entry', 'person_mention', 'mk_case', 'schema_version', 'setting', 'place')", [], |r| r.get(0))
        .map_err(|e| format!("это не файл прихода GenMetric (не читается как база: {e})"))?;
    if tables != 6 {
        return Err("это не файл прихода GenMetric: в нём нет таблиц записей".into());
    }
    let old = "файл прихода сохранён прежней сборкой GenMetric. Пусть помощник поставит свежую сборку, \
               откроет в ней этот приход, закроет программу и пришлёт файл заново";
    let version: i64 = src.query_row("SELECT coalesce(max(version), 0) FROM schema_version", [], |r| r.get(0)).map_err(s)?;
    if version < SCHEMA_VERSION {
        return Err(old.into());
    }
    if version > SCHEMA_VERSION {
        return Err("файл прихода сохранён более новой сборкой GenMetric, чем ваша: обновите программу у себя и повторите".into());
    }
    let uid = |c: &Connection| -> Result<Option<String>, String> {
        c.query_row(&statement("merge_uid")?, [], |r| r.get::<_, Option<String>>(0)).optional().map_err(s)
            .map(|v| v.flatten().filter(|v| !v.trim().is_empty()))
    };
    let Some(theirs) = uid(src)? else { return Err(old.into()) };
    if uid(conn)?.as_deref() == Some(theirs.as_str()) {
        return Err("это файл того же прихода, что открыт сейчас (или его копия): сливать приход с самим собой нельзя".into());
    }
    Ok(theirs)
}

/// Дело с числом записей.
#[derive(Clone)]
struct CaseRow {
    case: Case,
    entries: i64,
}

fn read_cases(conn: &Connection) -> Result<Vec<CaseRow>, String> {
    let mut stmt = conn.prepare(&statement("merge_cases")?).map_err(s)?;
    let rows = stmt.query_map([], |r| Ok(CaseRow {
        case: Case { id: r.get(0)?, archive: r.get(1)?, fond: r.get(2)?, opis: r.get(3)?, delo: r.get(4)?,
                     church: r.get(5)?, village: r.get(6)?, uyezd: r.get(7)?, guberniya: r.get(8)?,
                     year: r.get(9)?, indexer: r.get(10)? },
        entries: r.get(11)?,
    })).map_err(s)?;
    rows.collect::<Result<_, _>>().map_err(s)
}

/// Запись прихода: её номер, номер её дела и содержимое — как его принимает
/// сохранение. Все записи (`id` пуст) или одна.
fn read_entries(conn: &Connection, id: Option<i64>) -> Result<Vec<(i64, i64, EntryInput)>, String> {
    let mut persons: HashMap<i64, Vec<PersonInput>> = HashMap::new();
    {
        let mut stmt = conn.prepare(&statement("merge_mentions")?).map_err(s)?;
        let mut rows = stmt.query(rusqlite::named_params! { ":id": id }).map_err(s)?;
        while let Some(r) = rows.next().map_err(s)? {
            let entry: i64 = r.get(0).map_err(s)?;
            persons.entry(entry).or_default().push(PersonInput {
                role_code: r.get(1).map_err(s)?, sort_order: r.get(2).map_err(s)?,
                surname: r.get(3).map_err(s)?, first_name: r.get(4).map_err(s)?, patronymic: r.get(5).map_err(s)?,
                surname_modern: r.get(6).map_err(s)?, first_name_modern: r.get(7).map_err(s)?,
                patronymic_modern: r.get(8).map_err(s)?, maiden_surname: r.get(9).map_err(s)?,
                gender: r.get(10).map_err(s)?, rank: r.get(11).map_err(s)?, confession: r.get(12).map_err(s)?,
                place: r.get(13).map_err(s)?, note: r.get(14).map_err(s)?, uncertain: r.get(15).map_err(s)?,
                age_years: r.get(16).map_err(s)?, marriage_order: r.get(17).map_err(s)?, kinship: r.get(18).map_err(s)?,
                age_months: r.get(19).map_err(s)?, age_weeks: r.get(20).map_err(s)?, age_days: r.get(21).map_err(s)?,
                age_text: r.get(22).map_err(s)?, death_cause: r.get(23).map_err(s)?,
            });
        }
    }
    let mut out = Vec::new();
    let mut stmt = conn.prepare(&statement("merge_entries")?).map_err(s)?;
    let mut rows = stmt.query(rusqlite::named_params! { ":id": id }).map_err(s)?;
    while let Some(r) = rows.next().map_err(s)? {
        let eid: i64 = r.get(0).map_err(s)?;
        let case_id: i64 = r.get(1).map_err(s)?;
        let scan: Option<String> = r.get(14).map_err(s)?;
        out.push((eid, case_id, EntryInput {
            id: None, case_id,
            section: r.get(2).map_err(s)?, page: r.get(3).map_err(s)?,
            no_male: r.get(4).map_err(s)?, no_female: r.get(5).map_err(s)?,
            event_day: r.get(6).map_err(s)?, event_month: r.get(7).map_err(s)?, event_year: r.get(8).map_err(s)?,
            rite_day: r.get(9).map_err(s)?, rite_month: r.get(10).map_err(s)?, rite_year: r.get(11).map_err(s)?,
            note: r.get(12).map_err(s)?, uncertain: r.get(13).map_err(s)?,
            scan_file: Some(scan),
            persons: persons.remove(&eid).unwrap_or_default(),
        }));
    }
    Ok(out)
}

// ----------------------------------------------------------------------------
//  Отпечаток и сравнение записей
// ----------------------------------------------------------------------------

fn text(v: &Option<String>) -> String {
    v.as_deref().map(str::trim).unwrap_or("").to_string()
}

fn number(v: Option<i64>) -> String {
    v.map(|n| n.to_string()).unwrap_or_default()
}

/// Отпечаток записи помощника: изменился — запись у него поправили. Поля
/// перечислены явно, а не снимком структуры: новое поле программы не должно
/// превратить все слитые записи в «поправленные». Файл скана не входит —
/// привязка скана не правка записи.
pub fn fingerprint(e: &EntryInput) -> String {
    let mut parts: Vec<String> = vec![
        e.section.to_string(), text(&e.page), number(e.no_male), number(e.no_female),
        number(e.event_day), number(e.event_month), number(e.event_year),
        number(e.rite_day), number(e.rite_month), number(e.rite_year),
        text(&e.note), text(&e.uncertain),
    ];
    let mut persons: Vec<&PersonInput> = e.persons.iter().collect();
    persons.sort_by(|a, b| (a.sort_order, &a.role_code).cmp(&(b.sort_order, &b.role_code)));
    for p in persons {
        parts.extend([
            p.role_code.clone(), p.sort_order.to_string(),
            text(&p.surname), text(&p.first_name), text(&p.patronymic),
            text(&p.surname_modern), text(&p.first_name_modern), text(&p.patronymic_modern),
            text(&p.maiden_surname), text(&p.gender), text(&p.rank), text(&p.confession),
            text(&p.place), text(&p.note), text(&p.uncertain),
            number(p.age_years), text(&p.marriage_order), text(&p.kinship),
            number(p.age_months), number(p.age_weeks), number(p.age_days),
            text(&p.age_text), text(&p.death_cause),
        ]);
    }
    // FNV-1a, 64 бита: одинаков на любой машине и в любой сборке (хеш
    // стандартной библиотеки этого не обещает).
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in parts.join("\u{1f}").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn iof(p: &PersonInput) -> String {
    [&p.first_name, &p.patronymic, &p.surname].iter()
        .filter_map(|v| v.as_deref().map(str::trim).filter(|v| !v.is_empty()))
        .collect::<Vec<_>>().join(" ")
}

const MAIN_ROLES: [&str; 5] = ["child", "father", "groom", "bride", "deceased"];

/// Главные лица записи одной строкой — «с точностью до написания»: без
/// регистра, «ё» и дореформенных букв.
fn main_persons(persons: &[PersonInput]) -> String {
    let mut all: Vec<String> = persons.iter().filter(|p| MAIN_ROLES.contains(&p.role_code.as_str()))
        .map(|p| format!("{}={}", p.role_code, normalize_words(&iof(p)))).collect();
    all.sort();
    all.join("|")
}

/// Те же лица для человека: «Иван Петров Сидоров, Мария Иванова».
fn main_shown(persons: &[PersonInput]) -> String {
    let all: Vec<String> = persons.iter().filter(|p| MAIN_ROLES.contains(&p.role_code.as_str()))
        .map(iof).filter(|v| !v.is_empty()).collect();
    if all.is_empty() { "без имён".into() } else { all.join(", ") }
}

/// Место записи в книге: раздел, год книги, страница, счёт.
type Spot = (i64, Option<i64>, String, Option<i64>, Option<i64>);

fn spot(e: &EntryInput) -> Spot {
    let page: String = text(&e.page).to_lowercase().split_whitespace().collect();
    (e.section, e.rite_year.or(e.event_year), page, e.no_male, e.no_female)
}

/// Наша запись в указателе дублей.
struct Known {
    id: i64,
    day: Option<i64>,
    month: Option<i64>,
    persons: String,
    shown: String,
}

fn our_index(conn: &Connection) -> Result<HashMap<Spot, Vec<Known>>, String> {
    let mut people: HashMap<i64, Vec<PersonInput>> = HashMap::new();
    {
        let mut stmt = conn.prepare(&statement("merge_our_main")?).map_err(s)?;
        let mut rows = stmt.query([]).map_err(s)?;
        while let Some(r) = rows.next().map_err(s)? {
            let entry: i64 = r.get(0).map_err(s)?;
            people.entry(entry).or_default().push(PersonInput {
                role_code: r.get(1).map_err(s)?, first_name: r.get(2).map_err(s)?,
                patronymic: r.get(3).map_err(s)?, surname: r.get(4).map_err(s)?, ..blank_person()
            });
        }
    }
    let mut index: HashMap<Spot, Vec<Known>> = HashMap::new();
    let mut stmt = conn.prepare(&statement("merge_our_entries")?).map_err(s)?;
    let mut rows = stmt.query([]).map_err(s)?;
    while let Some(r) = rows.next().map_err(s)? {
        let id: i64 = r.get(0).map_err(s)?;
        let page: Option<String> = r.get(3).map_err(s)?;
        let page: String = text(&page).to_lowercase().split_whitespace().collect();
        let key: Spot = (r.get(1).map_err(s)?, r.get(2).map_err(s)?, page, r.get(4).map_err(s)?, r.get(5).map_err(s)?);
        let persons = people.remove(&id).unwrap_or_default();
        index.entry(key).or_default().push(Known {
            id, day: r.get(6).map_err(s)?, month: r.get(7).map_err(s)?,
            persons: main_persons(&persons), shown: main_shown(&persons),
        });
    }
    Ok(index)
}

fn blank_person() -> PersonInput {
    PersonInput {
        role_code: String::new(), sort_order: 0, surname: None, first_name: None, patronymic: None,
        surname_modern: None, first_name_modern: None, patronymic_modern: None, maiden_surname: None,
        gender: None, rank: None, confession: None, place: None, note: None, uncertain: None,
        age_years: None, marriage_order: None, kinship: None, age_months: None, age_weeks: None,
        age_days: None, age_text: None, death_cause: None,
    }
}

fn date(d: Option<i64>, m: Option<i64>, y: Option<i64>) -> String {
    if d.is_none() && m.is_none() && y.is_none() {
        return String::new();
    }
    let two = |v: Option<i64>| v.map(|n| format!("{n:02}")).unwrap_or_else(|| "??".into());
    format!("{}.{}.{}", two(d), two(m), y.map(|n| n.to_string()).unwrap_or_else(|| "????".into()))
}

fn differ(out: &mut Vec<String>, label: &str, a: String, b: String) {
    let quote = |v: &str| if v.is_empty() { "пусто".to_string() } else { format!("«{v}»") };
    if normalize(&a) != normalize(&b) {
        out.push(format!("{label}: у вас {}, у помощника {}", quote(&a), quote(&b)));
    }
}

/// Чем запись помощника отличается от нашей — словами, для списка на сверку.
/// Пусто — видимых отличий нет.
fn differences(ours: &EntryInput, theirs: &EntryInput, roles: &HashMap<String, String>) -> Vec<String> {
    let mut out = Vec::new();
    differ(&mut out, "страница", text(&ours.page), text(&theirs.page));
    differ(&mut out, "счёт м.", number(ours.no_male), number(theirs.no_male));
    differ(&mut out, "счёт ж.", number(ours.no_female), number(theirs.no_female));
    differ(&mut out, "дата события", date(ours.event_day, ours.event_month, ours.event_year),
           date(theirs.event_day, theirs.event_month, theirs.event_year));
    differ(&mut out, "дата обряда", date(ours.rite_day, ours.rite_month, ours.rite_year),
           date(theirs.rite_day, theirs.rite_month, theirs.rite_year));
    differ(&mut out, "примечание записи", text(&ours.note), text(&theirs.note));
    let title = |code: &str| roles.get(code).cloned().unwrap_or_else(|| code.to_string());
    let mut codes: Vec<(i64, String)> = Vec::new();
    for p in ours.persons.iter().chain(&theirs.persons) {
        if !codes.iter().any(|c| c.1 == p.role_code) {
            codes.push((p.sort_order, p.role_code.clone()));
        }
    }
    codes.sort();
    for (_, code) in codes {
        // У роли бывает несколько человек (два родственника жениха) — по порядку.
        let mine: Vec<&PersonInput> = ours.persons.iter().filter(|p| p.role_code == code).collect();
        let other: Vec<&PersonInput> = theirs.persons.iter().filter(|p| p.role_code == code).collect();
        for n in 0..mine.len().max(other.len()) {
            let who = title(&code);
            match (mine.get(n), other.get(n)) {
                (Some(a), Some(b)) => {
                    let age = |p: &PersonInput| {
                        let t = text(&p.age_text);
                        if t.is_empty() { number(p.age_years) } else { t }
                    };
                    for (label, x, y) in [
                        ("", iof(a), iof(b)),
                        (" — НП", text(&a.place), text(&b.place)),
                        (" — звание", text(&a.rank), text(&b.rank)),
                        (" — возраст", age(a), age(b)),
                        (" — причина смерти", text(&a.death_cause), text(&b.death_cause)),
                        (" — родство", text(&a.kinship), text(&b.kinship)),
                        (" — каким браком", text(&a.marriage_order), text(&b.marriage_order)),
                        (" — вероисповедание", text(&a.confession), text(&b.confession)),
                        (" — девичья фамилия", text(&a.maiden_surname), text(&b.maiden_surname)),
                        (" — примечание", text(&a.note), text(&b.note)),
                    ] {
                        differ(&mut out, &format!("{who}{label}"), x, y);
                    }
                }
                (None, Some(b)) => out.push(format!("у помощника есть {who}: «{}», у вас нет", iof(b))),
                (Some(a), None) => out.push(format!("у помощника нет {who}: у вас «{}»", iof(a))),
                (None, None) => {}
            }
        }
    }
    out
}

fn brief(lines: &[String]) -> String {
    const SHOWN: usize = 5;
    let mut shown = lines.iter().take(SHOWN).cloned().collect::<Vec<_>>().join("; ");
    if lines.len() > SHOWN {
        shown.push_str(&format!("; и ещё отличий: {}", lines.len() - SHOWN));
    }
    shown
}

// ----------------------------------------------------------------------------
//  Дела и пункты
// ----------------------------------------------------------------------------

fn fod(c: &Case) -> String {
    let part = |label: &str, v: &Option<String>| {
        let v = text(v);
        if v.is_empty() { String::new() } else { format!("{label}{v}") }
    };
    [part("Ф.", &c.fond), part("Оп.", &c.opis), part("Д.", &c.delo)].into_iter()
        .filter(|v| !v.is_empty()).collect::<Vec<_>>().join(" ")
}

fn same_fod(a: &Case, b: &Case) -> bool {
    let key = |c: &Case| [normalize(&text(&c.fond)), normalize(&text(&c.opis)), normalize(&text(&c.delo))];
    key(a) == key(b)
}

fn case_label(c: &Case) -> String {
    let year = c.year.map(|y| format!("{y} год")).unwrap_or_else(|| "без года".into());
    let f = fod(c);
    if f.is_empty() { year } else { format!("{year} — {f}") }
}

fn save_case_row(conn: &Connection, c: &Case) -> Result<(), String> {
    conn.execute(&statement("case_upsert")?, rusqlite::named_params! {
        ":id": c.id, ":archive": c.archive, ":fond": c.fond, ":opis": c.opis, ":delo": c.delo,
        ":church": c.church, ":village": c.village, ":uyezd": c.uyezd, ":guberniya": c.guberniya,
        ":year": c.year, ":parish_key": c.parish_key(), ":indexer": c.indexer,
    }).map(|_| ()).map_err(s)
}

/// Название пункта у нас: пункт, который у нас объединён с другим
/// («Глинница» → «Глиница»), приходит под оставшимся названием — иначе
/// слияние завело бы ушедший пункт заново.
fn our_place(conn: &Connection, name: &str) -> Result<String, String> {
    let name = name.trim();
    let into: Option<String> = conn
        .query_row(&statement("place_merged_into")?, rusqlite::named_params! { ":old_norm": normalize(name) }, |r| r.get(0))
        .optional().map_err(s)?;
    let Some(into) = into else { return Ok(name.to_string()) };
    let kept: Option<String> = conn
        .query_row(&statement("place_find")?, rusqlite::named_params! { ":name_norm": into }, |r| r.get(1))
        .optional().map_err(s)?;
    Ok(kept.unwrap_or_else(|| name.to_string()))
}

/// Карточка пункта: тип, губерния, уезд, волость, ссылка Familio, комментарий.
struct Card {
    name: String,
    fields: [String; 5],
    comment: String,
}

const CARD_FIELDS: [&str; 5] = ["тип", "губерния", "уезд", "волость", "ссылка Familio"];

// ----------------------------------------------------------------------------
//  Слияние
// ----------------------------------------------------------------------------

/// Годы по порядку, записи без года — последними.
type Years = BTreeMap<(bool, i64), YearRow>;

fn year_row(years: &mut Years, year: Option<i64>) -> &mut YearRow {
    years.entry((year.is_none(), year.unwrap_or(0))).or_insert_with(|| YearRow { year, ..YearRow::default() })
}

/// Слить файл помощника `src` в приход `conn`. `commit = false` — предпросмотр:
/// всё делается и откатывается. `file` — имя файла, для сообщений.
pub fn run(conn: &Connection, src: &Connection, file: &str, commit: bool) -> Result<Report, String> {
    let uid = check_source(conn, src)?;
    let tx = conn.unchecked_transaction().map_err(s)?;
    let report = merge_in_tx(&tx, src, &uid, file)?;
    if commit {
        tx.commit().map_err(s)?;
    }
    // Иначе транзакция откатывается сама — предпросмотр ничего не оставляет.
    Ok(report)
}

fn merge_in_tx(conn: &Connection, src: &Connection, uid: &str, _file: &str) -> Result<Report, String> {
    let mut report = Report::default();
    let their_cases: Vec<CaseRow> = read_cases(src)?.into_iter().filter(|c| c.entries > 0).collect();
    let their_entries = read_entries(src, None)?;
    report.total = their_entries.len() as i64;
    let most = |values: Vec<String>| -> String {
        let mut count: BTreeMap<String, usize> = BTreeMap::new();
        for v in values.into_iter().filter(|v| !v.is_empty()) {
            *count.entry(v).or_default() += 1;
        }
        let mut all: Vec<(String, usize)> = count.into_iter().collect();
        all.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        all.into_iter().map(|a| a.0).collect::<Vec<_>>().join(", ")
    };
    report.source = [most(their_cases.iter().map(|c| text(&c.case.village)).collect()),
                     most(their_cases.iter().map(|c| text(&c.case.church)).collect())]
        .into_iter().filter(|v| !v.is_empty()).collect::<Vec<_>>().join(" — ");
    report.indexers = most(their_cases.iter().map(|c| text(&c.case.indexer)).collect());
    if their_entries.is_empty() {
        report.warnings.push("В файле нет ни одной записи — сливать нечего.".into());
    }

    // --- Дела: год есть только в файле — заводится; у обоих — сверяются. ---
    let mut ours = read_cases(conn)?;
    let had_cases = !ours.is_empty();
    {
        let mine: HashSet<String> = ours.iter().map(|c| normalize(&text(&c.case.village))).filter(|v| !v.is_empty()).collect();
        let theirs: HashSet<String> = their_cases.iter().map(|c| normalize(&text(&c.case.village))).filter(|v| !v.is_empty()).collect();
        if !mine.is_empty() && !theirs.is_empty() && mine.is_disjoint(&theirs) {
            report.warnings.push(format!(
                "Село в делах файла — «{}», а в открытом приходе — «{}». Проверьте, что сливаете в нужный приход: записи файла станут записями открытого прихода.",
                most(their_cases.iter().map(|c| text(&c.case.village)).collect()),
                most(ours.iter().map(|c| text(&c.case.village)).collect())));
        }
    }
    let mut case_map: HashMap<i64, i64> = HashMap::new();
    let mut created_cases: Vec<(i64, String)> = Vec::new();
    let mut seen_diffs: HashSet<String> = HashSet::new();
    for theirs in &their_cases {
        let t = &theirs.case;
        let found = ours.iter().position(|o| o.case.year == t.year && same_fod(&o.case, t))
            .or_else(|| t.year.and_then(|_| ours.iter().position(|o| o.case.year == t.year)));
        // Свойства прихода (церковь, село, уезд, губерния) — наши; у файла
        // другие — в отчёт. Пустые наши дополняются.
        let parish_fields = |c: &Case| [("церковь", c.church.clone()), ("село", c.village.clone()),
                                        ("уезд", c.uyezd.clone()), ("губерния", c.guberniya.clone())];
        match found {
            Some(i) => {
                let mut mine = ours[i].case.clone();
                let label = case_label(&mine);
                let mut changed = false;
                {
                    let slots: [(&str, &mut Option<String>, &Option<String>, bool); 9] = [
                        ("архив", &mut mine.archive, &t.archive, true), ("фонд", &mut mine.fond, &t.fond, true),
                        ("опись", &mut mine.opis, &t.opis, true), ("дело", &mut mine.delo, &t.delo, true),
                        ("церковь", &mut mine.church, &t.church, true), ("село", &mut mine.village, &t.village, true),
                        ("уезд", &mut mine.uyezd, &t.uyezd, true), ("губерния", &mut mine.guberniya, &t.guberniya, true),
                        // Кто индексировал — у каждого своё имя: не расхождение.
                        ("кто индексирует", &mut mine.indexer, &t.indexer, false),
                    ];
                    for (name, slot, theirs, compare) in slots {
                        let (a, b) = (text(slot), text(theirs));
                        if b.is_empty() || normalize(&a) == normalize(&b) {
                            continue;
                        }
                        if a.is_empty() {
                            *slot = Some(b.clone());
                            changed = true;
                            report.case_filled.push(format!("дело {label}: {name} — «{b}»"));
                        } else if compare {
                            let tail = if matches!(name, "фонд" | "опись" | "дело") {
                                "осталось ваше; записи файла этого года лежат в вашем деле"
                            } else {
                                "осталось ваше"
                            };
                            let line = format!("дело {label}: {name} — у вас «{a}», в файле «{b}» ({tail})");
                            if seen_diffs.insert(format!("{name}|{}|{}", normalize(&a), normalize(&b))) {
                                report.case_diffs.push(line);
                            }
                        }
                    }
                }
                if changed {
                    save_case_row(conn, &mine)?;
                    ours[i].case = mine;
                }
                case_map.insert(t.id, ours[i].case.id);
            }
            None => {
                let id: i64 = conn.query_row(&statement("case_new_id")?, [], |r| r.get(0)).map_err(s)?;
                let mut new = t.clone();
                new.id = id;
                if let Some(base) = ours.first().map(|o| o.case.clone()) {
                    for ((name, mine), (_, theirs)) in parish_fields(&base).into_iter().zip(parish_fields(t)) {
                        let (a, b) = (text(&mine), text(&theirs));
                        if !a.is_empty() && !b.is_empty() && normalize(&a) != normalize(&b)
                            && seen_diffs.insert(format!("{name}|{}|{}", normalize(&a), normalize(&b))) {
                            report.case_diffs.push(format!("{name} прихода — у вас «{a}», в файле «{b}» (осталось ваше)"));
                        }
                    }
                    let keep = |mine: &Option<String>, theirs: &Option<String>| if text(mine).is_empty() { theirs.clone() } else { mine.clone() };
                    new.church = keep(&base.church, &t.church);
                    new.village = keep(&base.village, &t.village);
                    new.uyezd = keep(&base.uyezd, &t.uyezd);
                    new.guberniya = keep(&base.guberniya, &t.guberniya);
                    if text(&new.archive).is_empty() {
                        new.archive = base.archive.clone();
                    }
                }
                if let Some(v) = new.village.clone() {
                    new.village = Some(our_place(conn, &v)?);
                }
                save_case_row(conn, &new)?;
                created_cases.push((id, case_label(&new)));
                report.cases_new.push(case_label(&new));
                case_map.insert(t.id, id);
                ours.push(CaseRow { case: new, entries: 0 });
            }
        }
    }
    // Запись в файле без дела (такого быть не должно) — в первое наше дело.
    let fallback_case = ours.first().map(|c| c.case.id);

    // --- Годы и разделы: что у нас было. ---
    let mut years: Years = BTreeMap::new();
    let index = our_index(conn)?;
    for (key, known) in &index {
        if (1..=3).contains(&key.0) {
            year_row(&mut years, key.1).ours[(key.0 - 1) as usize] += known.len() as i64;
        }
    }

    // --- Записи. ---
    let places_before: HashSet<String> = {
        let mut stmt = conn.prepare(&statement("merge_place_norms")?).map_err(s)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(s)?;
        rows.collect::<Result<_, _>>().map_err(s)?
    };
    let yearless: Vec<i64> = {
        let mut stmt = conn.prepare(&statement("merge_year_null_cases")?).map_err(s)?;
        let rows = stmt.query_map([], |r| r.get::<_, i64>(0)).map_err(s)?;
        rows.collect::<Result<_, _>>().map_err(s)?
    };
    let last_case: i64 = conn.query_row(&statement("merge_case_max")?, [], |r| r.get(0)).map_err(s)?;
    let roles: HashMap<String, String> = {
        let mut stmt = conn.prepare(&statement("merge_roles")?).map_err(s)?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(s)?;
        rows.collect::<Result<_, _>>().map_err(s)?
    };
    let mut parish_keys: HashMap<i64, String> = HashMap::new();
    let mut touched_places: HashSet<String> = HashSet::new();
    let mut place_names: HashMap<String, String> = HashMap::new();
    let review = |kind: &str, text: &str, entry: Option<i64>, origin: Option<i64>| -> Result<(), String> {
        conn.execute(&statement("merge_review_add")?, rusqlite::named_params! {
            ":kind": kind, ":text": text, ":entry_id": entry, ":origin_id": origin,
        }).map(|_| ()).map_err(s)
    };
    let exists = |id: Option<i64>| -> Result<bool, String> {
        let Some(id) = id else { return Ok(false) };
        conn.query_row(&statement("merge_entry_exists")?, rusqlite::named_params! { ":id": id }, |_| Ok(()))
            .optional().map(|v| v.is_some()).map_err(s)
    };
    let mut their_ids: HashSet<i64> = HashSet::new();
    for (their_id, their_case, raw) in their_entries {
        their_ids.insert(their_id);
        let section = raw.section;
        let book_year = raw.rite_year.or(raw.event_year);
        if (1..=3).contains(&section) {
            year_row(&mut years, book_year).theirs[(section - 1) as usize] += 1;
        }
        let print = fingerprint(&raw);
        // Запись — как она ляжет у нас: дело и названия пунктов наши.
        let mut entry = raw;
        let Some(case_id) = case_map.get(&their_case).copied().or(fallback_case) else {
            return Err("в файле есть записи, но нет ни одного дела — слить нельзя".into());
        };
        entry.case_id = case_id;
        for p in &mut entry.persons {
            if let Some(name) = p.place.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string) {
                let mapped = match place_names.get(&name) {
                    Some(m) => m.clone(),
                    None => {
                        let m = our_place(conn, &name)?;
                        place_names.insert(name.clone(), m.clone());
                        m
                    }
                };
                p.place = Some(mapped);
            }
        }
        let payload = || serde_json::to_string(&entry).map_err(s);

        let origin: Option<(i64, Option<i64>, String, Option<String>)> = conn
            .query_row(&statement("merge_origin_get")?, rusqlite::named_params! { ":uid": uid, ":source_entry": their_id },
                       |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .optional().map_err(s)?;
        if let Some((origin_id, our_id, known_print, pending_print)) = origin {
            let alive = exists(our_id)?;
            if alive && print == known_print {
                // Не менялась. Ждала правка, а помощник вернул как было — ждать нечего.
                if pending_print.is_some() {
                    conn.execute(&statement("merge_origin_pending")?, rusqlite::named_params! {
                        ":id": origin_id, ":pending": Option::<String>::None, ":pending_fp": Option::<String>::None,
                    }).map_err(s)?;
                    conn.execute(&statement("merge_review_close")?, rusqlite::named_params! { ":origin_id": origin_id }).map_err(s)?;
                }
                report.known += 1;
                continue;
            }
            if pending_print.as_deref() == Some(print.as_str()) {
                // Об этой правке (или об этой удалённой записи) уже спрашивали.
                report.known += 1;
                continue;
            }
            let where_ = format!("{}, {}", ["", "рождение", "брак", "смерть"].get(section as usize).copied().unwrap_or("запись"),
                                 main_shown(&entry.persons));
            if alive {
                let mine = read_entries(conn, our_id)?.into_iter().next().map(|e| e.2);
                let mut lines = mine.as_ref().map(|m| differences(m, &entry, &roles)).unwrap_or_default();
                // Отличий в показанных полях нет, а записи разные — правка в
                // поле, которого в перечне нет (пол, современное написание,
                // отметка сомнения): молча её не терять (ревьюер 11.11.2026).
                let identical = mine.as_ref().is_some_and(|m| fingerprint(m) == fingerprint(&entry));
                if lines.is_empty() && !identical {
                    lines.push("отличия — в полях, которые здесь не показаны: пол, современное написание имени, отметка сомнения, части возраста".into());
                }
                if lines.is_empty() {
                    // У нас уже так же (поправили сами) — принять молча.
                    conn.execute(&statement("merge_origin_accept")?, rusqlite::named_params! {
                        ":id": origin_id, ":entry_id": our_id, ":fingerprint": print,
                    }).map_err(s)?;
                    conn.execute(&statement("merge_review_close")?, rusqlite::named_params! { ":origin_id": origin_id }).map_err(s)?;
                    report.known += 1;
                    continue;
                }
                conn.execute(&statement("merge_review_drop_open")?, rusqlite::named_params! { ":origin_id": origin_id }).map_err(s)?;
                review("merge_edit", &format!("помощник поправил запись ({where_}): {}. «Взять правку» заменит вашу запись версией помощника целиком",
                                              brief(&lines)), our_id, Some(origin_id))?;
                report.edits += 1;
            } else {
                conn.execute(&statement("merge_review_drop_open")?, rusqlite::named_params! { ":origin_id": origin_id }).map_err(s)?;
                review("merge_deleted", &format!("у вас эта запись удалена, а в файле помощника она есть: {where_}{}{}",
                                                 book_year.map(|y| format!(", {y} год")).unwrap_or_default(),
                                                 if text(&entry.page).is_empty() { String::new() } else { format!(", стр. {}", text(&entry.page)) }),
                       None, Some(origin_id))?;
                report.deleted += 1;
            }
            conn.execute(&statement("merge_origin_pending")?, rusqlite::named_params! {
                ":id": origin_id, ":pending": payload()?, ":pending_fp": print,
            }).map_err(s)?;
            continue;
        }

        // Новая для нас запись: нет ли такой же, набранной у нас?
        let key = spot(&entry);
        let persons = main_persons(&entry.persons);
        let numbered = entry.no_male.is_some() || entry.no_female.is_some();
        let here = index.get(&key);
        // Без счёта и без имён (умерший «личность не установлена») место в
        // книге не опознать — такая запись всегда новая.
        let named = entry.persons.iter().any(|p| MAIN_ROLES.contains(&p.role_code.as_str()) && !iof(p).is_empty());
        let same = here.filter(|_| numbered || named)
            .and_then(|all| all.iter().find(|k| k.persons == persons && k.day == entry.event_day && k.month == entry.event_month));
        if let Some(same) = same {
            conn.execute(&statement("merge_origin_add")?, rusqlite::named_params! {
                ":uid": uid, ":source_entry": their_id, ":entry_id": same.id, ":fingerprint": print,
            }).map_err(s)?;
            report.duplicates += 1;
            continue;
        }
        let parish = match parish_keys.get(&case_id) {
            Some(k) => k.clone(),
            None => {
                let k: String = conn.query_row(&statement("case_parish_key")?, rusqlite::named_params! { ":id": case_id },
                                               |r| r.get::<_, Option<String>>(0)).optional().map_err(s)?.flatten().unwrap_or_default();
                parish_keys.insert(case_id, k.clone());
                k
            }
        };
        if let Some(p) = entry.persons.iter().find(|p| !roles.contains_key(&p.role_code)) {
            return Err(format!("в файле помощника есть роль «{}», которой нет в вашей сборке GenMetric: обновите программу у себя и повторите", p.role_code));
        }
        let saved = save_entry_in_tx(conn, &entry, &parish).map_err(|e| format!("запись № {their_id} файла не перенесена: {e}"))?;
        conn.execute(&statement("entry_set_case")?, rusqlite::named_params! { ":id": saved.id, ":case_id": case_id }).map_err(s)?;
        conn.execute(&statement("merge_origin_add")?, rusqlite::named_params! {
            ":uid": uid, ":source_entry": their_id, ":entry_id": saved.id, ":fingerprint": print,
        }).map_err(s)?;
        for p in &entry.persons {
            if let Some(name) = p.place.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
                touched_places.insert(normalize(name));
            }
        }
        report.added += 1;
        if (1..=3).contains(&section) {
            year_row(&mut years, book_year).added[(section - 1) as usize] += 1;
        }
        // То же место в книге (страница и счёт), а люди или день другие — обе
        // записи остаются, обе — на сверку (Роман 19.10.2026).
        if numbered {
            for twin in here.into_iter().flatten() {
                let place = format!("{}{}", book_year.map(|y| format!("{y} год, ")).unwrap_or_default(),
                                    if text(&entry.page).is_empty() { "без страницы".to_string() } else { format!("стр. {}", text(&entry.page)) });
                review("merge_twin", &format!("та же страница и тот же счёт, что у вашей записи ({place}): у вас — {}, в файле помощника — {}. Сохранены обе: сверьте по скану и поправьте неверную (удалить запись в программе пока нельзя)",
                                              twin.shown, main_shown(&entry.persons)), Some(saved.id), None)?;
                review("merge_twin", &format!("в файле помощника на этом месте книги ({place}) другая запись: {} — она добавлена рядом с вашей ({}). Сверьте по скану и поправьте неверную (удалить запись в программе пока нельзя)",
                                              main_shown(&entry.persons), twin.shown), Some(twin.id), None)?;
                report.twins += 1;
            }
        }
    }

    // Сохранение записи другого года могло дать год нашему делу без года или
    // завести дело года сверх дел файла — вернуть как было (как при импорте).
    for id in yearless {
        conn.execute(&statement("case_clear_year")?, rusqlite::named_params! { ":id": id }).map_err(s)?;
    }
    conn.execute(&statement("case_drop_empty_after")?, rusqlite::named_params! { ":last": last_case }).map_err(s)?;
    // Дело, заведённое по файлу, осталось пустым (все его записи у нас уже
    // были) — не оставлять.
    for (id, label) in &created_cases {
        if conn.execute(&statement("merge_case_drop")?, rusqlite::named_params! { ":id": id }).map_err(s)? > 0 {
            report.cases_new.retain(|l| l != label);
        }
    }

    // Слиты раньше, а у помощника их больше нет.
    {
        let mut stmt = conn.prepare(&statement("merge_origin_sources")?).map_err(s)?;
        let rows = stmt.query_map(rusqlite::named_params! { ":uid": uid }, |r| r.get::<_, i64>(0)).map_err(s)?;
        for id in rows {
            if !their_ids.contains(&id.map_err(s)?) {
                report.gone += 1;
            }
        }
    }

    // --- Карточки пунктов: пустые поля наших дополняются, расхождения — в отчёт. ---
    for c in &their_cases {
        if let Some(v) = c.case.village.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            if !report.cases_new.is_empty() {
                touched_places.insert(normalize(&our_place(conn, v)?));
            }
        }
    }
    let mut cards: Vec<Card> = Vec::new();
    {
        let mut stmt = src.prepare(&statement("merge_places")?).map_err(s)?;
        let mut rows = stmt.query([]).map_err(s)?;
        let mut last = String::new();
        while let Some(r) = rows.next().map_err(s)? {
            let norm: String = r.get(1).map_err(s)?;
            if norm == last {
                continue; // у названия несколько строк — лучшая идёт первой
            }
            last = norm;
            cards.push(Card {
                name: r.get(0).map_err(s)?,
                fields: [r.get(2).map_err(s)?, r.get(3).map_err(s)?, r.get(4).map_err(s)?, r.get(5).map_err(s)?, r.get(6).map_err(s)?],
                comment: r.get(7).map_err(s)?,
            });
        }
    }
    for theirs in cards {
        let name = our_place(conn, &theirs.name)?;
        let norm = normalize(&name);
        if !touched_places.contains(&norm) {
            continue;
        }
        let mine: Option<(i64, String, [String; 5], String)> = conn
            .query_row(&statement("place_get")?, rusqlite::named_params! { ":name_norm": norm }, |r| {
                let get = |i: usize| r.get::<_, Option<String>>(i).map(|v| v.unwrap_or_default());
                Ok((r.get(0)?, r.get(1)?, [get(2)?, get(3)?, get(4)?, get(5)?, get(6)?], r.get(8)?))
            })
            .optional().map_err(s)?;
        let Some((id, our_name, mut fields, comment)) = mine else { continue };
        let is_new = !places_before.contains(&norm);
        let mut filled = Vec::new();
        for ((slot, theirs), label) in fields.iter_mut().zip(&theirs.fields).zip(CARD_FIELDS) {
            let (a, b) = (slot.trim().to_string(), theirs.trim());
            if b.is_empty() || normalize(&a) == normalize(b) {
                continue;
            }
            if a.is_empty() {
                *slot = b.to_string();
                filled.push(format!("{label}: {b}"));
            } else {
                report.place_diffs.push(format!("пункт «{our_name}»: {label} — у вас «{a}», в файле «{b}» (осталось ваше)"));
            }
        }
        if filled.is_empty() {
            continue;
        }
        let blank = |v: &str| (!v.trim().is_empty()).then(|| v.trim().to_string());
        // Комментарий тёзки — часть названия: берётся только у нового пункта.
        let comment = if is_new && comment.trim().is_empty() { theirs.comment.clone() } else { comment };
        let clean = place_clean(&our_name, &comment).to_string();
        let done = conn.execute(&statement("place_update")?, rusqlite::named_params! {
            ":id": id, ":name": our_name, ":name_norm": norm,
            ":np_type": blank(&fields[0]), ":guberniya": blank(&fields[1]), ":uyezd": blank(&fields[2]),
            ":volost": blank(&fields[3]), ":familio_url": blank(&fields[4]),
            ":clean": strip_place_type(&clean), ":comment": blank(&comment),
        });
        match done {
            Ok(_) if !is_new => report.place_filled.push(format!("пункт «{our_name}» — {}", filled.join(", "))),
            Ok(_) => {}
            Err(e) if e.to_string().contains("UNIQUE") => report.place_diffs.push(format!(
                "пункт «{our_name}»: карточка не дополнена из файла — у него есть двойник с теми же типом, уездом и губернией")),
            Err(e) => return Err(e.to_string()),
        }
    }
    // Новые пункты, у которых в файле карточки не нашлось, — тоже новые.
    {
        let mut stmt = conn.prepare(&statement("merge_place_norms")?).map_err(s)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(s)?;
        let now: HashSet<String> = rows.collect::<Result<_, _>>().map_err(s)?;
        report.places_new = now.difference(&places_before).count() as i64;
    }

    // --- Связанные написания имён. ---
    {
        let mut stmt = src.prepare(&statement("merge_aliases")?).map_err(s)?;
        let mut rows = stmt.query([]).map_err(s)?;
        let add = statement("merge_alias_add")?;
        while let Some(r) = rows.next().map_err(s)? {
            let (kind, form, form_norm, target, gender): (String, String, String, Option<String>, Option<String>) =
                (r.get(0).map_err(s)?, r.get(1).map_err(s)?, r.get(2).map_err(s)?, r.get(3).map_err(s)?, r.get(4).map_err(s)?);
            report.aliases += conn.execute(&add, rusqlite::named_params! {
                ":kind": kind, ":form": form, ":form_norm": form_norm, ":target": target, ":gender": gender,
            }).map_err(s)? as i64;
        }
    }

    // Расхождения дел и карточек — и в список на сверку: отчёт закроют, а
    // поправить реквизиты можно потом. Одно расхождение — одна строка навсегда.
    for line in report.case_diffs.iter().chain(&report.place_diffs) {
        conn.execute(&statement("merge_note_add")?, rusqlite::named_params! {
            // Без имени файла: тот же файл под другим именем не должен повторять строку.
            ":text": format!("слияние с файлом помощника: {line}"),
        }).map_err(s)?;
    }
    if !had_cases && !report.cases_new.is_empty() {
        report.warnings.push("В открытом приходе ещё нет дел: реквизиты (церковь, село, уезд, губерния) будут взяты из файла помощника.".into());
    }
    report.years = years.into_values().collect();
    Ok(report)
}

/// Запись целиком одной строкой JSON — в журнал перед удалением, чтобы
/// удалённое можно было восстановить (11.11.2026).
pub fn snapshot(conn: &Connection, id: i64) -> Result<String, String> {
    let entry = read_entries(conn, Some(id))?.into_iter().next().map(|e| e.2)
        .ok_or("этой записи уже нет — список перечитан")?;
    serde_json::to_string(&entry).map_err(s)
}

/// Что сделала кнопка строки списка на сверку.
#[derive(Serialize, Debug)]
pub struct Taken {
    /// Наша запись, получившая версию помощника (или заведённая заново).
    pub entry_id: i64,
    pub section: i64,
    /// true — запись заведена заново («Вернуть запись»).
    pub restored: bool,
}

/// «Взять правку» / «Вернуть запись» в списке на сверку: запись помощника,
/// ждавшая решения, становится нашей. Строка списка отмечается сделанной.
pub fn take(conn: &Connection, review_id: i64) -> Result<Taken, String> {
    let (kind, origin_id): (String, Option<i64>) = conn
        .query_row(&statement("merge_review_origin")?, rusqlite::named_params! { ":id": review_id }, |r| Ok((r.get(0)?, r.get(1)?)))
        .optional().map_err(s)?
        .ok_or("этой строки в списке на сверку уже нет")?;
    let origin_id = origin_id.filter(|_| kind == "merge_edit" || kind == "merge_deleted")
        .ok_or("у этой строки списка нет записи помощника")?;
    let (_, our_id, pending, pending_print): (i64, Option<i64>, Option<String>, Option<String>) = conn
        .query_row(&statement("merge_origin_by_id")?, rusqlite::named_params! { ":id": origin_id },
                   |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .optional().map_err(s)?
        .ok_or("память слияния об этой записи не найдена")?;
    let (Some(pending), Some(print)) = (pending, pending_print) else {
        return Err("правка помощника уже взята или отменена следующим слиянием".into());
    };
    let mut entry: EntryInput = serde_json::from_str(&pending).map_err(|e| format!("запись помощника не читается: {e}"))?;
    let alive = match our_id {
        Some(id) => conn.query_row(&statement("merge_entry_exists")?, rusqlite::named_params! { ":id": id }, |_| Ok(()))
            .optional().map_err(s)?.is_some(),
        None => false,
    };
    let wanted_case = entry.case_id;
    let tx = conn.unchecked_transaction().map_err(s)?;
    // Пункт, объединённый у нас, пока правка ждала, — под оставшимся названием.
    for p in &mut entry.persons {
        if let Some(name) = p.place.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string) {
            p.place = Some(our_place(&tx, &name)?);
        }
    }
    // Как в слиянии: сохранение записи другого года не должно оставить
    // после себя лишнее дело или год у дела без года.
    let yearless: Vec<i64> = {
        let mut stmt = tx.prepare(&statement("merge_year_null_cases")?).map_err(s)?;
        let rows = stmt.query_map([], |r| r.get::<_, i64>(0)).map_err(s)?;
        rows.collect::<Result<_, _>>().map_err(s)?
    };
    let last_case: i64 = tx.query_row(&statement("merge_case_max")?, [], |r| r.get(0)).map_err(s)?;
    if alive {
        entry.id = our_id;
        // Привязка скана у нашей записи остаётся нашей: скан в отпечаток и в
        // отличия не входит, менять его молча нельзя (ревьюер 11.11.2026).
        entry.scan_file = None;
        // Дело — то, где запись лежит у нас.
        entry.case_id = tx.query_row(&statement("merge_entry_case")?, rusqlite::named_params! { ":id": our_id }, |r| r.get(0)).map_err(s)?;
    } else {
        entry.id = None;
    }
    let parish: String = tx
        .query_row(&statement("case_parish_key")?, rusqlite::named_params! { ":id": entry.case_id }, |r| r.get::<_, Option<String>>(0))
        .optional().map_err(s)?.flatten().unwrap_or_default();
    let saved = save_entry_in_tx(&tx, &entry, &parish)?;
    if !alive {
        // Вернувшаяся запись — в дело, куда её положило слияние, если оно ещё есть.
        let there = tx.query_row(&statement("merge_case_exists")?, rusqlite::named_params! { ":id": wanted_case }, |_| Ok(()))
            .optional().map_err(s)?.is_some();
        if there {
            tx.execute(&statement("entry_set_case")?, rusqlite::named_params! { ":id": saved.id, ":case_id": wanted_case }).map_err(s)?;
            for id in yearless {
                tx.execute(&statement("case_clear_year")?, rusqlite::named_params! { ":id": id }).map_err(s)?;
            }
            tx.execute(&statement("case_drop_empty_after")?, rusqlite::named_params! { ":last": last_case }).map_err(s)?;
        }
    }
    tx.execute(&statement("merge_origin_accept")?, rusqlite::named_params! {
        ":id": origin_id, ":entry_id": saved.id, ":fingerprint": print,
    }).map_err(s)?;
    tx.execute(&statement("merge_review_close")?, rusqlite::named_params! { ":origin_id": origin_id }).map_err(s)?;
    tx.commit().map_err(s)?;
    Ok(Taken { entry_id: saved.id, section: entry.section, restored: !alive })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::{save_case, save_entry};
    use std::path::Path;

    fn fresh(tag: &str, year: i64, delo: &str) -> Option<(std::path::PathBuf, Connection)> {
        let seed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../resources/seed.sqlite");
        if !seed.exists() {
            crate::seed_missing("тест слияния приходов пропущен");
            return None;
        }
        let path = std::env::temp_dir().join(format!("genmetric-join-{tag}-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::copy(&seed, &path).unwrap();
        let conn = Connection::open(&path).unwrap();
        conn.execute(&statement("merge_uid_ensure").unwrap(), []).unwrap();
        let case = Case { id: 0, archive: Some("ГА".into()), fond: Some("56".into()), opis: Some("31".into()), delo: Some(delo.into()),
                          church: Some("Никольская".into()), village: Some("Тестово".into()), uyezd: None, guberniya: None,
                          year: Some(year), indexer: Some(tag.into()) };
        save_case(&conn, &case, false).unwrap();
        Some((path, conn))
    }

    fn person(role: &str, order: i64, first: &str, patr: &str, surname: &str, place: &str) -> PersonInput {
        let some = |v: &str| (!v.is_empty()).then(|| v.to_string());
        PersonInput { role_code: role.into(), sort_order: order, first_name: some(first), patronymic: some(patr),
                      surname: some(surname), place: some(place), gender: None, ..blank_person() }
    }

    fn birth(year: i64, page: &str, no: i64, girl: bool, child: &str, father: (&str, &str, &str), place: &str) -> EntryInput {
        EntryInput {
            id: None, case_id: 1, section: 1, page: Some(page.into()),
            no_male: (!girl).then_some(no), no_female: girl.then_some(no),
            event_day: Some(3), event_month: Some(5), event_year: Some(year),
            rite_day: Some(4), rite_month: Some(5), rite_year: Some(year), note: None, uncertain: None, scan_file: None,
            persons: vec![person("child", 10, child, "", "", ""), person("father", 20, father.0, father.1, father.2, place)],
        }
    }

    fn num(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn merge_story() {
        let Some((ours_path, ours)) = fresh("ours", 1889, "11") else { return };
        let Some((theirs_path, theirs)) = fresh("theirs", 1890, "12") else { return };
        // У нас: 1889 год, две записи; пункт «Фетинино» без волости; «Глинница» объединена с «Глиница».
        ours.execute_batch("INSERT INTO place (name, name_norm, np_type, uyezd) VALUES ('Фетинино', 'фетинино', 'д.', 'Макарьевский');
                            INSERT INTO place (name, name_norm) VALUES ('Глиница', 'глиница');
                            INSERT INTO place_merged (old_norm, into_norm) VALUES ('глинница', 'глиница');").unwrap();
        save_entry(&ours, &birth(1889, "14", 1, false, "Пётр", ("Иван", "Капитонов", ""), "Фетинино")).unwrap();
        save_entry(&ours, &birth(1889, "15", 2, true, "Анна", ("Семён", "Иванов", ""), "Фетинино")).unwrap();
        // У помощника: 1890 год (две записи) и дело 1889 года с другим номером
        // дела: дубль нашей записи и запись на месте нашей «Анны».
        theirs.execute_batch("INSERT INTO place (name, name_norm, np_type, uyezd, volost) VALUES ('Фетинино', 'фетинино', 'д.', 'Юрьевецкий', 'Завражная');
                              INSERT INTO place (name, name_norm, np_type, volost) VALUES ('Малово', 'малово', 'д.', 'Завражная');").unwrap();
        save_entry(&theirs, &birth(1890, "3", 1, true, "Евдокия", ("Яков", "Ильин", "Ермаков"), "Малово")).unwrap();
        save_entry(&theirs, &birth(1890, "3об", 1, false, "Фёдор", ("Павел", "Петров", ""), "Глинница")).unwrap();
        save_case(&theirs, &Case { id: 1, archive: Some("ГА".into()), fond: Some("56".into()), opis: Some("31".into()), delo: Some("99".into()),
                                   church: Some("Никольская".into()), village: Some("Тестово".into()), uyezd: None, guberniya: None,
                                   year: Some(1889), indexer: Some("theirs".into()) }, false).unwrap();
        save_entry(&theirs, &birth(1889, "14", 1, false, "Петръ", ("Иванъ", "Капитоновъ", ""), "Фетинино")).unwrap();
        save_entry(&theirs, &birth(1889, "15", 2, true, "Евдокия", ("Семён", "Иванов", ""), "Фетинино")).unwrap();

        // Предпросмотр ничего не оставляет.
        let before = num(&ours, "SELECT count(*) FROM entry");
        let plan = run(&ours, &theirs, "помощник.sqlite", false).unwrap();
        assert_eq!((plan.total, plan.added, plan.duplicates, plan.twins, plan.known), (4, 3, 1, 1, 0), "{plan:?}");
        assert_eq!(num(&ours, "SELECT count(*) FROM entry"), before);
        assert_eq!(num(&ours, "SELECT count(*) FROM merge_origin") + num(&ours, "SELECT count(*) FROM review_item"), 0);
        assert_eq!(num(&ours, "SELECT count(*) FROM mk_case"), 1);

        // Слияние: числа те же, что в предпросмотре.
        let done = run(&ours, &theirs, "помощник.sqlite", true).unwrap();
        assert_eq!((done.total, done.added, done.duplicates, done.twins), (4, 3, 1, 1));
        assert_eq!(num(&ours, "SELECT count(*) FROM entry"), before + 3);
        assert_eq!(done.cases_new, vec!["1890 год — Ф.56 Оп.31 Д.12".to_string()]);
        assert!(done.case_diffs.iter().any(|d| d.contains("дело —") && d.contains("«11»") && d.contains("«99»")), "{:?}", done.case_diffs);
        assert_eq!(num(&ours, "SELECT count(*) FROM mk_case"), 2, "дело 1890 года заведено, лишних нет");
        assert_eq!(num(&ours, "SELECT count(*) FROM entry e JOIN mk_case c ON c.id = e.case_id WHERE c.year = 1890"), 2);
        assert_eq!(num(&ours, "SELECT count(*) FROM review_item WHERE kind = 'merge_twin'"), 2, "обе записи одного места — на сверке");
        assert_eq!(num(&ours, "SELECT count(*) FROM review_item WHERE kind = 'note' AND text LIKE 'слияние с файлом помощника:%'"),
                   (done.case_diffs.len() + done.place_diffs.len()) as i64);
        // Пункты: «Глинница» у нас объединена — запись легла к «Глиница»; карточки.
        assert_eq!(num(&ours, "SELECT count(*) FROM place WHERE name_norm = 'глинница'"), 0);
        assert_eq!(num(&ours, "SELECT count(*) FROM person_mention m JOIN place p ON p.id = m.place_id WHERE p.name = 'Глиница'"), 1);
        assert_eq!(done.places_new, 1, "новый — только «Малово»");
        assert_eq!(num(&ours, "SELECT count(*) FROM place WHERE name = 'Малово' AND volost = 'Завражная' AND np_type = 'д.'"), 1);
        assert_eq!(done.place_filled, vec!["пункт «Фетинино» — волость: Завражная".to_string()]);
        assert!(done.place_diffs.iter().any(|d| d.contains("«Фетинино»") && d.contains("«Макарьевский»") && d.contains("«Юрьевецкий»")));
        assert_eq!(num(&ours, "SELECT count(*) FROM place WHERE name = 'Фетинино' AND uyezd = 'Макарьевский' AND volost = 'Завражная'"), 1);
        let y1890 = done.years.iter().find(|y| y.year == Some(1890)).unwrap();
        assert_eq!((y1890.ours, y1890.theirs, y1890.added), ([0, 0, 0], [2, 0, 0], [2, 0, 0]));
        let y1889 = done.years.iter().find(|y| y.year == Some(1889)).unwrap();
        assert_eq!((y1889.ours, y1889.theirs, y1889.added), ([2, 0, 0], [2, 0, 0], [1, 0, 0]));
        // Память подсказок выросла как при наборе.
        assert_eq!(num(&ours, "SELECT count(*) FROM person_index WHERE iof = 'Яков Ильин Ермаков' AND place = 'Малово'"), 1);

        // Тот же файл второй раз: ничего не добавляет и ничего не шлёт на сверку.
        let reviews = num(&ours, "SELECT count(*) FROM review_item");
        let again = run(&ours, &theirs, "помощник.sqlite", true).unwrap();
        assert_eq!((again.added, again.duplicates, again.known, again.to_review()), (0, 0, 4, 0), "{again:?}");
        assert_eq!(num(&ours, "SELECT count(*) FROM entry"), before + 3);
        assert_eq!(num(&ours, "SELECT count(*) FROM review_item"), reviews);

        // Помощник поправил слитую запись: наша прежняя, правка — на сверке.
        theirs.execute("UPDATE person_mention SET first_name = 'Евпраксия' WHERE first_name = 'Евдокия' AND entry_id = 1", []).unwrap();
        let edited = run(&ours, &theirs, "помощник.sqlite", true).unwrap();
        assert_eq!((edited.added, edited.edits, edited.known), (0, 1, 3), "{edited:?}");
        assert_eq!(num(&ours, "SELECT count(*) FROM person_mention WHERE first_name = 'Евпраксия'"), 0);
        let text: String = ours.query_row("SELECT text FROM review_item WHERE kind = 'merge_edit' AND done = 0", [], |r| r.get(0)).unwrap();
        assert!(text.contains("у вас «Евдокия», у помощника «Евпраксия»"), "{text}");
        // О той же правке второй раз не спрашиваем.
        let same = run(&ours, &theirs, "помощник.sqlite", true).unwrap();
        assert_eq!((same.edits, same.known), (0, 4));
        assert_eq!(num(&ours, "SELECT count(*) FROM review_item WHERE kind = 'merge_edit'"), 1);
        // «Взять правку»: привязка скана у нашей записи остаётся нашей.
        ours.execute("UPDATE entry SET scan_file = 'наш лист.jpg' WHERE id = (SELECT entry_id FROM merge_origin WHERE source_entry = 1)", []).unwrap();
        let review_id = num(&ours, "SELECT id FROM review_item WHERE kind = 'merge_edit'");
        let taken = take(&ours, review_id).unwrap();
        assert!(!taken.restored);
        assert_eq!(num(&ours, "SELECT count(*) FROM person_mention WHERE first_name = 'Евпраксия' AND entry_id = (SELECT entry_id FROM merge_origin WHERE source_entry = 1)"), 1);
        assert_eq!(num(&ours, "SELECT count(*) FROM entry"), before + 3);
        assert_eq!(num(&ours, "SELECT count(*) FROM review_item WHERE kind = 'merge_edit' AND done = 0"), 0);
        assert_eq!(num(&ours, "SELECT count(*) FROM entry WHERE scan_file = 'наш лист.jpg'"), 1, "скан не снят");
        assert!(take(&ours, review_id).is_err(), "вторично взять нечего");
        // Удалили запись, у которой ждала правка помощника, — следующее слияние
        // говорит «у вас удалено», а не молчит (ревьюер 11.11.2026).
        let pend_id = num(&ours, "SELECT entry_id FROM merge_origin WHERE source_entry = 3");
        ours.execute("UPDATE merge_origin SET pending = 'x', pending_fp = fingerprint WHERE source_entry = 3", []).unwrap();
        crate::records::delete_entry(&ours, pend_id).unwrap();
        assert_eq!(run(&ours, &theirs, "помощник.sqlite", false).unwrap().deleted, 1);
        // Вернуть, как было: дальше история идёт с этой записью.
        let back_id = take(&ours, {
            run(&ours, &theirs, "помощник.sqlite", true).unwrap();
            num(&ours, "SELECT id FROM review_item WHERE kind = 'merge_deleted' AND done = 0")
        }).unwrap();
        assert!(back_id.restored);
        // Правка в поле, которого нет в списке отличий (пол), молча не теряется.
        theirs.execute("UPDATE person_mention SET gender = 'Ж' WHERE role_code = 'child' AND entry_id = 1", []).unwrap();
        let hidden = run(&ours, &theirs, "помощник.sqlite", true).unwrap();
        assert_eq!(hidden.edits, 1, "{hidden:?}");
        let text: String = ours.query_row("SELECT text FROM review_item WHERE kind = 'merge_edit' AND done = 0", [], |r| r.get(0)).unwrap();
        assert!(text.contains("не показаны"), "{text}");
        take(&ours, num(&ours, "SELECT id FROM review_item WHERE kind = 'merge_edit' AND done = 0")).unwrap();
        assert_eq!(num(&ours, "SELECT count(*) FROM person_mention WHERE gender = 'Ж' AND first_name = 'Евпраксия'"), 1);
        assert_eq!(run(&ours, &theirs, "помощник.sqlite", true).unwrap().to_review(), 0);

        // У нас запись удалена, у помощника есть: не возвращается молча.
        let gone_id = num(&ours, "SELECT entry_id FROM merge_origin WHERE source_entry = 2");
        // Удаление записи кнопкой (11.11.2026): память подсказок снята, память
        // слияния — без номера, запись не вернётся молча.
        let before_memory = num(&ours, "SELECT count(*) FROM person_index WHERE iof = 'Павел Петров'");
        assert_eq!(crate::records::delete_entry(&ours, gone_id).unwrap(), 1);
        assert_eq!(num(&ours, "SELECT count(*) FROM person_index WHERE iof = 'Павел Петров'"), before_memory - 1);
        assert_eq!(num(&ours, "SELECT count(*) FROM merge_origin WHERE source_entry = 2 AND entry_id IS NULL"), 1);
        assert_eq!(num(&ours, &format!("SELECT count(*) FROM person_mention WHERE entry_id = {gone_id}")), 0);
        assert!(crate::records::delete_entry(&ours, gone_id).is_err(), "второй раз — понятный отказ");
        // Новая запись не получает номер удалённой в памяти слияния.
        save_entry(&ours, &birth(1891, "1", 1, false, "Новый", ("Пётр", "", "Новиков"), "")).unwrap();
        assert_eq!(num(&ours, "SELECT count(*) FROM merge_origin WHERE source_entry = 2 AND entry_id IS NULL"), 1);
        let deleted = run(&ours, &theirs, "помощник.sqlite", true).unwrap();
        assert_eq!((deleted.added, deleted.deleted), (0, 1), "{deleted:?}");
        assert_eq!(num(&ours, "SELECT count(*) FROM entry"), before + 3);
        assert_eq!(run(&ours, &theirs, "помощник.sqlite", true).unwrap().deleted, 0, "второй раз не спрашиваем");
        let review_id = num(&ours, "SELECT id FROM review_item WHERE kind = 'merge_deleted' AND done = 0");
        let back = take(&ours, review_id).unwrap();
        assert!(back.restored);
        assert_eq!(num(&ours, "SELECT count(*) FROM entry"), before + 4);
        assert_eq!(num(&ours, "SELECT count(*) FROM entry e JOIN mk_case c ON c.id = e.case_id WHERE c.year = 1890"), 2);
        let last = run(&ours, &theirs, "помощник.sqlite", true).unwrap();
        assert_eq!((last.added, last.known, last.to_review()), (0, 4, 0), "{last:?}");

        // Помощник удалил запись у себя — у нас она остаётся, отчёт говорит.
        theirs.execute("DELETE FROM entry WHERE id = 1", []).unwrap();
        let fewer = run(&ours, &theirs, "помощник.sqlite", false).unwrap();
        assert_eq!((fewer.total, fewer.gone), (3, 1));

        // Файл помощника не изменён слиянием.
        assert_eq!(num(&theirs, "SELECT count(*) FROM merge_origin") + num(&theirs, "SELECT count(*) FROM review_item"), 0);
        drop((ours, theirs));
        let _ = std::fs::remove_file(ours_path);
        let _ = std::fs::remove_file(theirs_path);
    }

    /// Дело помощника, все записи которого у нас уже есть (в деле другого
    /// года — импорт «по реквизитам»), пустым у нас не остаётся (ревьюер 11.11.2026).
    #[test]
    fn empty_case_is_not_left() {
        let Some((a_path, ours)) = fresh("empty-a", 1914, "20") else { return };
        let Some((b_path, theirs)) = fresh("empty-b", 1913, "19") else { return };
        // У нас запись 1913 года лежит в деле 1914 года.
        let saved = save_entry(&ours, &birth(1913, "7", 4, false, "Пётр", ("Иван", "Капитонов", ""), "")).unwrap();
        ours.execute("UPDATE entry SET case_id = 1 WHERE id = ?1", [saved.id]).unwrap();
        ours.execute("DELETE FROM mk_case WHERE id <> 1", []).unwrap();
        save_entry(&theirs, &birth(1913, "7", 4, false, "Пётр", ("Иван", "Капитонов", ""), "")).unwrap();
        let done = run(&ours, &theirs, "помощник.sqlite", true).unwrap();
        assert_eq!((done.added, done.duplicates), (0, 1), "{done:?}");
        assert!(done.cases_new.is_empty(), "{:?}", done.cases_new);
        assert_eq!(num(&ours, "SELECT count(*) FROM mk_case"), 1, "пустого дела 1913 года нет");
        drop((ours, theirs));
        let _ = std::fs::remove_file(a_path);
        let _ = std::fs::remove_file(b_path);
    }

    #[test]
    fn wrong_files_are_refused() {
        let Some((a_path, a)) = fresh("a", 1889, "11") else { return };
        let Some((b_path, b)) = fresh("b", 1889, "11") else { return };
        // Копия того же прихода.
        let copy = std::env::temp_dir().join(format!("genmetric-join-copy-{}.sqlite", std::process::id()));
        drop(a);
        std::fs::copy(&a_path, &copy).unwrap();
        let a = Connection::open(&a_path).unwrap();
        let twin = Connection::open(&copy).unwrap();
        assert!(run(&a, &twin, "копия.sqlite", false).unwrap_err().contains("того же прихода"));
        // Прежняя сборка: без метки или старой схемы.
        b.execute("DELETE FROM setting WHERE key = 'parish_uid'", []).unwrap();
        assert!(run(&a, &b, "старый.sqlite", false).unwrap_err().contains("прежней сборкой"));
        b.execute(&statement("merge_uid_ensure").unwrap(), []).unwrap();
        b.execute("DELETE FROM schema_version", []).unwrap();
        b.execute("INSERT INTO schema_version (version) VALUES (12)", []).unwrap();
        assert!(run(&a, &b, "старый.sqlite", false).unwrap_err().contains("прежней сборкой"));
        // Не приход вовсе.
        let other = Connection::open_in_memory().unwrap();
        assert!(run(&a, &other, "чужой.sqlite", false).unwrap_err().contains("не файл прихода"));
        // Пустой приход помощника — сливать нечего, но не ошибка.
        b.execute("DELETE FROM schema_version", []).unwrap();
        b.execute("INSERT INTO schema_version (version) VALUES (?1)", [SCHEMA_VERSION]).unwrap();
        let empty = run(&a, &b, "пустой.sqlite", false).unwrap();
        assert_eq!(empty.total, 0);
        assert!(!empty.warnings.is_empty());
        drop((a, b, twin));
        for p in [a_path, b_path, copy] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn fingerprint_ignores_scan_and_spaces() {
        let mut a = birth(1890, "3", 1, true, "Евдокия", ("Яков", "Ильин", ""), "Малово");
        let print = fingerprint(&a);
        a.scan_file = Some(Some("лист 5.jpg".into()));
        a.page = Some(" 3 ".into());
        assert_eq!(fingerprint(&a), print);
        a.persons[0].first_name = Some("Евпраксия".into());
        assert_ne!(fingerprint(&a), print);
    }
}

#[cfg(test)]
mod real {
    /// На настоящих базах (обе — открытые этой сборкой, схема 13):
    /// `GENMETRIC_MERGE_DB=/наш.sqlite GENMETRIC_MERGE_SRC=/помощник.sqlite cargo test -p
    /// genmetric-core --release merge_real -- --ignored --nocapture` — предпросмотр
    /// и время; `GENMETRIC_MERGE_COMMIT=1` — слить на самом деле (на копии!).
    #[test]
    #[ignore]
    fn merge_real() {
        let (Ok(ours), Ok(theirs)) = (std::env::var("GENMETRIC_MERGE_DB"), std::env::var("GENMETRIC_MERGE_SRC")) else { return };
        let conn = rusqlite::Connection::open(&ours).unwrap();
        let src = rusqlite::Connection::open(&theirs).unwrap();
        let commit = std::env::var("GENMETRIC_MERGE_COMMIT").is_ok();
        let started = std::time::Instant::now();
        let report = super::run(&conn, &src, "помощник.sqlite", commit).unwrap();
        eprintln!("{} за {:.1} с: всего {}, новых {}, дублей {}, слито раньше {}, правок {}, удалённых {}, одно место {}; дел новых {}, пунктов новых {}",
                  if commit { "слито" } else { "предпросмотр" }, started.elapsed().as_secs_f32(),
                  report.total, report.added, report.duplicates, report.known, report.edits, report.deleted, report.twins,
                  report.cases_new.len(), report.places_new);
        for line in report.warnings.iter().chain(&report.case_diffs).chain(&report.place_diffs).take(15) {
            eprintln!("    {line}");
        }
    }
}
