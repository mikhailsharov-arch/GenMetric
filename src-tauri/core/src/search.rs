//! Поиск персоны по приходу и её досье (спека 2026-10-09-okno-poiska-persony).
//!
//! Роман 09.10.2026: «Я хочу просто вбить интересующего человека и мгновенно
//! увидеть всё о нем… увидеть полный список всех его детей, а также все
//! события, где он был поручителем или восприемником». Сейчас это три
//! фильтра в выгрузке Excel: ИОФ → НП → галочки ролей.
//!
//! Записи в базе между собой не связаны (`person_id` пуст): «персона» здесь —
//! совпадение ИОФ в современном написании и НП, а не установленная личность.
//! Поэтому в досье попадает только то, где стоит её ИОФ: она — жених, отец,
//! умершая, восприемник, поручитель, родственник. Её собственное рождение
//! (у ребёнка в записи одно имя) — следующий шаг, там нужна догадка.
//!
//! Только чтение. Сопоставление слов — здесь, а не в SQL: сравнение без учёта
//! регистра в SQLite кириллицу не понимает (грабля про COLLATE NOCASE).

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::statement;
use crate::text::{normalize, normalize_name};

/// Что набрано на экране «Поиск».
#[derive(Deserialize, Default, Clone, Debug)]
pub struct Filter {
    /// Слова ИОФ — в любом порядке, каждое — начало имени, отчества или фамилии.
    pub query: String,
    /// Название пункта как в справочнике; пусто — все пункты.
    pub place: Option<String>,
    /// «Свои события»: жених или невеста, отец или мать, умерший.
    pub own: bool,
    /// «Участие»: восприемник, поручитель, родственник.
    pub part: bool,
    /// Причт — стоит в каждой записи, изначально выключен.
    pub clergy: bool,
    pub year_from: Option<i64>,
    pub year_to: Option<i64>,
}

/// Строка списка найденных: один ИОФ в одном НП.
#[derive(Serialize, Debug, PartialEq)]
pub struct PersonHit {
    /// Ключ персоны — с ним спрашивают досье.
    pub key: String,
    /// ИОФ в современном написании.
    pub iof: String,
    pub place: String,
    pub mentions: usize,
    pub year_from: Option<i64>,
    pub year_to: Option<i64>,
}

#[derive(Serialize, Debug)]
pub struct Found {
    pub persons: Vec<PersonHit>,
    /// Сколько персон подошло всего (в `persons` — не больше предела).
    pub total: usize,
}

/// Человек в записи — сама персона или тот, кто стоит рядом.
#[derive(Serialize, Debug, Clone)]
pub struct Mention {
    /// Ключ персоны — по нему (и по НП) открывают досье именно этого человека,
    /// а не самого частого из тех, кого находит его ИОФ.
    pub key: String,
    pub role_code: String,
    /// ИОФ как в записи.
    pub iof: String,
    /// ИОФ в современном написании — им ищут досье этого человека.
    pub iof_modern: String,
    pub place: String,
    pub rank: Option<String>,
    pub gender: Option<String>,
    /// Возраст как в книге («3 мес») или число лет.
    pub age: Option<String>,
    pub death_cause: Option<String>,
    pub marriage_order: Option<String>,
    pub kinship: Option<String>,
    /// У поручителя — сторона («по жениху»), у прочих — примечание.
    pub note: Option<String>,
}

/// Событие досье: запись, в которой персона упомянута.
#[derive(Serialize, Debug)]
pub struct Event {
    pub entry_id: i64,
    /// 1 — рождение, 2 — брак, 3 — смерть.
    pub section: i64,
    pub year: Option<i64>,
    pub day: Option<i64>,
    pub month: Option<i64>,
    pub page: Option<String>,
    pub note: Option<String>,
    /// Сама персона в этой записи.
    pub me: Mention,
    /// Остальные люди записи, кроме причта, — в порядке записи.
    pub others: Vec<Mention>,
}

#[derive(Serialize, Debug)]
pub struct Dossier {
    pub iof: String,
    pub place: String,
    /// Звания, какие встречались, — от частого к редкому.
    pub ranks: Vec<String>,
    pub mentions: usize,
    pub events: Vec<Event>,
}

/// Больше персон список не показывает — «уточните запрос».
pub const PERSON_LIMIT: usize = 200;

struct Row {
    entry_id: i64,
    role: String,
    first: String,
    patr: String,
    surname: String,
    first_modern: String,
    patr_modern: String,
    gender: Option<String>,
    rank: Option<String>,
    place: String,
    age: Option<String>,
    death_cause: Option<String>,
    marriage_order: Option<String>,
    kinship: Option<String>,
    note: Option<String>,
    section: i64,
    year: Option<i64>,
    day: Option<i64>,
    month: Option<i64>,
    page: Option<String>,
    entry_note: Option<String>,
    /// Ключ персоны и слова, по которым её находят, — считаются один раз при
    /// чтении: поиск идёт на каждую набранную букву.
    key: String,
    words: Vec<String>,
    place_norm: String,
}

/// Упоминания прихода в памяти. До 10.10.2026 поиск и досье читали их из
/// базы каждый раз — на ста тысячах упоминаний по 0,3 с на каждую букву.
/// Кто держит `Index` между вызовами, сам следит, что база не изменилась
/// (`changes`).
pub struct Index {
    rows: Vec<Row>,
}

impl Index {
    pub fn load(conn: &Connection) -> Result<Index, String> {
        Ok(Index { rows: load(conn)? })
    }
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Сколько строк изменило это соединение с открытия: число растёт с каждым
/// сохранением, правкой и импортом — по нему видно, что память поиска
/// устарела. У нового соединения счёт свой, поэтому при смене прихода память
/// сбрасывают отдельно.
pub fn changes(conn: &Connection) -> Result<i64, String> {
    conn.query_row("SELECT total_changes()", [], |r| r.get(0)).map_err(|e| e.to_string())
}

/// К какой из трёх групп относится роль. Ребёнок в записи о рождении — не
/// «персона»: у него одно имя, и по нему его не отличить от сотен тёзок.
#[derive(PartialEq, Clone, Copy)]
enum Kind { Own, Part, Clergy, Child }

fn kind_of(role: &str) -> Kind {
    match role {
        "child" => Kind::Child,
        "groom" | "bride" | "father" | "mother" | "deceased" => Kind::Own,
        r if r.starts_with("clergy") => Kind::Clergy,
        _ => Kind::Part,
    }
}

fn blank(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn load(conn: &Connection) -> Result<Vec<Row>, String> {
    let mut stmt = conn.prepare(&statement("search_mentions")?).map_err(|e| e.to_string())?;
    let text = |r: &rusqlite::Row, i: usize| -> rusqlite::Result<String> {
        Ok(r.get::<_, Option<String>>(i)?.unwrap_or_default().trim().to_string())
    };
    let rows = stmt
        .query_map([], |r| {
            let years: Option<i64> = r.get(12)?;
            let (months, weeks, days): (Option<i64>, Option<i64>, Option<i64>) = (r.get(13)?, r.get(14)?, r.get(15)?);
            let age_text = blank(r.get(16)?);
            // Возраст — как в книге, если он записан текстом; иначе по частям.
            // Одни годы — числом (слово «лет» допишет окно); годы с месяцами —
            // с единицей у каждой части, иначе вышло бы «1 2 мес».
            let age = age_text.or_else(|| {
                let only_years = months.is_none() && weeks.is_none() && days.is_none();
                let parts: Vec<String> = [(years, "г."), (months, "мес"), (weeks, "нед"), (days, "дн")]
                    .iter()
                    .filter_map(|(v, unit)| v.map(|n| if *unit == "г." && only_years { n.to_string() } else { format!("{n} {unit}") }))
                    .collect();
                if parts.is_empty() { None } else { Some(parts.join(" ")) }
            });
            let mut row = Row {
                entry_id: r.get(1)?,
                role: r.get(2)?,
                first: text(r, 4)?,
                patr: text(r, 5)?,
                surname: text(r, 6)?,
                first_modern: text(r, 7)?,
                patr_modern: text(r, 8)?,
                gender: blank(r.get(9)?),
                rank: blank(r.get(10)?),
                place: text(r, 11)?,
                age,
                death_cause: blank(r.get(17)?),
                marriage_order: blank(r.get(18)?),
                kinship: blank(r.get(19)?),
                note: blank(r.get(20)?),
                section: r.get(21)?,
                year: r.get(22)?,
                day: r.get(23)?,
                month: r.get(24)?,
                page: blank(r.get(25)?),
                entry_note: blank(r.get(26)?),
                key: String::new(),
                words: Vec::new(),
                place_norm: String::new(),
            };
            row.key = row.modern().iter().filter(|s| !s.is_empty()).map(|s| normalize_name(s)).collect::<Vec<_>>().join(" ");
            row.words = [&row.first, &row.patr, &row.surname, &row.first_modern, &row.patr_modern]
                .iter().filter(|s| !s.is_empty()).map(|s| normalize_name(s)).collect();
            row.place_norm = normalize(&row.place);
            Ok(row)
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

impl Row {
    fn modern(&self) -> [&str; 3] {
        [if self.first_modern.is_empty() { &self.first } else { &self.first_modern },
         if self.patr_modern.is_empty() { &self.patr } else { &self.patr_modern },
         &self.surname]
    }
    fn iof(&self) -> String {
        [self.first.as_str(), &self.patr, &self.surname].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" ")
    }
    fn iof_modern(&self) -> String {
        self.modern().iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" ")
    }
    /// Ключ персоны: ИОФ в современном написании, без регистра, «ё» и «ъ».
    /// Слова, по которым её находят, — книжное и современное написание
    /// (`words`). Оба считает `load`.
    fn key(&self) -> String {
        self.key.clone()
    }
    fn mention(&self) -> Mention {
        Mention {
            key: self.key(), role_code: self.role.clone(), iof: self.iof(), iof_modern: self.iof_modern(), place: self.place.clone(),
            rank: self.rank.clone(), gender: self.gender.clone(), age: self.age.clone(),
            death_cause: self.death_cause.clone(), marriage_order: self.marriage_order.clone(),
            kinship: self.kinship.clone(), note: self.note.clone(),
        }
    }
}

/// Подходит ли упоминание под отборы экрана (кроме слов ИОФ).
fn passes(row: &Row, f: &Filter, place: &Option<String>) -> bool {
    let wanted = match kind_of(&row.role) {
        Kind::Own => f.own,
        Kind::Part => f.part,
        Kind::Clergy => f.clergy,
        Kind::Child => false,
    };
    if !wanted || row.first.is_empty() || row.first == crate::records::NO_NAME {
        return false;
    }
    if let Some(p) = place {
        if &row.place_norm != p {
            return false;
        }
    }
    if let Some(from) = f.year_from {
        if row.year.map_or(true, |y| y < from) { return false; }
    }
    if let Some(to) = f.year_to {
        if row.year.map_or(true, |y| y > to) { return false; }
    }
    true
}

fn place_key(f: &Filter) -> Option<String> {
    f.place.as_deref().map(normalize).filter(|p| !p.is_empty())
}

/// Найденные персоны: одна строка — один ИОФ в одном НП. Запрос короче двух
/// букв — пусто: по одной букве подошла бы половина прихода.
pub fn find(conn: &Connection, f: &Filter) -> Result<Found, String> {
    Ok(find_in(&Index::load(conn)?, f))
}

/// То же по упоминаниям, уже прочитанным в память.
pub fn find_in(index: &Index, f: &Filter) -> Found {
    let words: Vec<String> = f.query.split_whitespace().map(normalize_name).filter(|w| !w.is_empty()).collect();
    if words.iter().map(|w| w.chars().count()).sum::<usize>() < 2 {
        return Found { persons: Vec::new(), total: 0 };
    }
    let place = place_key(f);
    struct Group { iof: String, place: String, mentions: usize, from: Option<i64>, to: Option<i64>, words: BTreeSet<String> }
    let mut groups: BTreeMap<(String, String), Group> = BTreeMap::new();
    for row in index.rows.iter().filter(|r| passes(r, f, &place)) {
        let g = groups.entry((row.key(), row.place.clone())).or_insert_with(|| Group {
            iof: row.iof_modern(), place: row.place.clone(), mentions: 0, from: None, to: None, words: BTreeSet::new(),
        });
        g.mentions += 1;
        if let Some(y) = row.year {
            g.from = Some(g.from.map_or(y, |v| v.min(y)));
            g.to = Some(g.to.map_or(y, |v| v.max(y)));
        }
        g.words.extend(row.words.iter().cloned());
    }
    let mut hits: Vec<PersonHit> = groups
        .into_iter()
        // Каждое набранное слово — начало какого-то слова персоны.
        .filter(|(_, g)| words.iter().all(|w| g.words.iter().any(|have| have.starts_with(w.as_str()))))
        .map(|((key, _), g)| PersonHit { key, iof: g.iof, place: g.place, mentions: g.mentions, year_from: g.from, year_to: g.to })
        .collect();
    hits.sort_by(|a, b| b.mentions.cmp(&a.mentions).then(a.iof.cmp(&b.iof)).then(a.place.cmp(&b.place)));
    let total = hits.len();
    hits.truncate(PERSON_LIMIT);
    Found { persons: hits, total }
}

/// Досье персоны: все записи, где стоит её ИОФ (ключ) в этом НП, с теми, кто
/// рядом в записи. Отборы по ролям и годам — те же, что у списка; слова
/// запроса и отбор по НП здесь не действуют — персона уже выбрана.
pub fn dossier(conn: &Connection, key: &str, place: &str, f: &Filter) -> Result<Dossier, String> {
    Ok(dossier_in(&Index::load(conn)?, key, place, f))
}

/// То же по упоминаниям, уже прочитанным в память.
pub fn dossier_in(index: &Index, key: &str, place: &str, f: &Filter) -> Dossier {
    let rows = &index.rows;
    let only = Filter { place: None, ..f.clone() };
    let mine: Vec<&Row> = rows.iter()
        .filter(|r| r.place == place && r.key == key && passes(r, &only, &None))
        .collect();
    let mut by_entry: BTreeMap<i64, Vec<&Row>> = BTreeMap::new();
    let wanted: BTreeSet<i64> = mine.iter().map(|r| r.entry_id).collect();
    for row in rows.iter().filter(|r| wanted.contains(&r.entry_id)) {
        by_entry.entry(row.entry_id).or_default().push(row);
    }
    let mut ranks: BTreeMap<String, usize> = BTreeMap::new();
    let mut events: Vec<Event> = Vec::new();
    for me in &mine {
        if let Some(rank) = &me.rank {
            *ranks.entry(rank.clone()).or_default() += 1;
        }
        let others = by_entry.get(&me.entry_id).map(|all| {
            all.iter()
                .filter(|r| !std::ptr::eq(**r, *me) && kind_of(&r.role) != Kind::Clergy
                            && (!r.first.is_empty() || !r.surname.is_empty() || r.role == "deceased"))
                .map(|r| r.mention())
                .collect()
        }).unwrap_or_default();
        events.push(Event {
            entry_id: me.entry_id, section: me.section,
            year: me.year, day: me.day, month: me.month,
            page: me.page.clone(), note: me.entry_note.clone(), me: me.mention(), others,
        });
    }
    events.sort_by_key(|e| (e.year.unwrap_or(i64::MAX), e.month.unwrap_or(13), e.day.unwrap_or(32), e.entry_id));
    let mut ranks: Vec<(String, usize)> = ranks.into_iter().collect();
    ranks.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Dossier {
        iof: mine.first().map(|r| r.iof_modern()).unwrap_or_default(),
        place: place.to_string(),
        ranks: ranks.into_iter().map(|r| r.0).collect(),
        mentions: mine.len(),
        events,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::records::{save_case, save_entry, Case, EntryInput, PersonInput};
    use std::path::Path;

    fn person(role: &str, order: i64, iof: (&str, &str, &str), modern: (&str, &str), place: &str, rank: &str, sex: &str) -> PersonInput {
        let opt = |v: &str| if v.is_empty() { None } else { Some(v.to_string()) };
        PersonInput {
            role_code: role.into(), sort_order: order, surname: opt(iof.2), first_name: opt(iof.0), patronymic: opt(iof.1),
            surname_modern: None, first_name_modern: opt(modern.0), patronymic_modern: opt(modern.1),
            maiden_surname: None, gender: opt(sex), rank: opt(rank), confession: None, place: opt(place), note: None,
            uncertain: None, age_years: None, marriage_order: None, kinship: None, age_months: None, age_weeks: None,
            age_days: None, age_text: None, death_cause: None,
        }
    }

    fn entry(section: i64, year: i64, day: i64, month: i64, page: &str, persons: Vec<PersonInput>) -> EntryInput {
        EntryInput {
            id: None, case_id: 1, section, page: Some(page.into()), no_male: Some(1), no_female: None,
            event_day: Some(day), event_month: Some(month), event_year: Some(year), rite_day: None, rite_month: None,
            rite_year: if section == 2 { None } else { Some(year) }, note: None, uncertain: None, persons,
        }
    }

    fn fresh(tag: &str) -> Option<(std::path::PathBuf, Connection)> {
        let seed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../resources/seed.sqlite");
        if !seed.exists() {
            crate::seed_missing("тест поиска пропущен");
            return None;
        }
        let path = std::env::temp_dir().join(format!("genmetric-search-{tag}-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::copy(&seed, &path).unwrap();
        let conn = Connection::open(&path).unwrap();
        let case = Case { id: 0, archive: None, fond: None, opis: None, delo: None, church: Some("Ц".into()),
                          village: Some("Тестово".into()), uyezd: None, guberniya: None, year: Some(1889), indexer: None };
        save_case(&conn, &case, false).unwrap();
        Some((path, conn))
    }

    fn all() -> Filter {
        Filter { own: true, part: true, ..Filter::default() }
    }

    /// Жизнь «Ивана Капитонова» из Фетинина и его тёзки из Воспицы.
    fn story(conn: &Connection) {
        let ivan = |role: &str, order: i64, place: &str| person(role, order, ("Иоаннъ", "Капитоновъ", ""), ("Иван", "Капитонович"), place, "крестьянин", "М");
        let priest = person("clergy1", 100, ("Александр", "", "Рождественский"), ("Александр", ""), "", "священник", "М");
        // 1889 — рождение дочери Марии; 1891 — сына Петра от той же жены.
        for (year, day, month, child, sex, page) in [(1889, 12, 3, "Мария", "Ж", "14"), (1891, 2, 11, "Пётр", "М", "40об")] {
            save_entry(conn, &entry(1, year, day, month, page, vec![
                person("child", 10, (child, "", ""), (child, ""), "", "", sex),
                ivan("father", 20, "Фетинино"),
                person("mother", 30, ("Анна", "Петрова", ""), ("Анна", "Петровна"), "Фетинино", "законная жена его", "Ж"),
                person("godparent1", 40, ("Пётр", "Сидоров", "Орлов"), ("Пётр", "Сидорович"), "Малово", "крестьянин", "М"),
                priest_clone(&priest),
            ])).unwrap();
        }
        // 1890 — он восприемник у чужого ребёнка.
        save_entry(conn, &entry(1, 1890, 5, 6, "20", vec![
            person("child", 10, ("Фёдор", "", ""), ("Фёдор", ""), "", "", "М"),
            person("father", 20, ("Семён", "Иванов", ""), ("Семён", "Иванович"), "Фетинино", "крестьянин", "М"),
            ivan("godparent1", 40, "Фетинино"),
            priest_clone(&priest),
        ])).unwrap();
        // 1893 — поручитель на свадьбе; 1897 — умер.
        let mut witness = ivan("witness1", 60, "Фетинино");
        witness.note = Some("по жениху".into());
        let mut groom = person("groom", 10, ("Фёдор", "Иванов", ""), ("Фёдор", "Иванович"), "Малово", "крестьянский сын", "М");
        groom.age_years = Some(22);
        groom.marriage_order = Some("Первым браком".into());
        save_entry(conn, &entry(2, 1893, 20, 1, "3", vec![
            groom, person("bride", 20, ("Дарья", "Петрова", ""), ("Дарья", "Петровна"), "Воспица", "крестьянская дочь-девица", "Ж"),
            witness, priest_clone(&priest),
        ])).unwrap();
        let mut dead = ivan("deceased", 10, "Фетинино");
        dead.age_years = Some(60);
        dead.age_text = Some("60".into());
        dead.death_cause = Some("чахотка".into());
        save_entry(conn, &entry(3, 1897, 5, 5, "51", vec![dead, priest_clone(&priest)])).unwrap();
        // Тёзка из другой деревни — один раз отцом.
        save_entry(conn, &entry(1, 1895, 7, 7, "22", vec![
            person("child", 10, ("Олимпиада", "", ""), ("Олимпиада", ""), "", "", "Ж"),
            person("father", 20, ("Иван", "Капитонов", ""), ("Иван", "Капитонович"), "Воспица", "крестьянин", "М"),
        ])).unwrap();
    }

    fn priest_clone(p: &PersonInput) -> PersonInput {
        person(&p.role_code, p.sort_order, (p.first_name.as_deref().unwrap_or(""), "", p.surname.as_deref().unwrap_or("")),
               (p.first_name_modern.as_deref().unwrap_or(""), ""), "", p.rank.as_deref().unwrap_or(""), "М")
    }

    #[test]
    fn finds_person_by_words() {
        let Some((path, conn)) = fresh("find") else { return };
        story(&conn);
        let hits = |f: Filter| -> Vec<(String, String, usize)> {
            find(&conn, &f).unwrap().persons.into_iter().map(|p| (p.iof, p.place, p.mentions)).collect()
        };
        let q = |text: &str| Filter { query: text.into(), ..all() };
        let both = vec![("Иван Капитонович".to_string(), "Фетинино".to_string(), 5),
                        ("Иван Капитонович".to_string(), "Воспица".to_string(), 1)];
        assert_eq!(hits(q("кап ив")), both, "слова в любом порядке и не целиком; одна строка — ИОФ в одном НП");
        assert_eq!(hits(q("ИОАНН капитонов")), both[..1].to_vec(), "книжное написание, регистр, «ъ» — у того, кто так записан");
        assert_eq!(hits(q("иван капитонович")), both, "современное написание");
        assert_eq!(hits(q("иван сидор")), vec![], "каждое слово должно найтись у одной персоны");
        assert_eq!(hits(q("и")), vec![], "с одной буквы поиск не идёт");
        assert_eq!(hits(q("фёдор")).len(), 1, "ребёнок «Фёдор» персоной не считается, жених — считается");
        assert_eq!(hits(q("федор"))[0].0, "Фёдор Иванович", "«ё» и «е» — одно");

        assert_eq!(hits(Filter { place: Some("фетинино".into()), ..q("иван кап") }),
                   vec![("Иван Капитонович".to_string(), "Фетинино".to_string(), 5)], "отбор по НП отсекает тёзку");
        assert_eq!(hits(Filter { part: false, ..q("иван кап") })[0].2, 3, "только свои события: дважды отец и умер");
        assert_eq!(hits(Filter { own: false, ..q("иван кап") }),
                   vec![("Иван Капитонович".to_string(), "Фетинино".to_string(), 2)], "только участие: восприемник и поручитель");
        assert_eq!(hits(Filter { year_from: Some(1890), year_to: Some(1893), ..q("иван кап") })[0].2, 3, "годы от и до");
        assert_eq!(hits(q("рождеств")), vec![], "причт без переключателя не ищется");
        assert_eq!(hits(Filter { clergy: true, ..q("рождеств") }),
                   vec![("Александр Рождественский".to_string(), String::new(), 5)], "с переключателем — ищется");
        let found = find(&conn, &q("иван кап")).unwrap();
        assert_eq!((found.total, found.persons[0].year_from, found.persons[0].year_to), (2, Some(1889), Some(1897)));
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn builds_dossier() {
        let Some((path, conn)) = fresh("dossier") else { return };
        story(&conn);
        let key = find(&conn, &Filter { query: "иван кап".into(), ..all() }).unwrap().persons.remove(0).key;
        let d = dossier(&conn, &key, "Фетинино", &all()).unwrap();
        assert_eq!((d.iof.as_str(), d.place.as_str(), d.mentions), ("Иван Капитонович", "Фетинино", 5));
        assert_eq!(d.ranks, vec!["крестьянин".to_string()]);
        let line: Vec<(i64, i64, String)> = d.events.iter().map(|e| (e.year.unwrap(), e.section, e.me.role_code.clone())).collect();
        assert_eq!(line, vec![(1889, 1, "father".into()), (1890, 1, "godparent1".into()), (1891, 1, "father".into()),
                              (1893, 2, "witness1".into()), (1897, 3, "deceased".into())], "события по порядку лет");
        // Рождение дочери: ребёнок, мать, восприемник — рядом; причта и его самого среди «рядом» нет.
        let birth = &d.events[0];
        assert_eq!((birth.day, birth.month, birth.page.as_deref()), (Some(12), Some(3), Some("14")));
        let near: Vec<(&str, &str, &str)> = birth.others.iter().map(|m| (m.role_code.as_str(), m.iof.as_str(), m.place.as_str())).collect();
        assert_eq!(near, vec![("child", "Мария", ""), ("mother", "Анна Петрова", "Фетинино"), ("godparent1", "Пётр Сидоров Орлов", "Малово")]);
        assert_eq!(birth.me.iof, "Иоаннъ Капитоновъ", "ИОФ в строке — как в записи");
        assert_eq!(birth.others[2].iof_modern, "Пётр Сидорович Орлов", "по современному ИОФ ищут досье соседа");
        let neighbour = &birth.others[2];
        let his = dossier(&conn, &neighbour.key, &neighbour.place, &all()).unwrap();
        assert_eq!((his.iof.as_str(), his.mentions), ("Пётр Сидорович Орлов", 2), "ключ соседа открывает именно его досье");
        // Чужое рождение: видно, чей ребёнок.
        assert!(d.events[1].others.iter().any(|m| m.role_code == "father" && m.iof == "Семён Иванов"));
        // Свадьба: сторона поручителя, жених с возрастом и «каким браком».
        let wedding = &d.events[3];
        assert_eq!(wedding.me.note.as_deref(), Some("по жениху"));
        let groom = wedding.others.iter().find(|m| m.role_code == "groom").unwrap();
        assert_eq!((groom.age.as_deref(), groom.marriage_order.as_deref()), (Some("22"), Some("Первым браком")));
        // Смерть: возраст и причина.
        assert_eq!((d.events[4].me.age.as_deref(), d.events[4].me.death_cause.as_deref()), (Some("60"), Some("чахотка")));

        let own = dossier(&conn, &key, "Фетинино", &Filter { part: false, ..all() }).unwrap();
        assert_eq!(own.events.len(), 3, "переключатель ролей действует и в досье");
        let twin = dossier(&conn, &key, "Воспица", &all()).unwrap();
        assert_eq!((twin.mentions, twin.events[0].others[0].iof.as_str()), (1, "Олимпиада"), "тёзка из Воспицы — своё досье");
        assert_eq!(dossier(&conn, "нет такого", "Фетинино", &all()).unwrap().mentions, 0);
        // Рождение 30 декабря 1899 года, крещение уже в 1900-м: год события —
        // 1899, по нему и отбор, и строка.
        let mut late = entry(1, 1899, 30, 12, "99", vec![
            person("child", 10, ("Тит", "", ""), ("Тит", ""), "", "", "М"),
            person("father", 20, ("Савва", "Титов", "Позднев"), ("Савва", "Титович"), "Малово", "крестьянин", "М"),
        ]);
        late.rite_year = Some(1900);
        save_entry(&conn, &late).unwrap();
        let savva = Filter { query: "позднев".into(), ..all() };
        assert_eq!(find(&conn, &Filter { year_to: Some(1899), ..savva.clone() }).unwrap().total, 1, "отбор «до 1899» видит событие 1899 года");
        assert_eq!(find(&conn, &Filter { year_from: Some(1900), ..savva.clone() }).unwrap().total, 0);
        let hit = find(&conn, &savva).unwrap().persons.remove(0);
        assert_eq!((hit.year_from, hit.year_to), (Some(1899), Some(1899)));
        assert_eq!(dossier(&conn, &hit.key, &hit.place, &all()).unwrap().events[0].year, Some(1899));
        // Женщина с фамилией и без — разные персоны одного пункта; ключ ведёт к своей.
        for (surname, times) in [("", 1), ("Иванова", 3)] {
            for i in 0..times {
                save_entry(&conn, &entry(1, 1894, 1 + i, 2, "5", vec![
                    person("child", 10, ("Яков", "", ""), ("Яков", ""), "", "", "М"),
                    person("mother", 30, ("Анна", "Петрова", surname), ("Анна", "Петровна"), "Малово", "", "Ж"),
                ])).unwrap();
            }
        }
        let annas = find(&conn, &Filter { query: "анна петровна".into(), place: Some("Малово".into()), ..all() }).unwrap().persons;
        assert_eq!(annas.iter().map(|p| (p.iof.as_str(), p.mentions)).collect::<Vec<_>>(),
                   vec![("Анна Петровна Иванова", 3), ("Анна Петровна", 1)], "запрос находит обеих, частая — первой");
        let plain = annas.iter().find(|p| p.iof == "Анна Петровна").unwrap();
        assert_eq!(dossier(&conn, &plain.key, "Малово", &all()).unwrap().mentions, 1, "ключ различает их");
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    /// Память поиска: по прочитанным упоминаниям ответы те же, что из базы, а
    /// счётчик изменений растёт с каждым сохранением — по нему память и
    /// сбрасывают. Без сброса новая запись в поиске не появилась бы.
    #[test]
    fn index_answers_and_goes_stale() {
        let Some((path, conn)) = fresh("index") else { return };
        story(&conn);
        let q = Filter { query: "иван кап".into(), ..all() };
        let index = Index::load(&conn).unwrap();
        assert!(!index.is_empty() && index.len() >= 20, "{}", index.len());
        assert_eq!(find_in(&index, &q).persons, find(&conn, &q).unwrap().persons);
        let direct = dossier(&conn, "иван капитонович", "Фетинино", &all()).unwrap();
        let cached = dossier_in(&index, "иван капитонович", "Фетинино", &all());
        assert_eq!((cached.mentions, cached.events.len(), cached.ranks.clone()), (direct.mentions, direct.events.len(), direct.ranks));
        assert_eq!(cached.mentions, 5);

        let before = changes(&conn).unwrap();
        assert_eq!(changes(&conn).unwrap(), before, "чтение счётчик не двигает");
        let _ = find(&conn, &q).unwrap();
        assert_eq!(changes(&conn).unwrap(), before, "поиск базу не меняет");
        save_entry(&conn, &entry(1, 1896, 9, 9, "30", vec![
            person("child", 10, ("Анна", "", ""), ("Анна", ""), "", "", "Ж"),
            person("father", 20, ("Иоаннъ", "Капитоновъ", ""), ("Иван", "Капитонович"), "Фетинино", "крестьянин", "М"),
        ])).unwrap();
        assert!(changes(&conn).unwrap() > before, "сохранение записи видно по счётчику");
        assert_eq!(find_in(&index, &q).persons[0].mentions, 5, "старая память новой записи не знает");
        assert_eq!(find_in(&Index::load(&conn).unwrap(), &q).persons[0].mentions, 6, "прочитанная заново — знает");
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    /// Поиск и досье на настоящей базе — только у разработчика:
    /// GENMETRIC_SEARCH_DB=/путь/база.sqlite GENMETRIC_SEARCH_QUERY="иван кап" \
    ///   cargo test -p genmetric-core --release search_real -- --ignored --nocapture
    #[test]
    #[ignore]
    fn search_real() {
        let Ok(path) = std::env::var("GENMETRIC_SEARCH_DB") else { return };
        let query = std::env::var("GENMETRIC_SEARCH_QUERY").unwrap_or_else(|_| "иван".into());
        let conn = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let f = Filter { query, place: std::env::var("GENMETRIC_SEARCH_PLACE").ok(), ..all() };
        let started = std::time::Instant::now();
        let found = find(&conn, &f).unwrap();
        let took = started.elapsed();
        println!("найдено персон: {} (поиск {took:?})", found.total);
        for p in found.persons.iter().take(12) {
            println!("  {} | {} | упоминаний {} | {:?}–{:?}", p.iof, p.place, p.mentions, p.year_from, p.year_to);
        }
        let Some(first) = found.persons.first() else { return };
        let started = std::time::Instant::now();
        let d = dossier(&conn, &first.key, &first.place, &f).unwrap();
        println!("досье «{}», {} — событий {} (сбор {:?}); звания: {:?}", d.iof, d.place, d.events.len(), started.elapsed(), d.ranks);
        for e in &d.events {
            let near: Vec<String> = e.others.iter().map(|m| format!("{}: {}", m.role_code, m.iof)).collect();
            println!("  {:?}.{:?}.{:?} стр. {:?} — {} [{}] {:?} {:?} | {}", e.day, e.month, e.year, e.page, e.me.role_code,
                     e.me.iof, e.me.age, e.me.death_cause, near.join("; "));
        }
    }

    /// Поиск идёт при наборе: приход в сто тысяч упоминаний не должен
    /// заставлять ждать. Без оптимизации (так тест идёт в конвейере) порог
    /// щедрый — он ловит только грубую ошибку вроде запроса на каждую строку.
    #[test]
    fn search_is_fast_on_big_parish() {
        let Some((path, conn)) = fresh("speed") else { return };
        story(&conn);
        conn.execute_batch("BEGIN").unwrap();
        {
            let mut entry_stmt = conn.prepare("INSERT INTO entry (case_id, section, page, event_day, event_month, event_year, rite_year) VALUES (1, 1, '1', 1, 1, ?1, ?1)").unwrap();
            let mut m = conn.prepare("INSERT INTO person_mention (entry_id, role_code, sort_order, first_name, patronymic, surname, first_name_modern, patronymic_modern, gender, rank) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?4, ?5, 'М', 'крестьянин')").unwrap();
            for i in 0..20_000i64 {
                entry_stmt.execute([1850 + i % 60]).unwrap();
                let id = conn.last_insert_rowid();
                for (k, role) in ["father", "mother", "godparent1", "godparent2", "clergy1"].iter().enumerate() {
                    m.execute(rusqlite::params![id, role, k as i64 * 10, format!("Имя{}", i % 700), format!("Отчествов{}", (i + k as i64) % 300), format!("Фамилин{}", i % 1500)]).unwrap();
                }
            }
        }
        conn.execute_batch("COMMIT").unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM person_mention", [], |r| r.get(0)).unwrap();
        assert!(n >= 100_000, "{n}");
        let started = std::time::Instant::now();
        let found = find(&conn, &Filter { query: "иван кап".into(), ..all() }).unwrap();
        let d = dossier(&conn, &found.persons[0].key, "Фетинино", &all()).unwrap();
        let took = started.elapsed();
        assert_eq!((found.total, d.mentions), (2, 5));
        eprintln!("поиск и досье на {n} упоминаниях: {took:?}");
        // Замер 09.10.2026 на Mac: 0,6 с в собранной программе (поиск и досье
        // вместе, по 0,3 с), около 3 с без оптимизации.
        let limit = if cfg!(debug_assertions) { 60.0 } else { 1.5 };
        assert!(took.as_secs_f64() < limit, "поиск и досье на {n} упоминаниях заняли {took:?}");
        let many = find(&conn, &Filter { query: "имя".into(), ..all() }).unwrap();
        assert!(many.total > PERSON_LIMIT && many.persons.len() == PERSON_LIMIT, "{} / {}", many.total, many.persons.len());
        drop(conn);
        let _ = std::fs::remove_file(path);
    }
}
