//! Поиск персоны по приходу и её досье (спека 2026-10-09-okno-poiska-persony).
//!
//! Роман 09.10.2026: «Я хочу просто вбить интересующего человека и мгновенно
//! увидеть всё о нем… увидеть полный список всех его детей, а также все
//! события, где он был поручителем или восприемником». Сейчас это три
//! фильтра в выгрузке Excel: ИОФ → НП → галочки ролей.
//!
//! Записи в базе между собой не связаны (`person_id` пуст): «персона» здесь —
//! совпадение ИОФ в современном написании и НП, а не установленная личность.
//! В досье попадает то, где стоит её ИОФ: она — жених, отец, умершая,
//! восприемник, поручитель, родственник. С 17.10.2026 к этому прикрепляются
//! её собственное рождение и смерть младенцем, когда они однозначны
//! (`family.rs`, спека 2026-10-15-semya-celikom).
//!
//! Только чтение. Сопоставление слов — здесь, а не в SQL: сравнение без учёта
//! регистра в SQLite кириллицу не понимает (грабля про COLLATE NOCASE).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::statement;
use crate::text::{normalize, normalize_name, normalize_words};

mod family;
use family::Person;

/// Что набрано на экране «Поиск».
#[derive(Deserialize, Default, Clone, Debug)]
pub struct Filter {
    /// Слова ИОФ — в любом порядке; каждое — начало имени, отчества или
    /// фамилии, а слово из трёх и более букв — и середина (`word_match`).
    pub query: String,
    /// Название пункта как в справочнике; пусто — все пункты.
    pub place: Option<String>,
    /// «Свои события»: жених или невеста, отец или мать, умерший.
    pub own: bool,
    /// «Участие»: восприемник, поручитель, родственник.
    pub part: bool,
    /// Причт — стоит в каждой записи, изначально выключен.
    pub clergy: bool,
    /// Роли, выключенные по отдельности (Роман 12.10.2026: «отдельные галочки
    /// по каждой конкретной роли»), — как их называет `role_class`.
    #[serde(default)]
    pub skip: Vec<String>,
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
    /// Как ещё человек записан (женщина — с девичьей фамилией и фамилией
    /// мужа): до двух написаний, кроме заглавного.
    pub also: Vec<String>,
    /// Супруг, с которым он чаще всего записан: тёзки в строке «Возможно»
    /// различимы по нему (долг п. 71, проверяющий 17.10.2026).
    pub spouse: Option<String>,
    /// Роль первого упоминания: по ней экран включает нужную группу ролей,
    /// когда переходит к этой персоне (ссылка «Возможно»).
    pub role: String,
    /// Год собственного рождения, если оно прикреплено.
    pub born: Option<i64>,
    /// Год смерти, если она есть и событий после неё нет.
    pub died: Option<i64>,
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
    /// Год смерти этого человека (его досье), если он умер: «†» у детей в
    /// «Семье» (Роман 16.10.2026: смерти детей-младенцев «абсолютно
    /// необходимо сохранить»).
    pub died: Option<i64>,
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
    /// Файл скана разворота этой записи — ссылка «скан» в досье.
    pub scan_file: Option<String>,
    /// Год книги записи (её дело): папка сканов — у дела.
    pub book_year: Option<i64>,
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
    /// Год собственного рождения, если оно прикреплено, и год смерти.
    pub born: Option<i64>,
    pub died: Option<i64>,
    /// «Возможно, тот же человек» — без слияния (спека, «Общее»).
    pub possible: Vec<PersonHit>,
}

/// Больше персон список не показывает — «уточните запрос».
pub const PERSON_LIMIT: usize = 200;

struct Row {
    entry_id: i64,
    /// Место человека в записи — с номером записи это ключ досье ребёнка и
    /// частей (`family::stable`): номер упоминания меняется при пересохранении.
    sort_order: i64,
    /// Девичья фамилия из поля «Девичья фамилия» — заголовок досье женщины.
    maiden: String,
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
    scan_file: Option<String>,
    book_year: Option<i64>,
    /// Ключ персоны и слова, по которым её находят, — считаются один раз при
    /// чтении: поиск идёт на каждую набранную букву.
    key: String,
    words: Vec<String>,
    place_norm: String,
    /// Возраст годами и «младенец» (месяцы, недели, дни без лет) — для связей.
    age_years: Option<i64>,
    age_small: bool,
    /// Имя и отчество в современном написании, основа фамилии — нормализованные.
    first_n: String,
    /// Имя, сведённое к общему корню вариантов словаря (`search_name_links`).
    first_c: String,
    patr_n: String,
    sur_base: String,
    /// Полное ИОФ ребёнка и умершего младенца (`family::derive`).
    full: Option<String>,
    /// ИО отца и матери в записи (у умершего — родственника) — `family.rs`.
    father_io: String,
    mother_io: String,
    /// Персона (досье), к которой относится упоминание.
    person: usize,
}

/// Упоминания прихода в памяти. До 10.10.2026 поиск и досье читали их из
/// базы каждый раз — на ста тысячах упоминаний по 0,3 с на каждую букву.
/// Кто держит `Index` между вызовами, сам следит, что база не изменилась
/// (`changes`). Персоны и связи между записями (`family.rs`) считаются здесь
/// же, при чтении.
pub struct Index {
    rows: Vec<Row>,
    persons: Vec<Person>,
    /// (ключ, НП) → персона.
    by_key: HashMap<(String, String), usize>,
    /// (ИОФ как в книге, НП) → персоны: подсказка ищет по нему умерших.
    by_book: HashMap<(String, String), Vec<usize>>,
    /// Супруг упоминания (`family::spouses`): строка «Возможно» называет его.
    spouse: Vec<Option<usize>>,
}

impl Index {
    pub fn load(conn: &Connection) -> Result<Index, String> {
        let mut rows = load(conn)?;
        let mut dict = family::Patronymics::new();
        {
            let mut stmt = conn.prepare(&statement("search_patronymics")?).map_err(|e| e.to_string())?;
            let found = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?)))
                .map_err(|e| e.to_string())?;
            for row in found {
                let (name, m, f) = row.map_err(|e| e.to_string())?;
                dict.insert(normalize_name(&name), (m.unwrap_or_default(), f.unwrap_or_default()));
            }
        }
        // Варианты имён — одним корнем (union-find по связям словаря).
        let mut roots: HashMap<String, String> = HashMap::new();
        {
            fn find(roots: &mut HashMap<String, String>, x: &str) -> String {
                let mut cur = x.to_string();
                while let Some(next) = roots.get(&cur).filter(|n| **n != cur).cloned() {
                    cur = next;
                }
                cur
            }
            let mut stmt = conn.prepare(&statement("search_name_links")?).map_err(|e| e.to_string())?;
            let links = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?)))
                .map_err(|e| e.to_string())?;
            for link in links {
                let (name, base, variant) = link.map_err(|e| e.to_string())?;
                let a = find(&mut roots, &normalize_name(&name));
                for other in [base, variant].into_iter().flatten().filter(|v| !v.trim().is_empty()) {
                    let b = find(&mut roots, &normalize_name(&other));
                    if a != b {
                        let (lo, hi) = if a < b { (a.clone(), b) } else { (b, a.clone()) };
                        roots.insert(hi, lo);
                    }
                }
            }
            let names: Vec<String> = roots.keys().cloned().collect();
            for n in names {
                let r = find(&mut roots, &n);
                roots.insert(n, r);
            }
        }
        for row in rows.iter_mut() {
            row.first_c = roots.get(&row.first_n).cloned().unwrap_or_else(|| row.first_n.clone());
        }
        family::derive(&mut rows, &dict);
        let (persons, spouse) = family::build(&mut rows);
        let mut by_key = HashMap::new();
        for (n, p) in persons.iter().enumerate() {
            by_key.insert((p.key.clone(), p.place.clone()), n);
        }
        let mut by_book: HashMap<(String, String), Vec<usize>> = HashMap::new();
        for row in rows.iter().filter(|r| !matches!(kind_of(&r.role), Kind::Child | Kind::Clergy)) {
            let list = by_book.entry((normalize_words(&row.iof()), row.place_norm.clone())).or_default();
            if !list.contains(&row.person) {
                list.push(row.person);
            }
        }
        Ok(Index { rows, persons, by_key, by_book, spouse })
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

/// Группа упоминания: как у роли, но смерть своего ребёнка (родственник
/// умершего «отец», «мать») — «свои события», не «участие» (Роман
/// 19.10.2026).
fn row_kind(row: &Row) -> Kind {
    if row.role.starts_with("deceased_") {
        let kin = normalize(row.kinship.as_deref().unwrap_or(""));
        if kin.starts_with("отец") || kin.starts_with("мать") {
            return Kind::Own;
        }
    }
    kind_of(&row.role)
}

fn kind_of(role: &str) -> Kind {
    match role {
        "child" => Kind::Child,
        "groom" | "bride" | "father" | "mother" | "deceased" => Kind::Own,
        r if r.starts_with("clergy") => Kind::Clergy,
        _ => Kind::Part,
    }
}

/// Роль без номера: «godparent2» → «godparent», «clergy1» → «clergy». По этим
/// именам экран выключает роли по отдельности (`Filter::skip`).
/// Родители и супруги из импорта Excel («bride_parent», «deceased_spouse»)
/// идут с родственниками своей стороны — отдельной галочки у них нет.
pub fn role_class(role: &str) -> &str {
    match role.trim_end_matches(|c: char| c.is_ascii_digit()) {
        "groom_parent" => "groom_relative",
        "bride_parent" => "bride_relative",
        "deceased_parent" | "deceased_spouse" => "deceased_relative",
        other => other,
    }
}

/// Как набранное слово подходит слову персоны: 2 — с начала, 1 — в середине,
/// 0 — никак. В середине ищем только слово из трёх и более букв: по обрывку
/// «рьева» находится «Григорьева» (второй тестировщик, 12.10.2026 — текст,
/// ушедший в корешок), а «ив» в середине нашло бы половину прихода.
fn word_match(have: &str, typed: &str) -> u8 {
    if have.starts_with(typed) { 2 }
    else if typed.chars().count() >= 3 && have.contains(typed) { 1 }
    else { 0 }
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
                sort_order: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                maiden: String::new(),
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
                scan_file: blank(r.get(27)?),
                book_year: r.get(28)?,
                key: String::new(),
                words: Vec::new(),
                place_norm: String::new(),
                age_years: years,
                age_small: years.unwrap_or(0) == 0 && (months.is_some() || weeks.is_some() || days.is_some()),
                first_n: String::new(),
                first_c: String::new(),
                patr_n: String::new(),
                sur_base: String::new(),
                full: None,
                father_io: String::new(),
                mother_io: String::new(),
                person: usize::MAX,
            };
            row.key = row.modern().iter().filter(|s| !s.is_empty()).map(|s| normalize_name(s)).collect::<Vec<_>>().join(" ");
            row.words = [&row.first, &row.patr, &row.surname, &row.first_modern, &row.patr_modern]
                .iter().filter(|s| !s.is_empty()).map(|s| normalize_name(s)).collect();
            // Причт — с селом церкви из «Дела», только при чтении: в запись и
            // выгрузки его НП не идёт (Роман 16.10.2026, ответ 8).
            if row.place.is_empty() && row.role.starts_with("clergy") {
                row.place = text(r, 30)?;
            }
            row.place_norm = normalize(&row.place);
            // Девичья фамилия — ещё одно слово, по которому женщину находят.
            let maiden = text(r, 29)?;
            if !maiden.is_empty() {
                row.words.push(normalize_name(&maiden));
            }
            row.maiden = maiden;
            let [first, patr, _] = row.modern();
            (row.first_n, row.patr_n) = (normalize_name(first), normalize_name(patr));
            row.sur_base = if row.surname.is_empty() { String::new() } else { crate::records::surname_pair(&row.surname).0 };
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
        if let Some(full) = &self.full {
            return full.clone();
        }
        self.modern().iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" ")
    }
    /// Ключ персоны: ИОФ в современном написании, без регистра, «ё» и «ъ».
    /// Слова, по которым её находят, — книжное и современное написание
    /// (`words`). Оба считает `load`.
    fn key(&self) -> String {
        self.key.clone()
    }
    fn mention(&self, persons: &[Person]) -> Mention {
        let person = persons.get(self.person);
        Mention {
            key: person.map_or_else(|| self.key(), |p| p.key.clone()), role_code: self.role.clone(), iof: self.iof(), iof_modern: self.iof_modern(),
            // Место — персоны: с ним досье и спрашивают (ревьюер 17.10.2026).
            place: person.map_or_else(|| self.place.clone(), |p| p.place.clone()),
            rank: self.rank.clone(), gender: self.gender.clone(), age: self.age.clone(),
            death_cause: self.death_cause.clone(), marriage_order: self.marriage_order.clone(),
            kinship: self.kinship.clone(), note: self.note.clone(),
            died: person.and_then(|p| p.died),
        }
    }
}

/// Подходит ли упоминание под отборы экрана (кроме слов ИОФ).
fn passes(row: &Row, f: &Filter, place: &Option<String>) -> bool {
    let wanted = match row_kind(row) {
        Kind::Own => f.own,
        Kind::Part => f.part,
        Kind::Clergy => f.clergy,
        // Своё рождение — «свои события» (спека 2026-10-15-semya-celikom, п. 1.1).
        Kind::Child => f.own,
    };
    if !wanted || row.first.is_empty() || row.first == crate::records::NO_NAME {
        return false;
    }
    if f.skip.iter().any(|s| s == role_class(&row.role)) {
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
    // Каждое набранное слово должно подойти какому-то слову персоны. Персоны,
    // у которых все слова совпали с начала, идут выше тех, у кого хоть одно
    // нашлось в середине: точное попадание не должно тонуть среди обрывков.
    let mut hits: Vec<(bool, PersonHit)> = Vec::new();
    for n in 0..index.persons.len() {
        let mine: Vec<&Row> = index.mine(n, f, &place);
        if mine.is_empty() {
            continue;
        }
        let have: BTreeSet<&String> = mine.iter().flat_map(|r| r.words.iter()).collect();
        let mut exact = true;
        let mut fits = true;
        for w in &words {
            match have.iter().map(|h| word_match(h, w)).max().unwrap_or(0) {
                0 => { fits = false; break; }
                1 => exact = false,
                _ => {}
            }
        }
        if fits {
            hits.push((exact, index.hit(n, &mine)));
        }
    }
    hits.sort_by(|(ea, a), (eb, b)| eb.cmp(ea).then(b.mentions.cmp(&a.mentions)).then(a.iof.cmp(&b.iof)).then(a.place.cmp(&b.place)));
    let total = hits.len();
    hits.truncate(PERSON_LIMIT);
    Found { persons: hits.into_iter().map(|(_, h)| h).collect(), total }
}

impl Index {
    /// Упоминания персоны, прошедшие отборы экрана; влитая персона — пусто.
    fn mine(&self, n: usize, f: &Filter, place: &Option<String>) -> Vec<&Row> {
        let p = &self.persons[n];
        if p.merged_into.is_some() {
            return Vec::new();
        }
        p.rows.iter().map(|&i| &self.rows[i]).filter(|r| passes(r, f, place)).collect()
    }

    /// Строка списка персоны. ИОФ — как у взрослого, если он есть: у
    /// рождения фамилия отца, а сам он мог записываться без неё.
    fn hit(&self, n: usize, mine: &[&Row]) -> PersonHit {
        let p = &self.persons[n];
        let years: Vec<i64> = mine.iter().filter_map(|r| r.year).collect();
        PersonHit {
            key: p.key.clone(),
            iof: Self::title(mine),
            also: {
                let title = Self::title(mine);
                let mut seen: Vec<String> = Vec::new();
                for r in mine.iter().filter(|r| r.role != "child") {
                    let v = r.iof_modern();
                    if v != title && !seen.contains(&v) && seen.len() < 2 {
                        seen.push(v);
                    }
                }
                seen
            },
            role: mine.first().map(|r| r.role.clone()).unwrap_or_default(),
            spouse: {
                let mut count: BTreeMap<String, usize> = BTreeMap::new();
                for &i in &p.rows {
                    if let Some(s) = self.spouse.get(i).copied().flatten() {
                        *count.entry(self.rows[s].iof_modern()).or_default() += 1;
                    }
                }
                count.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0))).map(|(name, _)| name)
            },
            place: p.place.clone(),
            mentions: mine.len(),
            year_from: years.iter().min().copied(),
            year_to: years.iter().max().copied(),
            born: p.birth.and_then(|i| self.rows[i].year),
            died: p.died,
        }
    }

    /// Заголовок досье. Женщина — по последней записи матерью (фамилия мужа),
    /// девичья фамилия — в скобках, если она другая (Роман 19.10.2026: «Мария
    /// Васильева Королева (Алдашкина)»): из невесты или поля «Девичья
    /// фамилия».
    fn dossier_title(mine: &[&Row]) -> String {
        let Some(mother) = mine.iter().rev().find(|r| r.role == "mother") else { return Self::title(mine) };
        // Девичья — из поля «Девичья фамилия», иначе у невесты первым браком:
        // у вдовы во втором браке фамилия прежнего мужа (ревьюер 19.10.2026).
        let first_marriage = |r: &&&Row| {
            let order = normalize(r.marriage_order.as_deref().unwrap_or(""));
            !["втор", "трет", "четв", "вдов"].iter().any(|w| order.contains(w) || normalize(r.rank.as_deref().unwrap_or("")).contains(w))
        };
        let maiden = mine.iter().map(|r| r.maiden.clone()).find(|m| !m.is_empty())
            .or_else(|| mine.iter().filter(|r| r.role == "bride" && !r.surname.is_empty()).find(first_marriage).map(|r| r.surname.clone()));
        let base = mother.iof_modern();
        match maiden {
            Some(m) if crate::records::surname_pair(&m).0 != mother.sur_base => format!("{base} ({m})"),
            _ => base,
        }
    }

    /// Строка списка: женщина — как в последней записи матерью (фамилия мужа),
    /// как и заголовок досье; невестой — в «также» (проверяющий 19.10.2026:
    /// заголовок и список не совпадали).
    fn title(mine: &[&Row]) -> String {
        mine.iter().rev().find(|r| r.role == "mother")
            .or_else(|| mine.iter().find(|r| r.role != "child")).or(mine.first())
            .map(|r| r.iof_modern()).unwrap_or_default()
    }

    /// Персона по ключу и НП; ключ влитой ведёт к той, в которую она влилась.
    fn person(&self, key: &str, place: &str) -> Option<usize> {
        let mut n = *self.by_key.get(&(key.to_string(), place.to_string()))?;
        while let Some(m) = self.persons[n].merged_into {
            n = m;
        }
        Some(n)
    }

    /// Год смерти персоны из подсказки (ИОФ как в книге и НП), если умерли
    /// все персоны с этим ИОФ и НП: живого тёзку в конец не уводим (спека,
    /// п. 1.6).
    pub fn died_of(&self, iof: &str, place: Option<&str>) -> Option<i64> {
        let list = self.by_book.get(&(normalize_words(iof), normalize(place.unwrap_or(""))))?;
        let mut year = None;
        for &n in list {
            year = Some(year.unwrap_or(i64::MIN).max(self.persons[n].died?));
        }
        year
    }
}

/// Полное ИОФ ребёнка для показа под полем (спека 2026-10-15-semya-celikom,
/// п. 1.5): имя ребёнка (современное) + отчество по имени отца + фамилия отца
/// в форме пола ребёнка — то же правило, что у памяти поиска (`family.rs`).
/// Нечего дописать (нет отца, имя не прочитано) — None.
pub fn child_full_iof(conn: &Connection, child: &str, gender: Option<&str>, father: &str, father_name_surname: bool)
    -> Result<Option<String>, String>
{
    let child = child.trim();
    if child.is_empty() || child.starts_with(crate::records::NO_NAME) || father.trim().is_empty() {
        return Ok(None);
    }
    let kid = crate::records::parse_iof_with(conn, child, false)?;
    let dad = crate::records::parse_iof_with(conn, father, father_name_surname)?;
    let Some(dad_first) = dad.first_name_modern.or(dad.first_name).filter(|s| !s.is_empty() && !s.starts_with(crate::records::NO_NAME)) else {
        return Ok(None);
    };
    let female = gender.or(kid.gender.as_deref()) == Some("Ж");
    let first = kid.first_name_modern.or(kid.first_name).unwrap_or_else(|| child.to_string());
    let mut dict = family::Patronymics::new();
    {
        let mut stmt = conn.prepare(&statement("search_patronymics")?).map_err(|e| e.to_string())?;
        let want = normalize_name(&dad_first);
        let found = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?)))
            .map_err(|e| e.to_string())?;
        for row in found {
            let (name, m, f) = row.map_err(|e| e.to_string())?;
            if normalize_name(&name) == want {
                dict.insert(want.clone(), (m.unwrap_or_default(), f.unwrap_or_default()));
            }
        }
    }
    let patr = family::patronymic(&dict, &dad_first, female);
    let surname = dad.surname.filter(|s| !s.trim().is_empty()).map(|s| family::gendered(&s, female));
    if patr.is_none() && surname.is_none() {
        return Ok(None);
    }
    Ok(Some([Some(first), patr, surname].into_iter().flatten().collect::<Vec<_>>().join(" ")))
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
    let n = index.person(key, place);
    let mine: Vec<&Row> = n.map(|n| index.mine(n, &only, &None)).unwrap_or_default();
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
                .map(|r| r.mention(&index.persons))
                .collect()
        }).unwrap_or_default();
        events.push(Event {
            entry_id: me.entry_id, section: me.section,
            year: me.year, day: me.day, month: me.month,
            page: me.page.clone(), note: me.entry_note.clone(), scan_file: me.scan_file.clone(), book_year: me.book_year,
            me: me.mention(&index.persons), others,
        });
    }
    events.sort_by_key(|e| (e.year.unwrap_or(i64::MAX), e.month.unwrap_or(13), e.day.unwrap_or(32), e.entry_id));
    let mut ranks: Vec<(String, usize)> = ranks.into_iter().collect();
    ranks.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let person = n.map(|n| &index.persons[n]);
    // «Возможно» — без отборов по словам, годам и пункту: это подсказка, где
    // ещё посмотреть. Но роли — как на экране: выключили «причт» — причта в
    // «Возможно» нет (долг п. 72, проверяющий 17.10.2026).
    let everything = Filter { own: f.own, part: f.part, clergy: f.clergy, skip: f.skip.clone(), ..Filter::default() };
    let possible = person.map(|p| p.possible.iter().filter_map(|&m| {
        let theirs = index.mine(m, &everything, &None);
        (!theirs.is_empty()).then(|| index.hit(m, &theirs))
    }).collect()).unwrap_or_default();
    Dossier {
        iof: Index::dossier_title(&mine),
        place: place.to_string(),
        ranks: ranks.into_iter().map(|r| r.0).collect(),
        mentions: mine.len(),
        events,
        born: person.and_then(|p| p.birth).and_then(|i| rows[i].year),
        died: person.and_then(|p| p.died),
        possible,
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
            rite_year: if section == 2 { None } else { Some(year) }, note: None, uncertain: None, scan_file: None, persons,
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
        // С 17.10.2026 ребёнок — персона с полным ИОФ: отчество по отцу.
        assert_eq!(hits(q("фёдор")).iter().map(|h| h.0.as_str()).collect::<Vec<_>>(), vec!["Фёдор Иванович", "Фёдор Семенович"],
                   "жених и ребёнок «Фёдор» Семёна Иванова — разные персоны");
        assert_eq!(hits(q("федор"))[0].0, "Фёдор Иванович", "«ё» и «е» — одно");

        assert_eq!(hits(Filter { place: Some("фетинино".into()), ..q("иван кап") }),
                   vec![("Иван Капитонович".to_string(), "Фетинино".to_string(), 5)], "отбор по НП отсекает тёзку");
        assert_eq!(hits(Filter { part: false, ..q("иван кап") })[0].2, 3, "только свои события: дважды отец и умер");
        assert_eq!(hits(Filter { own: false, ..q("иван кап") }),
                   vec![("Иван Капитонович".to_string(), "Фетинино".to_string(), 2)], "только участие: восприемник и поручитель");
        assert_eq!(hits(Filter { year_from: Some(1890), year_to: Some(1893), ..q("иван кап") })[0].2, 3, "годы от и до");
        assert_eq!(hits(q("рождеств")), vec![], "причт без переключателя не ищется");
        assert_eq!(hits(Filter { clergy: true, ..q("рождеств") }),
                   vec![("Александр Рождественский".to_string(), "Тестово".to_string(), 5)], "с переключателем — ищется, с селом «Дела» (п. 2.5)");
        // Слово в середине (12.10.2026): обрывок из трёх и более букв.
        assert_eq!(hits(q("питон ив")), both, "обрывок «питон» находит «Капитонович»");
        assert_eq!(hits(q("ит ив")), vec![], "две буквы в середине слова не ищутся");
        assert_eq!((word_match("григорьева", "рьева"), word_match("иванов", "ив"), word_match("крапивин", "ив"), word_match("крапивин", "пив")),
                   (1, 2, 0, 1), "с начала — 2, в середине с трёх букв — 1");
        // Роли по отдельности.
        assert_eq!(hits(Filter { skip: vec!["father".into()], ..q("иван кап") })[0].2, 3, "без роли «отец»: умер, восприемник, поручитель");
        assert_eq!(hits(Filter { skip: vec!["godparent".into(), "witness".into()], ..q("иван кап") })[0].2, 3, "без восприемника и поручителя");
        assert_eq!((role_class("godparent2"), role_class("clergy1"), role_class("groom_relative"), role_class("deceased_spouse")),
                   ("godparent", "clergy", "groom_relative", "deceased_relative"));
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
        assert_eq!(near, vec![("child", "Мария", "Фетинино"), ("mother", "Анна Петрова", "Фетинино"), ("godparent1", "Пётр Сидоров Орлов", "Малово")]);
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
        // Файл скана записи доезжает до события досье; путь отбрасывается при сохранении.
        let mut with_scan = entry(1, 1898, 3, 3, "60", vec![
            person("child", 10, ("Нил", "", ""), ("Нил", ""), "", "", "М"),
            person("father", 20, ("Нил", "Нилов", "Сканов"), ("Нил", "Нилович"), "Малово", "крестьянин", "М"),
        ]);
        with_scan.scan_file = Some(Some("C:\\сканы\\дело 5\\лист 12.jpg".into()));
        let saved = save_entry(&conn, &with_scan).unwrap();
        let stored: Option<String> = conn.query_row("SELECT scan_file FROM entry WHERE id = ?1", [saved.id], |r| r.get(0)).unwrap();
        assert_eq!(stored.as_deref(), Some("лист 12.jpg"), "в базе — только имя файла");
        let nil = find(&conn, &Filter { query: "сканов".into(), ..all() }).unwrap().persons.remove(0);
        assert_eq!(dossier(&conn, &nil.key, &nil.place, &all()).unwrap().events[0].scan_file.as_deref(), Some("лист 12.jpg"));
        assert_eq!(d.events[0].scan_file, None, "у записи без скана — пусто");
        with_scan.id = Some(saved.id);
        with_scan.scan_file = Some(Some("  ".into()));
        save_entry(&conn, &with_scan).unwrap();
        let cleared: Option<String> = conn.query_row("SELECT scan_file FROM entry WHERE id = ?1", [saved.id], |r| r.get(0)).unwrap();
        assert_eq!(cleared, None, "пустое имя — в базе пусто");
        // Правка без поля скана привязку не трогает; с null — снимает.
        with_scan.scan_file = Some(Some("лист 12.jpg".into()));
        save_entry(&conn, &with_scan).unwrap();
        with_scan.scan_file = None;
        save_entry(&conn, &with_scan).unwrap();
        let kept: Option<String> = conn.query_row("SELECT scan_file FROM entry WHERE id = ?1", [saved.id], |r| r.get(0)).unwrap();
        assert_eq!(kept.as_deref(), Some("лист 12.jpg"), "поля нет — привязка осталась");
        let from_window: EntryInput = serde_json::from_str(r#"{"id":1,"section":1,"case_id":1,"page":null,"no_male":null,"no_female":null,"event_day":null,"event_month":null,"event_year":null,"rite_day":null,"rite_month":null,"rite_year":null,"note":null,"uncertain":null,"scan_file":null,"persons":[]}"#).unwrap();
        assert_eq!(from_window.scan_file, Some(None), "null от окна — «снять привязку», а не «поля нет»");
        let old_window: EntryInput = serde_json::from_str(r#"{"id":1,"section":1,"case_id":1,"page":null,"no_male":null,"no_female":null,"event_day":null,"event_month":null,"event_year":null,"rite_day":null,"rite_month":null,"rite_year":null,"note":null,"uncertain":null,"persons":[]}"#).unwrap();
        assert_eq!(old_window.scan_file, None, "поля нет вовсе");
        with_scan.scan_file = Some(None);
        save_entry(&conn, &with_scan).unwrap();
        let gone: Option<String> = conn.query_row("SELECT scan_file FROM entry WHERE id = ?1", [saved.id], |r| r.get(0)).unwrap();
        assert_eq!(gone, None, "null — привязка снята");
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
        // Ребёнок «Тит Саввич Позднев» — тоже персона (17.10.2026); здесь — отец.
        let savva = Filter { query: "позднев".into(), skip: vec!["child".into()], ..all() };
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

    /// Семья целиком, часть 1 (спека 2026-10-15-semya-celikom): ребёнок —
    /// персона с полным ИОФ, смерть младенца — в его досье, взрослый получает
    /// своё рождение; неоднозначное — «возможно»; база не меняется.
    #[test]
    fn links_birth_death_and_adult() {
        let Some((path, conn)) = fresh("family") else { return };
        let ivan = |role: &str| person(role, 20, ("Иоаннъ", "Капитоновъ", ""), ("Иван", "Капитонович"), "Фетинино", "крестьянин", "М");
        let anna = || person("mother", 30, ("Анна", "Петрова", ""), ("Анна", "Петровна"), "Фетинино", "законная жена его", "Ж");
        let child = |name: &str, sex: &str| person("child", 10, (name, "", ""), (name, ""), "", "", sex);
        let born = |year: i64, name: &str, sex: &str, father: PersonInput| {
            save_entry(&conn, &entry(1, year, 1, 2, "1", vec![child(name, sex), father, anna()])).unwrap();
        };
        let infant_death = |year: i64, name: &str, sex: &str, months: Option<i64>| {
            let mut dead = person("deceased", 10, (name, "", ""), (name, ""), "Фетинино", "", sex);
            dead.age_months = months;
            let mut rel = ivan("deceased_relative");
            rel.sort_order = 40;
            rel.kinship = Some("отец".into());
            save_entry(&conn, &entry(3, year, 3, 4, "9", vec![dead, rel])).unwrap();
        };
        // Мария 1886 умерла в 1887-м пяти месяцев; вторая Мария 1888 — жива.
        born(1886, "Мария", "Ж", ivan("father"));
        infant_death(1887, "Мария", "Ж", Some(5));
        born(1888, "Мария", "Ж", ivan("father"));
        // Анна 1890 и Анна 1891, смерть «Анны» 1892 без возраста — неоднозначно.
        born(1890, "Анна", "Ж", ivan("father"));
        born(1891, "Анна", "Ж", ivan("father"));
        infant_death(1892, "Анна", "Ж", None);
        // Пётр Семёнович Кулаев родился в 1868-м; женихом в 1890-м (22 года),
        // отцом в 1892-м, умер в 1896-м — без фамилии в записях.
        let semyon = || person("father", 20, ("Семёнъ", "Иванов", "Кулаевъ"), ("Семён", "Иванович"), "Фетинино", "крестьянин", "М");
        born(1868, "Пётр", "М", semyon());
        let petr = |role: &str| person(role, 10, ("Петр", "Семенов", ""), ("Пётр", "Семенович"), "Фетинино", "крестьянин", "М");
        let mut groom = petr("groom");
        groom.age_years = Some(22);
        save_entry(&conn, &entry(2, 1890, 5, 5, "3", vec![groom,
            person("bride", 20, ("Дарья", "Петрова", ""), ("Дарья", "Петровна"), "Воспица", "", "Ж")])).unwrap();
        // Мать Ирины — та же Дарья, что невеста (с 17.10.2026 жёны сверяются, п. 2.1).
        save_entry(&conn, &entry(1, 1892, 1, 2, "1", vec![child("Ирина", "Ж"), petr("father"),
            person("mother", 30, ("Дарья", "Петрова", ""), ("Дарья", "Петровна"), "Фетинино", "", "Ж")])).unwrap();
        let mut dead = petr("deceased");
        dead.age_years = Some(28);
        save_entry(&conn, &entry(3, 1896, 1, 1, "50", vec![dead])).unwrap();
        // Два Николая Семёновича родились в Фетинине — у взрослого «возможно».
        born(1870, "Николай", "М", semyon());
        born(1871, "Николай", "М", person("father", 20, ("Семёнъ", "Иванов", ""), ("Семён", "Иванович"), "Фетинино", "", "М"));
        born(1895, "Ольга", "Ж", person("father", 20, ("Николай", "Семенов", ""), ("Николай", "Семенович"), "Фетинино", "", "М"));

        // Отцы-тёзки по ИО с разными фамилиями: смерть Марфы Сидоровой к
        // рождению Марфы Кулаевой не цепляется (ревьюер 17.10.2026).
        born(1893, "Марфа", "Ж", person("father", 20, ("Иван", "Ильин", "Кулаев"), ("Иван", "Ильич"), "Малово", "", "М"));
        let mut marfa = person("deceased", 10, ("Марфа", "", ""), ("Марфа", ""), "Малово", "", "Ж");
        marfa.age_months = Some(2);
        let mut sid = person("deceased_relative", 40, ("Иван", "Ильин", "Сидоров"), ("Иван", "Ильич"), "Малово", "", "М");
        sid.kinship = Some("отец".into());
        save_entry(&conn, &entry(3, 1893, 9, 9, "70", vec![marfa, sid])).unwrap();
        // Неженатый сын 25 лет с отцом-родственником — рождение по возрасту ± 5.
        born(1871, "Яков", "М", ivan("father"));
        let mut yakov = person("deceased", 10, ("Яков", "", ""), ("Яков", ""), "Фетинино", "", "М");
        yakov.age_years = Some(25);
        let mut rel = ivan("deceased_relative");
        rel.sort_order = 40;
        rel.kinship = Some("отец".into());
        save_entry(&conn, &entry(3, 1897, 2, 2, "71", vec![yakov, rel])).unwrap();

        let before = changes(&conn).unwrap();
        let index = Index::load(&conn).unwrap();
        assert_eq!(changes(&conn).unwrap(), before, "связи считаются в памяти, база не меняется");
        let marfas = find_in(&index, &Filter { query: "марфа ивановна".into(), ..all() }).persons;
        assert_eq!(marfas.iter().map(|h| (h.born, h.died)).collect::<Vec<_>>(), vec![(Some(1893), None), (None, Some(1893))],
                   "разные фамилии отцов — смерть отдельно");
        let yakovs = find_in(&index, &Filter { query: "яков иванович".into(), ..all() }).persons;
        assert_eq!(yakovs.iter().map(|h| (h.born, h.died)).collect::<Vec<_>>(), vec![(Some(1871), Some(1897))]);
        let find = |text: &str| find_in(&index, &Filter { query: text.into(), ..all() }).persons;
        let open = |h: &PersonHit| dossier_in(&index, &h.key, &h.place, &all());

        let maria = find("мария ивановна");
        assert_eq!(maria.iter().map(|h| (h.iof.as_str(), h.place.as_str(), h.born, h.died)).collect::<Vec<_>>(),
                   vec![("Мария Ивановна", "Фетинино", Some(1886), Some(1887)), ("Мария Ивановна", "Фетинино", Some(1888), None)],
                   "две Марии одной семьи — разные персоны, первая умерла");
        let first = open(&maria[0]);
        assert_eq!(first.events.iter().map(|e| (e.year.unwrap(), e.me.role_code.as_str())).collect::<Vec<_>>(),
                   vec![(1886, "child"), (1887, "deceased")], "своё рождение и смерть младенцем");
        assert!(first.events[0].others.iter().any(|m| m.role_code == "father" && m.iof == "Иоаннъ Капитоновъ"));

        // У отца — дети с «†» и его родство в смерти дочери, как было.
        let father = open(&find("иван капитонович")[0]);
        let kids: Vec<(i64, Option<i64>)> = father.events.iter().filter(|e| e.me.role_code == "father")
            .map(|e| (e.year.unwrap(), e.others.iter().find(|m| m.role_code == "child").unwrap().died)).collect();
        assert_eq!(kids, vec![(1871, Some(1897)), (1886, Some(1887)), (1888, None), (1890, None), (1891, None)]);
        assert!(father.events.iter().any(|e| e.year == Some(1887) && e.me.role_code == "deceased_relative"),
                "смерть ребёнка в досье отца сохраняется (Роман 16.10.2026)");
        let link = father.events[0].others.iter().find(|m| m.role_code == "child").unwrap();
        assert_eq!(dossier_in(&index, &link.key, &link.place, &all()).events.len(), 2, "ссылка с ребёнка — его досье");

        // Анна: две кандидатки — смерть отдельно, «возможно» у обеих сторон.
        let annas = find("анна ивановна");
        assert_eq!(annas.len(), 3, "две родившиеся и одна умершая");
        let lone = annas.iter().find(|h| h.born.is_none()).unwrap();
        assert_eq!((lone.died, open(lone).possible.len()), (Some(1892), 2));
        assert!(annas.iter().filter(|h| h.born.is_some()).all(|h| h.died.is_none() && open(h).possible.len() == 1));

        // Пётр: рождение влилось во взрослого; умер в 1896-м.
        let petr_hits = find("петр семенович");
        assert_eq!(petr_hits.iter().map(|h| (h.iof.as_str(), h.mentions, h.born, h.died)).collect::<Vec<_>>(),
                   vec![("Пётр Семенович", 4, Some(1868), Some(1896))]);
        let pd = open(&petr_hits[0]);
        assert_eq!(pd.events[0].me.role_code, "child");
        assert!(pd.events[0].others.iter().any(|m| m.role_code == "father" && m.iof == "Семёнъ Иванов Кулаевъ"));
        assert_eq!(find("петр кулаев").len(), 1, "по фамилии отца находится тот же Пётр");
        assert_eq!(index.died_of("Петр Семенов", Some("Фетинино")), Some(1896), "подсказка узнаёт умершего");
        assert_eq!(index.died_of("Иоаннъ Капитоновъ", Some("Фетинино")), None, "живой — нет");
        assert_eq!(index.died_of("Петр Семенов", Some("Воспица")), None, "в другом пункте — не он");

        // Николай: два рождения — не сливаем.
        let nik = find("николай семенович");
        let adult = nik.iter().find(|h| h.born.is_none()).unwrap();
        assert_eq!((nik.len(), open(adult).possible.len()), (3, 2), "взрослый и два рождения, связи «возможно»");
        // Полное ИОФ под полем — то же правило (п. 1.5).
        let full = |child: &str, sex: Option<&str>, father: &str| child_full_iof(&conn, child, sex, father, false).unwrap();
        assert_eq!(full("Мария", Some("Ж"), "Семёнъ Иванов Кулаевъ").as_deref(), Some("Мария Семеновна Кулаева"));
        assert_eq!(full("Пётр", Some("М"), "Иоаннъ Капитоновъ").as_deref(), Some("Петр Иванович"), "имя — современное, как в словаре");
        assert_eq!((full("", None, "Иван"), full("Петр", None, ""), full("***", None, "Иван")), (None, None, None), "нечего показать");
        // Своё рождение выключается ролью, как другие.
        let no_birth = find_in(&index, &Filter { query: "петр семенович".into(), skip: vec!["child".into()], ..all() }).persons;
        assert_eq!(no_birth[0].mentions, 3);
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    fn open_any(index: &Index, h: &PersonHit) -> Dossier {
        dossier_in(index, &h.key, &h.place, &all())
    }

    /// Семья целиком, часть 2 (спека 2026-10-15-semya-celikom, п. 2.1–2.3):
    /// тёзки с разными жёнами, вдовый второй брак, с фамилией и без,
    /// женщина после замужества, долг п. 69.
    #[test]
    fn links_spouses_and_married_women() {
        let Some((path, conn)) = fresh("family2") else { return };
        let man = |role: &str, first: &str, patr: (&str, &str), sur: &str, place: &str| {
            let order = if role == "father" || role == "groom" { 20 } else { 40 };
            person(role, order, (first, patr.0, sur), (first, patr.1), place, "крестьянин", "М")
        };
        let woman = |role: &str, first: &str, patr: (&str, &str), sur: &str, place: &str| {
            person(role, if role == "bride" { 30 } else { 30 }, (first, patr.0, sur), (first, patr.1), place, "", "Ж")
        };
        let kid = |name: &str, sex: &str| person("child", 10, (name, "", ""), (name, ""), "", "", sex);
        let birth = |year: i64, p: Vec<PersonInput>| { save_entry(&conn, &entry(1, year, 1, 3, "1", p)).unwrap(); };
        let wedding = |year: i64, groom: PersonInput, bride: PersonInput| { save_entry(&conn, &entry(2, year, 1, 2, "2", vec![groom, bride])).unwrap(); };
        let ilin = ("Ильин", "Ильич");
        // 1. Иван Петрович из Ушакова: две жены одновременно — два человека;
        //    восприемником — отдельно, «возможно» к обоим.
        birth(1885, vec![kid("Петр", "М"), man("father", "Иван", ("Петров", "Петрович"), "", "Ушаково"), woman("mother", "Мария", ("Иванова", "Ивановна"), "", "Ушаково")]);
        birth(1886, vec![kid("Павел", "М"), man("father", "Иван", ("Петров", "Петрович"), "", "Ушаково"), woman("mother", "Ольга", ("Семенова", "Семеновна"), "", "Ушаково")]);
        birth(1887, vec![kid("Нил", "М"), man("godparent1", "Иван", ("Петров", "Петрович"), "", "Ушаково")]);
        // 2. Семён Ильич: жена Анна умерла в 1882-м, в 1883-м — Ксения: один человек.
        birth(1880, vec![kid("Лука", "М"), man("father", "Семен", ilin, "", "Ушаково"), woman("mother", "Анна", ("Петрова", "Петровна"), "", "Ушаково")]);
        let mut anna = woman("deceased", "Анна", ("Петрова", "Петровна"), "", "Ушаково");
        anna.age_years = Some(30);
        save_entry(&conn, &entry(3, 1882, 5, 5, "3", vec![anna])).unwrap();
        wedding(1883, man("groom", "Семен", ilin, "", "Ушаково"), woman("bride", "Ксения", ("Федорова", "Федоровна"), "", "Ушаково"));
        birth(1884, vec![kid("Фока", "М"), man("father", "Семен", ilin, "", "Ушаково"), woman("mother", "Ксения", ("Федорова", "Федоровна"), "", "Ушаково")]);
        //    Пётр Ильич: «вторым браком» — тоже один.
        birth(1880, vec![kid("Агния", "Ж"), man("father", "Петр", ilin, "", "Ушаково"), woman("mother", "Анна", ("Иванова", "Ивановна"), "", "Ушаково")]);
        let mut second = man("groom", "Петр", ilin, "", "Ушаково");
        second.marriage_order = Some("Вторым браком".into());
        wedding(1884, second, woman("bride", "Евдокия", ("Карпова", "Карповна"), "", "Ушаково"));
        birth(1885, vec![kid("Зоя", "Ж"), man("father", "Петр", ilin, "", "Ушаково"), woman("mother", "Евдокия", ("Карпова", "Карповна"), "", "Ушаково")]);
        // 3. Николай Ильич с фамилией и без, жена та же — одно досье;
        //    Андрей Ильич без жены — «возможно».
        birth(1890, vec![kid("Анна", "Ж"), man("father", "Николай", ilin, "Кулаев", "Ушаково"), woman("mother", "Дарья", ("Иванова", "Ивановна"), "Кулаева", "Ушаково")]);
        birth(1892, vec![kid("Илья", "М"), man("father", "Николай", ilin, "", "Ушаково"), woman("mother", "Дарья", ("Иванова", "Ивановна"), "", "Ушаково")]);
        birth(1890, vec![kid("Ия", "Ж"), man("godparent1", "Андрей", ilin, "Кулаев", "Ушаково")]);
        birth(1891, vec![kid("Ия", "Ж"), man("godparent1", "Андрей", ilin, "", "Ушаково")]);
        // 4. Марья Семёновна Сидорова из Воспицы: родилась в 1866-м, в 1886-м
        //    вышла за Фому Ильича в Ушаково, в 1888-м — мать.
        birth(1866, vec![kid("Марья", "Ж"), man("father", "Семен", ("Иванов", "Иванович"), "Сидоров", "Воспица")]);
        let mut bride = woman("bride", "Марья", ("Семенова", "Семеновна"), "Сидорова", "Воспица");
        bride.age_years = Some(20);
        wedding(1886, man("groom", "Фома", ilin, "", "Ушаково"), bride);
        let mut mother = woman("mother", "Марья", ("Семенова", "Семеновна"), "", "Ушаково");
        mother.maiden_surname = Some("Сидорова".into());
        birth(1888, vec![kid("Петр", "М"), man("father", "Фома", ilin, "", "Ушаково"), mother]);
        // 5. Долг п. 69: Ольга Петровна известна только матерью — своё рождение
        //    в Ушакове ей не прикрепляется.
        birth(1860, vec![kid("Ольга", "Ж"), man("father", "Петр", ("Иванов", "Иванович"), "", "Ушаково")]);
        birth(1889, vec![kid("Яков", "М"), man("father", "Лука", ilin, "", "Ушаково"), woman("mother", "Ольга", ("Петрова", "Петровна"), "", "Ушаково")]);

        // Ревьюер 17.10.2026: жена — «Васса» и «Василиса» (варианты одного
        // имени в словаре) — один муж, не два.
        birth(1881, vec![kid("Агафья", "Ж"), man("father", "Кузьма", ("Власов", "Власович"), "", "Ушаково"), woman("mother", "Васса", ("Петрова", "Петровна"), "", "Ушаково")]);
        birth(1883, vec![kid("Агапит", "М"), man("father", "Кузьма", ("Власов", "Власович"), "", "Ушаково"), woman("mother", "Василиса", ("Иванова", "Ивановна"), "", "Ушаково")]);
        // Безфамильный «Захар Ильич» с женой Марией Ивановной подходит и
        // Кулаеву, и Сидорову с такими же жёнами — не сливаем ни с кем.
        for sur in ["Кулаев", "Сидоров", ""] {
            birth(1893, vec![kid("Ия", "Ж"), man("father", "Захар", ilin, sur, "Ушаково"), woman("mother", "Мария", ("Иванова", "Ивановна"), "", "Ушаково")]);
        }

        // Ж1.1 (19.10.2026): «Третьим браком» при жене, у которой дети и после
        // брака, — двое мужчин; смерть ребёнка — в семье, где он родился.
        let stepan = |role: &str| man(role, "Степан", ilin, "", "Ушаково");
        birth(1885, vec![kid("Ульяна", "Ж"), stepan("father"), woman("mother", "Мария", ("Петрова", "Петровна"), "", "Ушаково")]);
        let mut third = stepan("groom");
        third.marriage_order = Some("Третьим браком".into());
        wedding(1886, third, woman("bride", "Анна", ("Осипова", "Осиповна"), "", "Ушаково"));
        birth(1887, vec![kid("Ульян", "М"), stepan("father"), woman("mother", "Мария", ("Петрова", "Петровна"), "", "Ушаково")]);
        let mut ul = person("deceased", 10, ("Ульян", "", ""), ("Ульян", ""), "Ушаково", "", "М");
        ul.age_months = Some(3);
        let mut dad = stepan("deceased_relative");
        dad.kinship = Some("отец".into());
        save_entry(&conn, &entry(3, 1887, 5, 6, "8", vec![ul, dad])).unwrap();
        birth(1888, vec![kid("Осип", "М"), stepan("father"), woman("mother", "Анна", ("Осипова", "Осиповна"), "", "Ушаково")]);
        // Ж1.2: умершая 2 лет и восприемница-тёзка — разные люди.
        birth(1890, vec![kid("Илья", "М"), woman("godparent2", "Агния", ("Петрова", "Петровна"), "Сизова", "Ушаково")]);
        let mut agnia = woman("deceased", "Агния", ("Петрова", "Петровна"), "Сизова", "Ушаково");
        agnia.age_years = Some(2);
        save_entry(&conn, &entry(3, 1891, 1, 1, "9", vec![agnia])).unwrap();
        // Ж1.3: «Соломония» невестой и «Соломонида» матерью у того же мужа —
        // одна (в словаре они не связаны, отчество то же).
        wedding(1886, man("groom", "Семен", ("Алексеев", "Алексеевич"), "", "Ушаково"),
                woman("bride", "Соломония", ("Яковлева", "Яковлевна"), "", "Унжанка"));
        birth(1888, vec![kid("Анна", "Ж"), man("father", "Семен", ("Алексеев", "Алексеевич"), "", "Ушаково"),
                         woman("mother", "Соломонида", ("Яковлева", "Яковлевна"), "", "Ушаково")]);

        // Ревьюер 19.10.2026: вдовец Тимофей Ильич — Наталья Ивановна умерла,
        // вторым браком Анна Ивановна; то же отчество при разных именах — две жены.
        let tim = |role: &str| man(role, "Тимофей", ilin, "", "Ушаково");
        wedding(1880, tim("groom"), woman("bride", "Наталья", ("Иванова", "Ивановна"), "", "Ушаково"));
        birth(1881, vec![kid("Лев", "М"), tim("father"), woman("mother", "Наталья", ("Иванова", "Ивановна"), "", "Ушаково")]);
        let mut nat = woman("deceased", "Наталья", ("Иванова", "Ивановна"), "", "Ушаково");
        nat.age_years = Some(22);
        save_entry(&conn, &entry(3, 1882, 1, 1, "10", vec![nat])).unwrap();
        let mut second = tim("groom");
        second.marriage_order = Some("Вторым браком".into());
        wedding(1883, second, woman("bride", "Анна", ("Иванова", "Ивановна"), "", "Ушаково"));
        birth(1884, vec![kid("Нил", "М"), tim("father"), woman("mother", "Анна", ("Иванова", "Ивановна"), "", "Ушаково")]);
        // Жених с опиской в возрасте («70» вместо «20») не отрезает у отца
        // полжизни. («50» с 20.10.2026 уже не описка: отцу можно до 75 лет.)
        let mut old = man("groom", "Ефим", ilin, "", "Ушаково");
        old.age_years = Some(70);
        wedding(1880, old, woman("bride", "Ирина", ("Петрова", "Петровна"), "", "Ушаково"));
        for y in [1882, 1885, 1890, 1895, 1900] {
            birth(y, vec![kid("Ия", "Ж"), man("father", "Ефим", ilin, "", "Ушаково"), woman("mother", "Ирина", ("Петрова", "Петровна"), "", "Ушаково")]);
        }

        // Спека 2026-10-20-dosye-i-dovodka, п. 1 (Роман 20.10.2026, «Жары»):
        // Петр Михайлов из Лубян — брак 1888 «42 года, третьим браком», отец
        // до 64 лет с той же женой, смерть 1913 «71 год», своё рождение 1843.
        // Один человек со своим рождением, а не три досье.
        let mut petr = person("child", 10, ("Петр", "", ""), ("Петр", ""), "", "", "М");
        petr.age_years = None;
        save_entry(&conn, &entry(1, 1843, 6, 23, "100", vec![petr, man("father", "Михаил", ("Захаров", "Захарович"), "", "Лубяны"),
                                                          woman("mother", "Варвара", ("Иванова", "Ивановна"), "", "Лубяны")])).unwrap();
        let pm = |role: &str| man(role, "Петр", ("Михайлов", "Михайлович"), "", "Лубяны");
        let favsta = || woman("mother", "Фавста", ("Яковлева", "Яковлевна"), "", "Лубяны");
        birth(1886, vec![kid("Александр", "М"), pm("father"), woman("mother", "Ольга", ("Тихонова", "Тихоновна"), "", "Лубяны")]);
        let mut groom = pm("groom");
        groom.age_years = Some(42);
        groom.marriage_order = Some("Третьим браком".into());
        let mut fb = woman("bride", "Фавста", ("Яковлева", "Яковлевна"), "", "Лубяны");
        fb.age_years = Some(21);
        wedding(1888, groom, fb);
        for y in [1891, 1893, 1896, 1897, 1901, 1902, 1906, 1910] {
            birth(y, vec![kid("Ия", "Ж"), pm("father"), favsta()]);
        }
        let mut dead = pm("deceased");
        dead.age_years = Some(71);
        save_entry(&conn, &entry(3, 1913, 1, 1, "493", vec![dead])).unwrap();
        // Та же жена сильнее ролевых диапазонов: невеста «40 лет» (описка) —
        // и мать через 23 года всё равно одна женщина у того же мужа.
        let lav = |role: &str| man(role, "Лаврентий", ilin, "", "Лубяны");
        let pel = |role: &str| woman(role, "Пелагея", ("Петрова", "Петровна"), "", "Лубяны");
        let mut old_bride = pel("bride");
        old_bride.age_years = Some(40);
        wedding(1880, lav("groom"), old_bride);
        for y in [1882, 1890, 1903] {
            birth(y, vec![kid("Ия", "Ж"), lav("father"), pel("mother")]);
        }

        let index = Index::load(&conn).unwrap();
        let in_lubyany = |text: &str| find_in(&index, &Filter { query: text.into(), place: Some("Лубяны".into()), ..all() }).persons;
        let petrs = in_lubyany("петр михайлович");
        assert_eq!(petrs.iter().map(|h| (h.mentions, h.born)).collect::<Vec<_>>(), vec![(12, Some(1843))],
                   "Петр Михайлов — одно досье со своим рождением");
        let pelageya = in_lubyany("пелагея петровна");
        assert_eq!(pelageya.iter().map(|h| h.mentions).collect::<Vec<_>>(), vec![4], "та же жена — одна женщина, хоть возраст невесты и не сходится");
        let find = |text: &str| find_in(&index, &Filter { query: text.into(), place: Some("Ушаково".into()), ..all() }).persons;
        assert_eq!(find("тимофей ильич").len(), 1, "вдовец — один человек");
        let wives: Vec<usize> = find("ивановна").iter().filter(|h| h.iof.starts_with("Наталья") || h.iof.starts_with("Анна Ивановна")).map(|h| h.mentions).collect();
        assert!(wives.len() >= 2, "Наталья и Анна Ивановны — разные жёны: {wives:?}");
        let efim = find("ефим ильич");
        assert_eq!(efim.iter().map(|h| h.mentions).max(), Some(5), "отец с пятью детьми — одним досье: {:?}", efim.iter().map(|h| h.mentions).collect::<Vec<_>>());
        assert!(efim.iter().filter(|h| h.mentions == 1).all(|h| open_any(&index, h).possible.len() == 1), "отрезанный жених — «возможно»");
        let st = find("степан ильич");
        assert_eq!(st.len(), 2, "одновременные жёны — двое мужчин, даже при «Третьим браком»");
        let maria_side = st.iter().map(|h| open_any(&index, h)).find(|d| d.events.iter().any(|e| e.others.iter().any(|m| m.iof == "Мария Петрова"))).unwrap();
        assert!(maria_side.events.iter().any(|e| e.me.role_code == "deceased_relative"), "смерть Ульяна — в семье с Марией");
        let spouses: Vec<Option<String>> = st.iter().map(|h| h.spouse.clone()).collect();
        assert!(spouses.contains(&Some("Мария Петровна".into())) && spouses.contains(&Some("Анна Осиповна".into())),
                "в строке — с кем человек записан: {spouses:?}");
        // Смерть своего ребёнка — «свои события» (Роман 19.10.2026).
        let own_only = find_in(&index, &Filter { query: "степан ильич".into(), part: false, ..all() }).persons;
        assert!(own_only.iter().any(|h| dossier_in(&index, &h.key, &h.place, &Filter { part: false, ..all() })
            .events.iter().any(|e| e.me.role_code == "deceased_relative")), "без «участия» смерть ребёнка видна");
        let ag = find("агния петровна сизова");
        assert_eq!(ag.iter().map(|h| h.mentions).collect::<Vec<_>>(), vec![1, 1], "двухлетняя умершая и восприемница — разные");
        assert!(ag.iter().all(|h| open_any(&index, h).possible.iter().all(|q| !ag.iter().any(|o| o.key == q.key))),
                "между ними «возможно» нет: однозначно разные");
        let sol = find_in(&index, &Filter { query: "соломон яковлевна".into(), ..all() }).persons;
        assert_eq!(sol.iter().map(|h| h.mentions).collect::<Vec<_>>(), vec![2], "Соломония и Соломонида — одна женщина");
        assert_eq!(find("кузьма власович").iter().map(|h| h.mentions).collect::<Vec<_>>(), vec![2], "варианты имени жены — один человек");
        let zakhar = find("захар ильич");
        assert_eq!(zakhar.len(), 3, "безфамильный подходит двоим — не сливаем");
        assert!(zakhar.iter().any(|h| h.iof == "Захар Ильич" && open_any(&index, h).possible.len() == 2));
        let open = |h: &PersonHit| dossier_in(&index, &h.key, &h.place, &all());

        let ivans = find("иван петрович");
        assert_eq!(ivans.iter().map(|h| h.mentions).collect::<Vec<_>>(), vec![1, 1, 1], "две жены одновременно и восприемник — три досье");
        assert!(ivans.iter().all(|h| open(h).possible.len() == 2), "у каждого «возможно» к двум другим: {:?}", ivans.iter().map(|h| (h.iof.clone(), open(h).possible.len())).collect::<Vec<_>>());
        // Долг п. 72: роли «Возможно» — как на экране. Без «участия»
        // восприемник-тёзка там не показывается, отец с другой женой — да.
        let own_f = Filter { own: true, part: false, ..Filter::default() };
        let father_ivan = ivans.iter().find(|h| open(h).events.iter().any(|e| e.me.role_code == "father")).unwrap();
        assert_eq!(dossier_in(&index, &father_ivan.key, &father_ivan.place, &own_f).possible.len(), 1,
                   "без «участия» в «Возможно» нет восприемника");

        let semyon = find("семен ильич");
        assert_eq!((semyon.len(), semyon[0].mentions), (1, 3), "жена умерла до второго брака — один человек");
        assert_eq!(find("петр ильич").iter().map(|h| h.mentions).collect::<Vec<_>>(), vec![3], "«вторым браком» — один человек");

        let nik = find("николай ильич");
        assert_eq!(nik.iter().map(|h| (h.iof.as_str(), h.mentions)).collect::<Vec<_>>(), vec![("Николай Ильич Кулаев", 2)],
                   "с фамилией и без, жена та же — одно досье");
        let andrei = find("андрей ильич");
        assert_eq!(andrei.len(), 2);
        assert!(andrei.iter().all(|h| open(h).possible.len() == 1), "без жены — «возможно»");

        let maria = find_in(&index, &Filter { query: "марья семеновна".into(), ..all() }).persons;
        assert_eq!(maria.iter().filter(|h| h.born.is_none() || h.mentions > 1).map(|h| (h.iof.as_str(), h.mentions, h.born)).collect::<Vec<_>>(),
                   vec![("Марья Семеновна", 3, Some(1866))], "невеста и мать — одна женщина (в списке — как матерью), рождение — по невесте");
        let by_maiden = find_in(&index, &Filter { query: "сидорова фома".into(), ..all() }).persons;
        assert!(by_maiden.is_empty(), "слова одной персоны, не соседей");
        let found = find_in(&index, &Filter { query: "сидорова марья".into(), ..all() }).persons;
        assert_eq!(found.len(), 1, "по девичьей фамилии");
        assert!(found[0].also.iter().any(|a| a == "Марья Семеновна Сидорова"), "строка показывает и девичью запись: {:?}", found[0].also);
        let her = open(&found[0]);
        assert_eq!(her.iof, "Марья Семеновна (Сидорова)", "девичья фамилия — в скобках (Роман 19.10.2026)");
        assert_eq!(her.events.iter().map(|e| e.me.role_code.as_str()).collect::<Vec<_>>(), vec!["child", "bride", "mother"]);

        let olga = find("ольга петровна");
        let mum = olga.iter().find(|h| h.mentions == 1 && h.born.is_none()).unwrap();
        assert_eq!(open(mum).possible.len(), 1, "долг п. 69: только «возможно»");
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    /// Исправленная опечатка уходит из памяти подсказок (Роман 20.10.2026:
    /// «Павлоа», «Владимир Ремов» оставались после правки записи), а
    /// неизменённое пересохранение частоты не наращивает.
    #[test]
    fn edit_forgets_typos() {
        let Some((path, conn)) = fresh("forget") else { return };
        let people = |sur: &str, priest: &str| vec![
            person("child", 10, ("Ия", "", ""), ("Ия", ""), "", "", "Ж"),
            person("father", 20, ("Павел", "Петров", sur), ("Павел", "Петрович"), "Ушаково", "крестьянин", "М"),
            person("mother", 30, ("Анна", "Ивановна", "Кулаева"), ("Анна", "Ивановна"), "Ушаково", "", "Ж"),
            person("clergy1", 90, ("Владимир", "", priest), ("Владимир", ""), "", "священник", "М"),
        ];
        let saved = save_entry(&conn, &entry(1, 1915, 1, 1, "1", people("Павлоа", "Ремов"))).unwrap();
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(count("SELECT count(*) FROM usage_stat WHERE value = 'Павлоа'"), 3, "опечатка запомнилась (дело, приход, база)");
        let mut fixed = entry(1, 1915, 1, 1, "1", people("Павлов", "Реморов"));
        fixed.id = Some(saved.id);
        save_entry(&conn, &fixed).unwrap();
        assert_eq!(count("SELECT count(*) FROM usage_stat WHERE value = 'Павлоа'"), 0, "опечатки нет в частотах");
        assert_eq!(count("SELECT count(*) FROM lookup WHERE value = 'Павлоа'"), 0, "и в перечне фамилий");
        assert_eq!(count("SELECT count(*) FROM person_index WHERE iof LIKE '%Павлоа%'"), 0, "и в памяти персон");
        assert_eq!(count("SELECT count(*) FROM clergy_index WHERE iof = 'Владимир Ремов'"), 0, "и в списке причта");
        assert_eq!(count("SELECT count(*) FROM clergy_index WHERE iof = 'Владимир Реморов'"), 1);
        assert_eq!(count("SELECT max(count) FROM usage_stat WHERE value = 'Кулаева' AND scope = 'global'"), 1,
                   "неизменённая фамилия после правки — та же частота");
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    /// Подсказка персон: книжные написания одного отчества — один человек
    /// (Роман 20.10.2026, «Ксения Александрова» и «Ксения Александровна»).
    #[test]
    fn person_key_folds_patronymic_spellings() {
        let Some((path, conn)) = fresh("pkey") else { return };
        let key = |s: &str| crate::records::person_key(&conn, s, false).unwrap();
        assert_eq!(key("Ксения Александрова"), key("Ксения Александровна"));
        assert_eq!(key("Иванъ Петров Кулаев"), key("Иван Петрович Кулаев"));
        assert_ne!(key("Ксения Александровна"), key("Ксения Алексеевна"));
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    /// Обрывок со звёздочками не идёт в частоты фамилий, отчеств и имён
    /// (Роман 20.10.2026: «Яготи***» запомнилась новой фамилией).
    #[test]
    fn masked_words_stay_out_of_memory() {
        let Some((path, conn)) = fresh("masked") else { return };
        let p = person("deceased_relative", 20, ("Даниил", "Фил***ов", "Яготи***"), ("Даниил", ""), "Ушаково", "", "М");
        save_entry(&conn, &entry(3, 1915, 1, 1, "1", vec![p])).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM usage_stat WHERE value LIKE '%*%'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "обрывков со звёздочками в частотах нет");
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    /// НП родственникам жениха и невесты — при сохранении (п. 2.4): только
    /// пустой НП и только у названного человека.
    #[test]
    fn relatives_get_place_of_groom_and_bride() {
        let Some((path, conn)) = fresh("relplace") else { return };
        let rel = |role: &str, order: i64, name: &str, place: &str| {
            let mut p = person(role, order, (name, "", ""), (name, ""), place, "", "М");
            p.kinship = Some("отец".into());
            p
        };
        let saved = save_entry(&conn, &entry(2, 1890, 1, 2, "5", vec![
            person("groom", 10, ("Фёдор", "Иванов", ""), ("Фёдор", "Иванович"), "Малово", "", "М"),
            rel("groom_relative", 15, "Иван", ""),
            person("bride", 20, ("Дарья", "Петрова", ""), ("Дарья", "Петровна"), "Воспица", "", "Ж"),
            rel("bride_relative", 25, "Пётр", "Фетинино"),
            rel("bride_parent", 26, "Анна", ""),
            rel("groom_relative", 16, "", ""),
        ])).unwrap();
        let places: Vec<(String, Option<String>)> = conn.prepare(
            "SELECT m.role_code, p.name FROM person_mention m LEFT JOIN place p ON p.id = m.place_id WHERE m.entry_id = ?1 ORDER BY m.sort_order")
            .unwrap().query_map([saved.id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|r| r.unwrap()).collect();
        let want = |v: &str| Some(v.to_string());
        assert_eq!(places, vec![("groom".into(), want("Малово")), ("groom_relative".into(), want("Малово")), ("groom_relative".into(), None),
                                ("bride".into(), want("Воспица")), ("bride_relative".into(), want("Фетинино")), ("bride_parent".into(), want("Воспица"))],
                   "пустой НП — от жениха и невесты, набранный — как есть, безымянный — пусто");
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

    /// Связи «семьи целиком» на настоящей базе — только у разработчика:
    /// GENMETRIC_SEARCH_DB=/путь/база.sqlite \
    ///   cargo test -p genmetric-core --release family_real -- --ignored --nocapture
    #[test]
    #[ignore]
    fn family_real() {
        let Ok(path) = std::env::var("GENMETRIC_SEARCH_DB") else { return };
        let conn = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let started = std::time::Instant::now();
        let plain = load(&conn).unwrap().len();
        println!("только чтение упоминаний ({plain}): {:?}", started.elapsed());
        let started = std::time::Instant::now();
        let index = Index::load(&conn).unwrap();
        println!("память поиска: {} упоминаний, {} персон, {:?}", index.rows.len(), index.persons.len(), started.elapsed());
        let with_parent = index.rows.iter().filter(|r| r.role == "deceased" && r.full.is_some()).count();
        let infant_linked = index.persons.iter().filter(|p| p.birth.is_some() && p.rows.iter().any(|&i| index.rows[i].role == "deceased" && index.rows[i].full.is_some())).count();
        let infant_maybe = index.persons.iter().filter(|p| p.birth.is_none() && p.rows.len() == 1 && index.rows[p.rows[0]].full.is_some() && !p.possible.is_empty()).count();
        let adults_born = index.persons.iter().filter(|p| p.birth.is_some() && p.rows.iter().any(|&i| index.rows[i].role != "child" && index.rows[i].full.is_none())).count();
        let adults_maybe = index.persons.iter().filter(|p| p.birth.is_none() && p.merged_into.is_none() && !p.possible.is_empty()
            && p.rows.iter().all(|&i| index.rows[i].role != "child" && index.rows[i].full.is_none())).count();
        let dead = index.persons.iter().filter(|p| p.merged_into.is_none() && p.died.is_some()).count();
        println!("смертей с родителем: {with_parent}; прикреплено к рождению: {infant_linked}; «возможно»: {infant_maybe}");
        println!("взрослых со своим рождением: {adults_born}; взрослых с «возможно»: {adults_maybe}; умерших персон: {dead}");
        let split = index.persons.iter().filter(|p| p.key.contains('~')).count();
        let absorbed = index.persons.iter().filter(|p| p.merged_into.is_some() && p.birth.is_none()).count();
        let women = index.persons.iter().filter(|p| p.merged_into.is_none() && p.rows.iter().any(|&i| index.rows[i].role == "bride")
            && p.rows.iter().any(|&i| index.rows[i].role == "mother")).count();
        println!("частей после разделения по супругам: {split}; влито (фамилия, жена): {absorbed}; женщин «невеста + мать»: {women}");
        for p in index.persons.iter().filter(|p| p.merged_into.is_none() && p.rows.iter().any(|&i| index.rows[i].role == "bride")
            && p.rows.iter().any(|&i| index.rows[i].role == "mother")).take(4) {
            let line: Vec<String> = p.rows.iter().map(|&i| format!("{}:{}:{}:{:?}", index.rows[i].role, index.rows[i].iof(), index.rows[i].place, index.rows[i].year)).collect();
            println!("  ж {} | {}", p.key, line.join("; "));
        }
        for p in index.persons.iter().filter(|p| p.merged_into.is_none() && !p.possible.is_empty() && p.birth.is_none()
            && p.rows.iter().all(|&i| index.rows[i].full.is_none())).skip(40).take(6) {
            let other: Vec<String> = p.possible.iter().map(|&q| index.persons[q].key.clone()).collect();
            println!("  возм {} | {} | {} упом. | {:?}", p.key, p.place, p.rows.len(), other);
        }
        for p in index.persons.iter().filter(|p| p.key.contains('~')) {
            let line: Vec<String> = p.rows.iter().map(|&i| {
                let r = &index.rows[i];
                let wife = index.rows.iter().find(|w| w.entry_id == r.entry_id && matches!((r.role.as_str(), w.role.as_str()), ("father", "mother") | ("mother", "father") | ("groom", "bride") | ("bride", "groom")));
                format!("{}:{:?}+{}", r.role, r.year, wife.map(|w| w.iof()).unwrap_or_default())
            }).collect();
            println!("  раздел {} | {} | {}", p.key, p.place, line.join("; "));
        }
        for p in index.persons.iter().filter(|p| p.birth.is_some() && p.rows.len() > 2).take(8) {
            let line: Vec<String> = p.rows.iter().map(|&i| format!("{}:{}:{:?}", index.rows[i].role, index.rows[i].iof(), index.rows[i].year)).collect();
            println!("  {} | {} | {}", p.key, p.place, line.join("; "));
        }
        let started = std::time::Instant::now();
        let mut n = 0;
        for row in index.rows.iter().take(2000) {
            if index.died_of(&row.iof(), Some(&row.place)).is_some() { n += 1; }
        }
        println!("died_of ×2000: {:?}, умерших среди них {n}", started.elapsed());
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
