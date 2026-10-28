//! Импорт из Excel-шаблона Familio в новый приход (спека
//! 2026-10-17-import-iz-shablona-familio).
//!
//! Роман 19–20.10.2026: такие файлы присылают волонтёры, набранные для
//! загрузки на сайт Familio; «по одному населенному пункту или приходу
//! присылают много разных файлов, разбитых по годам … — это стандартная и
//! очень частая практика». Поэтому файлов может быть несколько, и все они
//! ложатся в один новый приход.
//!
//! Листы `РОЖДЕНИЕ`, `БРАК`, `СМЕРТЬ`: строка 1 — группы колонок («Отец»,
//! «Восприемник №1»), строка 2 — подписи, с четвёртой — записи. Колонки
//! ищутся **по подписям**, а не по номерам: в «Талызино» нет колонки
//! «Девичья М», и всё правее сдвинуто. Подпись повторяется («ФИО» у каждого
//! восприемника), поэтому ключ колонки — группа, подпись и номер повтора
//! подписи в группе; запасной ключ — подпись и номер её повтора на листе.
//! Номера колонок ниже — номера **нашего шаблона** (`familio_template.xlsx`,
//! ver. 2025-12fd, тот же, что у выгрузки `familio_*` в `statements.sql`);
//! колонка файла находится по ним сопоставлением подписей.
//!
//! Написание — ровно как в файле (Роман 19.10: «Иванович» или «Иванов» — как
//! внёс волонтёр; дореформенные буквы и пометка «(?)» тоже). Современное
//! написание имени и отчества для поиска ставит тот же разбор, что при
//! наборе. Каждая запись сохраняется тем же путём, что набранная
//! (`save_entry_in_tx`), как и при импорте индексатора (`import.rs`).

use std::collections::{BTreeMap, HashMap};

use rusqlite::Connection;
use serde::Serialize;

use crate::export::FAMILIO_TEMPLATE;
use crate::import::{add_places, kin_gender, odd_years, role_title, years_span, ImportReport, Importer, PlaceRow, Row, Sheet};
use crate::records::{looks_like_patronymic, parse_iof_in, save_entry_in_tx, Case, EntryInput, PersonInput};
use crate::statement;
use crate::text::{normalize, normalize_name};
use crate::xlsx::read_sheet;

/// Листы записей: имя листа и раздел словами (для сообщений).
const SHEETS: [(&str, &str); 3] = [("РОЖДЕНИЕ", "рождения"), ("БРАК", "браки"), ("СМЕРТЬ", "смерти")];

/// Первая строка записей шаблона: строка 3 — пустая под подписями.
const FIRST_ROW: u32 = 4;

/// Без этих колонок лист не разобрать (колонки шаблона): год и имя
/// главного лица. Остальных может не быть — тогда их поля пусты, а отчёт
/// называет, каких колонок не нашлось.
const MAIN: [&[u32]; 3] = [&[15, 19], &[14, 20, 38], &[15, 24]];

/// Что нашлось в одном файле — до импорта.
#[derive(Serialize, Debug, Default, Clone)]
pub struct FileSeen {
    pub file_name: String,
    pub births: usize,
    pub marriages: usize,
    pub deaths: usize,
    pub years: Vec<i64>,
    pub odd_years: Vec<i64>,
    /// Самый частый «Н.П. события» и церковь файла.
    pub village: String,
    pub church: String,
    /// «Кем проведена индексация» с листа `about`.
    pub indexer: String,
    /// Строки шаблона без данных (номер, формулы «#N/A») — не записи.
    pub empty_rows: usize,
    /// Колонки шаблона, которых в файле нет: «БРАК: Девичья М».
    pub missing: Vec<String>,
    /// Колонки файла, которых нет в шаблоне, — не переносятся.
    pub extra: Vec<String>,
}

// ----------------------------------------------------------------------------
//  Колонки по подписям
// ----------------------------------------------------------------------------

fn label(s: &str) -> String {
    normalize(s)
}

/// Подписи листа: (колонка, группа, подпись). Группа — последняя непустая
/// ячейка первой строки слева (объединённая ячейка хранит текст в первой).
fn heads(sheet: &Sheet) -> Vec<(u32, String, String)> {
    let empty = Row::new();
    let (r1, r2) = (sheet.get(&1).unwrap_or(&empty), sheet.get(&2).unwrap_or(&empty));
    let last = r1.keys().chain(r2.keys()).copied().max().unwrap_or(0);
    let mut group = String::new();
    let mut out = Vec::new();
    for c in 1..=last {
        let g = r1.get(&c).map(|v| label(v)).unwrap_or_default();
        if !g.is_empty() {
            group = g;
        }
        out.push((c, group.clone(), r2.get(&c).map(|v| label(v)).unwrap_or_default()));
    }
    out
}

/// Ключи колонок: (группа, подпись, повтор в группе) и (подпись, повтор на листе).
fn keys(heads: &[(u32, String, String)]) -> Vec<(u32, (String, String, usize), (String, usize))> {
    let mut in_group: HashMap<(String, String), usize> = HashMap::new();
    let mut on_sheet: HashMap<String, usize> = HashMap::new();
    heads.iter().map(|(c, g, l)| {
        let k1 = in_group.entry((g.clone(), l.clone())).or_default();
        let k2 = on_sheet.entry(l.clone()).or_default();
        let out = (*c, (g.clone(), l.clone(), *k1), (l.clone(), *k2));
        *k1 += 1;
        *k2 += 1;
        out
    }).collect()
}

/// Сопоставление колонок шаблона колонкам файла.
struct Cols {
    to_file: HashMap<u32, u32>,
    /// Колонка шаблона → её подпись (для сообщений).
    titles: HashMap<u32, String>,
    missing: Vec<u32>,
    extra: Vec<String>,
}

/// Колонки-формулы и служебные: их отсутствие ничего не меняет.
fn is_formula(label: &str) -> bool {
    matches!(label, "full_location" | "person_location" | "url")
}

fn map_columns(template: &Sheet, file: &Sheet) -> Cols {
    let t_heads = heads(template);
    let f_heads = heads(file);
    let f_keys = keys(&f_heads);
    let by1: HashMap<&(String, String, usize), u32> = f_keys.iter().map(|(c, k1, _)| (k1, *c)).collect();
    let by_label: HashMap<&(String, usize), u32> = f_keys.iter().map(|(c, _, k2)| (k2, *c)).collect();
    let mut cols = Cols { to_file: HashMap::new(), titles: HashMap::new(), missing: Vec::new(), extra: Vec::new() };
    let mut used: std::collections::HashSet<u32> = std::collections::HashSet::new();
    // Два прохода: сначала точные ключи (группа, подпись, повтор), потом
    // запасные — иначе запасной ключ ранней колонки забирал бы чужую колонку
    // раньше точного совпадения (ревьюер 21.10.2026: без блока «исповедовал»
    // его подписи забирали колонки погребавшего).
    let t_keys = keys(&t_heads);
    for (t, k1, _) in &t_keys {
        if let Some(c) = by1.get(k1).copied() {
            used.insert(c);
            cols.to_file.insert(*t, c);
        }
    }
    for (t, k1, k2) in t_keys {
        let title = t_heads.iter().find(|h| h.0 == t).map(|h| if h.2.is_empty() { h.1.clone() } else { h.2.clone() })
            .unwrap_or_default();
        cols.titles.insert(t, title);
        if cols.to_file.contains_key(&t) {
            continue;
        }
        let found = by_label.get(&k2).copied().filter(|c| !used.contains(c) && !k2.0.is_empty());
        match found {
            Some(c) => {
                used.insert(c);
                cols.to_file.insert(t, c);
            }
            None if !is_formula(&k1.1) => cols.missing.push(t),
            None => {}
        }
    }
    for (c, _, l) in &f_heads {
        if !used.contains(c) && !l.is_empty() && !is_formula(l) {
            cols.extra.push(l.clone());
        }
    }
    cols
}

/// Значение ячейки файла по колонке шаблона. Число — без «#» и без «.0»;
/// ошибка формулы («#N/A») — пусто; «-» — пусто.
fn raw(row: &Row, cols: &Cols, t: u32) -> Option<String> {
    let c = cols.to_file.get(&t)?;
    let v = row.get(c)?.trim();
    let v = match v.strip_prefix('#') {
        Some(n) => match n.parse::<f64>() {
            Ok(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
            Ok(_) => n.replace('.', ","),
            Err(_) => return None,
        },
        None => v.split_whitespace().collect::<Vec<_>>().join(" "),
    };
    Some(v).filter(|v| !v.is_empty() && v != "-")
}

fn num(row: &Row, cols: &Cols, t: u32) -> Option<i64> {
    raw(row, cols, t).and_then(|v| v.parse::<i64>().ok())
}

/// Колонки, которые не делают строку записью: номер, реквизиты дела,
/// «Событие» и формулы (их значение в пустой строке шаблона — «#N/A»).
fn is_frame(t: u32, title: &str) -> bool {
    t == 1 || (2..=10).contains(&t) || is_formula(title)
}

fn is_record(row: &Row, cols: &Cols) -> bool {
    cols.to_file.keys().any(|t| !is_frame(*t, cols.titles.get(t).map(String::as_str).unwrap_or("")) && raw(row, cols, *t).is_some())
}

/// Дело строки: архив, фонд, опись и дело (Роман 26.10.2026: «одни и те же
/// архивные реквизиты… = одно цельное дело»); строка без номера дела — дело
/// своего года; без того и другого — `None` (первое дело).
fn case_key(row: &Row, cols: &Cols, year: Option<i64>) -> Option<String> {
    let parts: Vec<String> = [2u32, 3, 4, 5].iter().map(|t| raw(row, cols, *t).map(|v| normalize(&v)).unwrap_or_default()).collect();
    // Без номера дела — дело своего года: один архив дела не задаёт.
    if !parts[3].is_empty() {
        Some(format!("дело|{}", parts.join("|")))
    } else {
        year.map(|y| format!("год|{y:06}"))
    }
}

/// Год книги: у рождений и смертей — год обряда (крещения, погребения), как
/// в форме; нет его — год события. У браков — год венчания.
fn book_year(section: usize, row: &Row, cols: &Cols) -> Option<i64> {
    let ok = |y: Option<i64>| y.filter(|y| (1500..=2100).contains(y));
    match section {
        1 => ok(num(row, cols, 14)),
        _ => ok(num(row, cols, 18)).or_else(|| ok(num(row, cols, 15))),
    }
}

// ----------------------------------------------------------------------------
//  Файл
// ----------------------------------------------------------------------------

struct Book {
    /// Лист и его колонки; листа нет — `None`.
    sheets: [Option<(Sheet, Cols)>; 3],
    locations: Vec<(u32, PlaceRow)>,
    indexer: String,
}

/// «Воронежская губ.» → «Воронежская»: в справочнике пунктов слово не
/// пишется, выгрузка добавляет его сама.
fn strip_word(v: Option<String>, words: &[&str]) -> Option<String> {
    let v = v?;
    let t = v.trim();
    for w in words {
        if let Some(rest) = t.strip_suffix(w) {
            let rest = rest.trim_end();
            if !rest.is_empty() && rest.len() < t.len() {
                return Some(rest.to_string());
            }
        }
    }
    Some(t.to_string()).filter(|s| !s.is_empty())
}

fn load(bytes: &[u8]) -> Result<Book, String> {
    let mut sheets: [Option<(Sheet, Cols)>; 3] = [None, None, None];
    for (i, (name, _)) in SHEETS.iter().enumerate() {
        let Ok(sheet) = read_sheet(bytes, name) else { continue };
        // Лист без подписей и без строк записей — как листа нет (ревьюер 21.10.2026).
        if !sheet.contains_key(&2) && sheet.range(FIRST_ROW..).next().is_none() {
            continue;
        }
        let template = read_sheet(FAMILIO_TEMPLATE, name).map_err(|e| format!("шаблон Familio: {e}"))?;
        let cols = map_columns(&template, &sheet);
        let lost: Vec<String> = MAIN[i].iter().filter(|t| !cols.to_file.contains_key(t))
            .map(|t| format!("«{}»", cols.titles.get(t).cloned().unwrap_or_default())).collect();
        if !lost.is_empty() {
            return Err(format!(
                "лист «{name}» не похож на лист шаблона Familio: во второй строке нет подписей {} — без них записи не разобрать",
                lost.join(", ")));
        }
        sheets[i] = Some((sheet, cols));
    }
    if sheets.iter().all(Option::is_none) {
        return Err("это не шаблон Familio: в файле нет листов «РОЖДЕНИЕ», «БРАК», «СМЕРТЬ»".into());
    }
    let mut locations = Vec::new();
    if let Ok(loc) = read_sheet(bytes, "location") {
        for (r, row) in &loc {
            if *r < 3 {
                continue;
            }
            let get = |c: u32| -> Option<String> {
                let v = row.get(&c)?.trim().to_string();
                Some(v).filter(|v| !v.is_empty() && !v.starts_with('#') && v != "0")
            };
            // «Соротово(?)» — тот же пункт, что у персоны: без пометки.
            let Some(name) = get(1).map(|n| clean(&n)).filter(|n| !n.is_empty()) else { continue };
            locations.push((*r, PlaceRow {
                name,
                np_type: get(2),
                guberniya: strip_word(get(3), &["губерния", "губернии", "губ.", "губ"]),
                uyezd: strip_word(get(4), &["уезд", "уезда", "у."]),
                volost: strip_word(get(5), &["волость", "волости", "вол."]),
                short: get(6),
                full: get(7),
                url: get(8).filter(|u| u.starts_with("http")),
            }));
        }
    }
    // «Кем проведена индексация (авторство)» — строка листа about.
    let mut indexer = String::new();
    if let Ok(about) = read_sheet(bytes, "about") {
        for row in about.values() {
            let head = row.get(&1).map(|v| normalize(v)).unwrap_or_default();
            if head.starts_with("кем проведена индексация") {
                indexer = row.get(&2).map(|v| v.trim().to_string()).unwrap_or_default();
            }
        }
    }
    Ok(Book { sheets, locations, indexer })
}

/// Самое частое значение (при равенстве — встретившееся первым).
fn most_common(values: &[String]) -> String {
    let mut count: HashMap<&str, (usize, usize)> = HashMap::new();
    for (i, v) in values.iter().enumerate() {
        let e = count.entry(v.as_str()).or_insert((0, i));
        e.0 += 1;
    }
    count.into_iter().max_by(|a, b| a.1 .0.cmp(&b.1 .0).then(b.1 .1.cmp(&a.1 .1)))
        .map(|(v, _)| v.to_string()).unwrap_or_default()
}

/// Что в файле — до импорта.
pub fn inspect(file_name: &str, bytes: &[u8]) -> Result<FileSeen, String> {
    let book = load(bytes)?;
    let mut out = FileSeen { file_name: file_name.to_string(), indexer: book.indexer.clone(), ..Default::default() };
    let mut years: BTreeMap<i64, usize> = BTreeMap::new();
    let (mut villages, mut churches) = (Vec::new(), Vec::new());
    for (i, slot) in book.sheets.iter().enumerate() {
        let Some((sheet, cols)) = slot else { continue };
        let mut n = 0;
        for (_, row) in sheet.range(FIRST_ROW..) {
            if !is_record(row, cols) {
                if cols.to_file.keys().any(|t| raw(row, cols, *t).is_some()) {
                    out.empty_rows += 1;
                }
                continue;
            }
            n += 1;
            if let Some(y) = book_year(i, row, cols) {
                *years.entry(y).or_default() += 1;
            }
            villages.extend(raw(row, cols, 7));
            churches.extend(raw(row, cols, 9));
        }
        match i {
            0 => out.births = n,
            1 => out.marriages = n,
            _ => out.deaths = n,
        }
        let name = SHEETS[i].0;
        for t in &cols.missing {
            out.missing.push(format!("{name}: {}", cols.titles.get(t).cloned().unwrap_or_default()));
        }
        for l in &cols.extra {
            out.extra.push(format!("{name}: {l}"));
        }
    }
    out.village = most_common(&villages);
    out.church = most_common(&churches);
    out.odd_years = odd_years(&years);
    out.years = years.into_keys().collect();
    if out.births + out.marriages + out.deaths == 0 {
        return Err("в файле нет ни одной записи".into());
    }
    Ok(out)
}

// ----------------------------------------------------------------------------
//  Люди
// ----------------------------------------------------------------------------

/// Пометка сомнения волонтёра: «Кудряков(?)». В записи остаётся как в файле,
/// для сверки со словарём — без неё.
const DOUBT: &str = "(?)";

fn clean(word: &str) -> String {
    word.replace(DOUBT, "").trim().to_string()
}

#[derive(Default, Clone)]
struct Names {
    surname: Option<String>,
    first: Option<String>,
    patr: Option<String>,
}

impl Names {
    fn is_empty(&self) -> bool {
        self.surname.is_none() && self.first.is_none() && self.patr.is_none()
    }
    fn shown(&self) -> String {
        [&self.surname, &self.first, &self.patr].iter().filter_map(|v| v.as_deref()).collect::<Vec<_>>().join(" ")
    }
}

struct Familio<'a> {
    imp: Importer<'a>,
    /// Звание и место человека без имени в текущей строке — в примечание
    /// записи (в форме персона без ИОФ не сохраняется), а не молча.
    orphans: Vec<String>,
    /// Слово → это имя из словаря (или заведённое человеком)?
    is_name: HashMap<String, bool>,
}

impl Familio<'_> {
    fn known_name(&mut self, word: &str) -> Result<bool, String> {
        let w = normalize_name(&clean(word));
        if w.is_empty() || !word.chars().next().is_some_and(char::is_uppercase) {
            return Ok(false);
        }
        if let Some(v) = self.is_name.get(&w) {
            return Ok(*v);
        }
        let conn = self.imp.conn;
        let known: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM name_form WHERE kind IN ('name', 'variant') AND form_norm = ?1)
                 OR EXISTS (SELECT 1 FROM name_alias WHERE kind = 'name' AND form_norm = ?1)",
            [&w], |r| r.get(0)).map_err(|e| e.to_string())?;
        self.is_name.insert(w, known);
        Ok(known)
    }

    /// «ФИО» одной колонкой (восприемники, поручители, причт). Порядок слов в
    /// файлах разный (Роман 19.10: «Фамилия Имя Отчество» у Нового Акшина,
    /// «Имя Отчество» у Фаустова, вперемешку у Шуи) — по словарю: слово из
    /// имён — имя, следующее за ним, похожее на отчество, — отчество,
    /// остальное — фамилия. Имени из словаря нет — порядок «Имя Отчество
    /// Фамилия», как при наборе.
    fn split_fio(&mut self, text: &str, clergy: bool) -> Result<Names, String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        // Первое слово из словаря имён; но если за каким-то именем стоит
        // отчество — оно («Ильина Анастасия Ивановна»: «Ильина» тоже есть
        // среди имён).
        let mut at = None;
        for (i, w) in words.iter().enumerate() {
            if self.known_name(w)? {
                if at.is_none() {
                    at = Some(i);
                }
                if words.get(i + 1).is_some_and(|n| strong_patronymic(n)) {
                    at = Some(i);
                    break;
                }
            }
        }
        if words.len() == 1 && strong_patronymic(words[0]) {
            return Ok(Names { patr: Some(words[0].to_string()), ..Default::default() });
        }
        // Имени из словаря нет, но слов три и больше и последнее — отчество:
        // порядок шаблона Familio «Фамилия Имя Отчество», имя — перед
        // отчеством («Усанов Евфросинья Ивановна» — имя в редком написании).
        // Ровно три слова и последнее похоже на отчество («Иванова Мароа
        // Михайлова», «Воробьева Мароа(?) Иосиина» — долг п. 79) — тоже
        // порядок шаблона: первое слово — фамилия.
        let last = words.len().wrapping_sub(1);
        // Но «Ефим Артемов Сидоров» (имя вне словаря, старое отчество в середине)
        // — порядок «Имя Отчество Фамилия»: середина — отчество, значит
        // фамилии впереди нет (долг п. 82, ревьюер 21.10.2026). Середина —
        // отчество по словарю отчеств, а не по окончанию: «Петрова Улияна
        // Иванова» — «Улияна» оканчивается на «-на», но это имя (п. 96).
        let middle_patr = |s: &mut Self| -> Result<bool, String> {
            Ok(looks_like_patronymic(&clean(words[1])) && s.is_patronymic(words[1])?)
        };
        if at.is_none() && words.len() >= 3
            && (strong_patronymic(words[last])
                || (words.len() == 3 && looks_like_patronymic(&clean(words[last])) && !middle_patr(self)?))
            && !strong_patronymic(words[last - 1]) {
            at = Some(last - 1);
        }
        let (before, from) = match at {
            Some(i) => (&words[..i], &words[i..]),
            None => (&words[..0], &words[..]),
        };
        let mut n = Names::default();
        let mut rest = from;
        if let Some(first) = rest.first() {
            n.first = Some(first.to_string());
            rest = &rest[1..];
        }
        // Отчество — второе слово после имени. «-вич», «-вна» — отчество
        // всегда; «Артемов» — если за ним есть фамилия, фамилия стоит
        // впереди или словарь знает такое отчество. У причта два слова — имя и
        // фамилия («Александр Аверьянов»), как в форме причта.
        if let Some(second) = rest.first() {
            let strong = strong_patronymic(second);
            let patr_like = looks_like_patronymic(&clean(second));
            let two_words = before.is_empty() && rest.len() == 1;
            if strong || (patr_like && !(clergy && two_words)
                          && (!before.is_empty() || rest.len() >= 2 || self.is_patronymic(second)?)) {
                n.patr = Some(second.to_string());
                rest = &rest[1..];
            }
        }
        let mut surname: Vec<&str> = before.to_vec();
        surname.extend_from_slice(rest);
        n.surname = Some(surname.join(" ")).filter(|s| !s.is_empty());
        Ok(n)
    }

    fn is_patronymic(&mut self, word: &str) -> Result<bool, String> {
        let w = normalize_name(&clean(word));
        self.imp.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM name_form WHERE kind LIKE 'patr%' AND form_norm = ?1)",
            [&w], |r| r.get(0)).map_err(|e| e.to_string())
    }

    /// Персона из частей файла. Ничего, кроме звания и места, — `None`.
    #[allow(clippy::too_many_arguments)]
    fn person(&mut self, role: &'static str, order: i64, n: Names, gender: Option<String>,
              rank: Option<String>, place: Option<String>, note: Option<String>) -> Result<Option<PersonInput>, String> {
        if n.is_empty() {
            let parts: Vec<String> = [rank, place].into_iter().flatten().collect();
            if !parts.is_empty() {
                self.orphans.push(format!("{} без имени: {}", role_title(role), parts.join(", ")));
            }
            return Ok(None);
        }
        // «Соротово(?)» — пункт под сомнением: в справочник — без пометки,
        // сомнение — у персоны («неуверенно»), как у имени.
        let place_doubt = place.as_deref().is_some_and(|p| p.contains(DOUBT));
        let place = place.map(|p| clean(&p)).filter(|p| !p.is_empty());
        let mut first_modern = None;
        let mut patr_modern = None;
        let mut by_name = None;
        if let Some(first) = &n.first {
            let text = [Some(clean(first)), n.patr.as_deref().map(clean)].into_iter().flatten()
                .filter(|v| !v.is_empty()).collect::<Vec<_>>().join(" ");
            let mut parsed = parse_iof_in(self.imp.conn, &text)?;
            // «Игнатьевич», «Евфросинья» — в словаре «Игнатиевич»,
            // «Евфросиния»: в книге остаётся как в файле, современное — из
            // словаря (проверяющий 21.10.2026: у Багаева 699 отчеств на сверке).
            if !parsed.known_name || (parsed.patronymic.is_none() && n.patr.is_some()) {
                let alt = text.split_whitespace().map(soft_sign).collect::<Vec<_>>().join(" ");
                if alt != text {
                    let again = parse_iof_in(self.imp.conn, &alt)?;
                    if again.known_name && (again.patronymic.is_some() || n.patr.is_none()) {
                        parsed = again;
                    }
                }
            }
            let shown = n.shown();
            if !parsed.known_name {
                self.imp.unknown_total += 1;
                let name = parsed.first_name.clone().unwrap_or_default();
                self.imp.pending.push(("name", format!("{}: имени «{name}» нет в словаре — «{shown}»", role_title(role))));
                *self.imp.unknown.entry(name).or_default() += 1;
            } else {
                first_modern = parsed.first_name_modern.clone();
            }
            if parsed.patronymic.is_some() {
                patr_modern = parsed.patronymic_modern.clone();
            } else if let Some(p) = n.patr.as_ref().filter(|_| !role.starts_with("clergy")) {
                // Колонка «Отчество» — значит, отчество, даже если словарь его
                // не знает: остаётся отчеством, как в файле, и идёт на сверку.
                self.imp.unknown_patr_total += 1;
                self.imp.pending.push(("patronymic", format!(
                    "{}: отчества «{p}» нет в словаре — перенесено как в файле: «{shown}»", role_title(role))));
                *self.imp.unknown_patr.entry(clean(p)).or_default() += 1;
            }
            by_name = parsed.gender;
        }
        let mut doubt: Vec<&str> = [("surname", &n.surname), ("first_name", &n.first), ("patronymic", &n.patr)]
            .iter().filter(|(_, v)| v.as_deref().is_some_and(|v| v.contains(DOUBT))).map(|(k, _)| *k).collect();
        if place_doubt {
            doubt.push("place");
        }
        Ok(Some(PersonInput {
            role_code: role.to_string(),
            sort_order: order,
            surname: n.surname,
            first_name: n.first,
            patronymic: n.patr,
            surname_modern: None,
            first_name_modern: first_modern,
            patronymic_modern: patr_modern,
            maiden_surname: None,
            gender: gender.or(by_name),
            rank,
            confession: None,
            place,
            note,
            uncertain: if doubt.is_empty() { None } else { serde_json::to_string(&doubt).ok() },
            age_years: None,
            marriage_order: None,
            kinship: None,
            age_months: None,
            age_weeks: None,
            age_days: None,
            age_text: None,
            death_cause: None,
        }))
    }
}

/// Отчество без сомнений — по окончанию: «Лаврентьевна», «Дормидонтович»,
/// «Козьмич». Фамилии так не оканчиваются (в отличие от «-ов»: «Артемов» —
/// и отчество, и фамилия).
fn strong_patronymic(word: &str) -> bool {
    let w = normalize_name(&clean(word));
    w.chars().count() > 4 && ["вич", "вна", "ична", "ич", "ишна"].iter().any(|e| w.ends_with(e))
}

/// Написание через «ь» → через «и», как в словаре: «Игнатьевич» →
/// «Игнатиевич», «Евфросинья» → «Евфросиния».
fn soft_sign(word: &str) -> String {
    let w = word.replace("ьев", "иев").replace("ьич", "ич");
    match w.strip_suffix("ья") {
        Some(stem) if stem.chars().count() > 2 => format!("{stem}ия"),
        _ => w,
    }
}

/// Фамилия и «Фамилия-2»: «Глухов (Логунков)» (Роман 19.10.2026) — так вторая
/// фамилия видна в поиске и в досье; выгрузка кладёт её обратно в «Фамилию-2».
fn surname(main: Option<String>, second: Option<String>) -> Option<String> {
    match (main, second) {
        (Some(a), Some(b)) => Some(format!("{a} ({b})")),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(format!("({b})")),
        (None, None) => None,
    }
}

fn sex(v: Option<String>) -> Option<String> {
    let v = normalize(&v?);
    if v.starts_with('м') { Some("М".into()) } else if v.starts_with('ж') { Some("Ж".into()) } else { None }
}

/// «№ брака» числом → словами формы: «Первым браком».
fn marriage_order(v: Option<String>) -> Option<String> {
    let v = v?;
    Some(match v.as_str() {
        "1" => "Первым браком".into(),
        "2" => "Вторым браком".into(),
        "3" => "Третьим браком".into(),
        "4" => "Четвертым браком".into(),
        "5" => "Пятым браком".into(),
        _ => v,
    })
}

/// Возраст по колонкам «Лет / Мес / Нед / Дней» — как его набрали бы в
/// поле: «53», «3 мес», «1 г 3 мес». Не число — текстом как есть.
fn age_text(parts: [Option<String>; 4]) -> Option<String> {
    let units = ["г", "мес", "нед", "дн"];
    let filled: Vec<(usize, &String)> = parts.iter().enumerate().filter_map(|(i, v)| v.as_ref().map(|v| (i, v))).collect();
    match filled.as_slice() {
        [] => None,
        [(0, y)] => Some((*y).clone()),
        // «3,5» недели — с единицей: без неё прочиталось бы годами (проверяющий 21.10.2026).
        many => Some(many.iter().map(|(i, v)| if v.replace(',', ".").parse::<f64>().is_ok() { format!("{v} {}", units[*i]) } else { (*v).clone() })
            .collect::<Vec<_>>().join(" ")),
    }
}

// ----------------------------------------------------------------------------
//  Перенос
// ----------------------------------------------------------------------------

/// Одна запись — для поиска одинаковых записей в разных файлах: раздел, год,
/// дата и главные лица с точностью до написания.
fn twin_key(section: i64, year: Option<i64>, day: Option<i64>, month: Option<i64>, persons: &[PersonInput]) -> Option<String> {
    let main: &[&str] = match section {
        1 => &["child", "father", "mother"],
        2 => &["groom", "bride"],
        _ => &["deceased"],
    };
    let mut parts = Vec::new();
    for role in main {
        if let Some(p) = persons.iter().find(|p| p.role_code == *role) {
            let words = [&p.first_name, &p.surname].iter().filter_map(|v| v.as_deref()).map(|v| normalize_name(&clean(v)))
                .collect::<Vec<_>>().join(" ");
            parts.push(words);
        }
    }
    if parts.iter().all(|p| p.is_empty()) {
        return None;
    }
    Some(format!("{section}|{year:?}|{day:?}|{month:?}|{}", parts.join("|")))
}

/// Файлы — в открытую базу нового прихода. Вызывается внутри транзакции
/// `parish::create_with`: ошибка — ничего не остаётся.
pub fn import_into(conn: &Connection, files: &[(String, Vec<u8>)]) -> Result<ImportReport, String> {
    let mut books = Vec::new();
    for (name, bytes) in files {
        books.push((name.clone(), load(bytes).map_err(|e| format!("файл «{name}»: {e}"))?));
    }
    let mut report = ImportReport::default();
    let count = |sql: &str| -> Result<i64, String> { conn.query_row(sql, [], |r| r.get(0)).map_err(|e| e.to_string()) };
    let places_before = count("SELECT count(*) FROM place")?;
    let several = books.len() > 1;
    // «лист РОЖДЕНИЕ, строка 7» — с именем файла, когда файлов несколько.
    let at = |file: &str, sheet: &str, r: u32| if several {
        format!("файл «{file}», лист «{sheet}», строка {r}")
    } else {
        format!("лист «{sheet}», строка {r}")
    };

    // Пункты всех файлов: губерния и уезд дела берутся у пункта события.
    let mut places: HashMap<String, &PlaceRow> = HashMap::new();
    for (_, book) in &books {
        for (_, p) in &book.locations {
            places.entry(normalize(&p.name)).or_insert(p);
        }
    }

    // Дела — по реквизитам строки (Роман 26.10.2026: «одни и те же архивные
    // реквизиты, указанные волонтером со скана = одно цельное дело в
    // программе, даже если внутри него встречаются события за разные годы»);
    // год дела — год большинства его записей. Строка без реквизитов — в дело
    // своего года, как раньше.
    let mut cases: BTreeMap<String, Case> = BTreeMap::new();
    let mut case_years: BTreeMap<String, BTreeMap<i64, usize>> = BTreeMap::new();
    let mut year_counts: BTreeMap<i64, usize> = BTreeMap::new();
    // Индексатор — у файла, давшего записи года; файлов несколько — через
    // запятую (ревьюер 21.10.2026: по приходу присылают файлы разных волонтёров).
    let mut indexers: BTreeMap<i64, Vec<String>> = BTreeMap::new();
    for (_, book) in &books {
        for (i, slot) in book.sheets.iter().enumerate() {
            let Some((sheet, cols)) = slot else { continue };
            for (_, row) in sheet.range(FIRST_ROW..) {
                if !is_record(row, cols) {
                    continue;
                }
                let year = book_year(i, row, cols);
                let Some(key) = case_key(row, cols, year) else { continue };
                if let Some(year) = year {
                    *year_counts.entry(year).or_default() += 1;
                    *case_years.entry(key.clone()).or_default().entry(year).or_default() += 1;
                    let who = indexers.entry(year).or_default();
                    if !book.indexer.trim().is_empty() && !who.iter().any(|w| w == book.indexer.trim()) {
                        who.push(book.indexer.trim().to_string());
                    }
                }
                let case = cases.entry(key).or_default();
                let fill = |slot: &mut Option<String>, v: Option<String>| {
                    if slot.is_none() {
                        *slot = v;
                    }
                };
                fill(&mut case.archive, raw(row, cols, 2));
                fill(&mut case.fond, raw(row, cols, 3));
                fill(&mut case.opis, raw(row, cols, 4));
                fill(&mut case.delo, raw(row, cols, 5));
                fill(&mut case.church, raw(row, cols, 9));
                let village = raw(row, cols, 7);
                if case.village.is_none() {
                    if let Some(p) = village.as_ref().and_then(|v| places.get(&normalize(v))) {
                        case.uyezd = p.uyezd.clone();
                        case.guberniya = p.guberniya.clone();
                    }
                }
                fill(&mut case.village, village);
            }
        }
    }
    // Год без записей с годом — одно дело без года не заводится: такие
    // записи ложатся в первое дело (как у импорта индексатора).
    if cases.is_empty() {
        cases.insert(String::new(), Case::default());
    }
    for (key, case) in cases.iter_mut() {
        case.year = case_years.get(key).and_then(|ys| ys.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))).map(|(y, _)| *y));
    }
    // Номера дел — по году, затем по реквизитам.
    let mut order: Vec<String> = cases.keys().cloned().collect();
    order.sort_by_key(|k| (cases[k].year.unwrap_or(i64::MAX), k.clone()));
    let parish_of = cases.values().find(|c| c.village.is_some() || c.church.is_some()).cloned().unwrap_or_default();
    let upsert = statement("case_upsert")?;
    let mut parish_key = String::new();
    let mut first_case = 0;
    for (n, key) in order.iter().enumerate() {
        let case = cases.get_mut(key).expect("ключ из того же перечня");
        if case.village.is_none() && case.church.is_none() {
            case.church = parish_of.church.clone();
            case.village = parish_of.village.clone();
            case.uyezd = parish_of.uyezd.clone();
            case.guberniya = parish_of.guberniya.clone();
        }
        case.indexer = case.year.and_then(|y| indexers.get(&y)).map(|w| w.join(", ")).filter(|w| !w.is_empty())
            .or_else(|| books.iter().map(|(_, b)| b.indexer.trim()).find(|v| !v.is_empty()).map(str::to_string));
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
    report.years = year_counts.keys().copied().collect();
    let odd = odd_years(&year_counts);
    let usual = years_span(&year_counts, &odd);

    let review_add = statement("review_add")?;
    let review = |kind: &str, text: &str, sheet: &str, row: u32, entry: Option<i64>| -> Result<(), String> {
        conn.execute(&review_add, rusqlite::named_params! {
            ":kind": kind, ":text": text, ":sheet": sheet, ":row": row, ":entry_id": entry,
        }).map(|_| ()).map_err(|e| e.to_string())
    };
    let mut fam = Familio {
        imp: Importer {
            conn, unknown: BTreeMap::new(), unknown_total: 0, unknown_patr: BTreeMap::new(), unknown_patr_total: 0,
            pending: Vec::new(),
        },
        is_name: HashMap::new(),
        orphans: Vec::new(),
    };
    // Одинаковые записи в разных файлах: обе сохраняются и обе — на сверку.
    let mut twins: HashMap<String, (usize, String, String, u32, i64)> = HashMap::new();
    let mut twins_flagged: std::collections::HashSet<i64> = std::collections::HashSet::new();
    let mut kin_of_godparents = 0;

    for (fi, (file, book)) in books.iter().enumerate() {
        for (i, slot) in book.sheets.iter().enumerate() {
            let Some((sheet, cols)) = slot else { continue };
            let section = i as i64 + 1;
            let name = SHEETS[i].0;
            // Лист в списке на сверку: «РОЖДЕНИЕ | файл.xlsx» (ReviewDialog).
            let review_sheet = format!("{name} | {file}");
            for (r, row) in sheet.range(FIRST_ROW..) {
                let r = *r;
                if !is_record(row, cols) {
                    if cols.to_file.keys().any(|t| raw(row, cols, *t).is_some()) {
                        report.empty_rows += 1;
                    }
                    continue;
                }
                let g = |t: u32| raw(row, cols, t);
                let n = |t: u32| num(row, cols, t);
                let notes_from = report.notes.len();
                fam.imp.pending.clear();
                fam.orphans.clear();
                let here = at(file, name, r);
                if let Some(v) = raw(row, cols, 11).filter(|v| v.parse::<i64>().is_err()) {
                    report.notes.push(format!("{here}: № записи «{v}» не число — в записи номера нет"));
                }
                // Дата: числа дня и месяца вне календаря — не молча.
                let date = |d: u32, m: u32, y: u32, report: &mut ImportReport| -> (Option<i64>, Option<i64>, Option<i64>) {
                    let (day, month, year) = (n(d), n(m), n(y));
                    let bad_day = g(d).is_some() && !day.is_some_and(|v| (1..=31).contains(&v));
                    let bad_month = g(m).is_some() && !month.is_some_and(|v| (1..=12).contains(&v));
                    if bad_day || bad_month {
                        report.notes.push(format!("{here}: дата «{}.{}.{}» — день или месяц вне календаря, в записи их нет",
                                                  g(d).unwrap_or_default(), g(m).unwrap_or_default(), g(y).unwrap_or_default()));
                    }
                    (day.filter(|v| (1..=31).contains(v)), month.filter(|v| (1..=12).contains(v)),
                     year.filter(|v| (1000..=2100).contains(v)))
                };
                let mut persons: Vec<PersonInput> = Vec::new();
                // Примечание записи (у браков и смертей — главного лица:
                // примечания записи в этих формах нет).
                let mut notes: Vec<String> = Vec::new();
                let (event, rite);
                let mut no: (Option<i64>, Option<i64>);
                match section {
                    1 => {
                        event = date(13, 14, 15, &mut report);
                        rite = date(16, 17, 18, &mut report);
                        let child_sex = sex(g(12));
                        let num_rec = n(11);
                        no = if child_sex.as_deref() == Some("Ж") { (None, num_rec) } else { (num_rec, None) };
                        if let Some(p) = fam.person("child", 10, Names { first: g(19), ..Default::default() },
                                                    child_sex.clone(), None, None, None)? {
                            if child_sex.is_none() && p.gender.as_deref() == Some("Ж") {
                                no = (None, num_rec);
                            }
                            persons.push(p);
                        }
                        let parents_place = g(20);
                        let father = Names { surname: surname(g(23), g(24)), first: g(25), patr: g(26) };
                        // Н.П. приписки — общий для родителей: безымянному
                        // родителю без звания он не нужен (у незаконнорождённых
                        // отца нет, и «отец без имени: Багаево» — шум).
                        let place_for = |n: &Names, rank: &Option<String>| if n.is_empty() && rank.is_none() { None } else { parents_place.clone() };
                        let father_place = place_for(&father, &g(22));
                        if let Some(p) = fam.person("father", 20, father, Some("М".into()), g(22), father_place, None)? {
                            persons.push(p);
                        }
                        let mother = Names { surname: g(28), first: g(30), patr: g(31) };
                        let mother_place = place_for(&mother, &g(27));
                        if let Some(mut p) = fam.person("mother", 30, mother, Some("Ж".into()), g(27), mother_place, None)? {
                            p.maiden_surname = g(29);
                            persons.push(p);
                        }
                        for k in 0..4u32 {
                            let c = 32 + 4 * k;
                            let role = ["godparent1", "godparent2", "godparent3", "godparent4"][k as usize];
                            let names = match g(c + 3) {
                                Some(fio) => fam.split_fio(&fio, false)?,
                                None => Names::default(),
                            };
                            if let Some(p) = fam.person(role, 40 + 10 * k as i64, names, None, g(c), g(c + 1), None)? {
                                persons.push(p);
                            }
                        }
                        // Родственник восприемника или родителя: в форме для
                        // него места нет — в примечание записи, ничего не теряя.
                        for k in 0..2u32 {
                            let c = 48 + 6 * k;
                            let parts: Vec<String> = [g(c + 2), g(c + 5), g(c + 3).map(|p| format!("({p})"))]
                                .into_iter().flatten().collect();
                            if parts.is_empty() {
                                continue;
                            }
                            let whose = [g(c), g(c + 1)].into_iter().flatten().collect::<Vec<_>>().join(", ");
                            notes.push(format!("родственник{}: {}", if whose.is_empty() { String::new() } else { format!(" ({whose})") },
                                               parts.join(" ")));
                            kin_of_godparents += 1;
                        }
                    }
                    2 => {
                        event = date(12, 13, 14, &mut report);
                        rite = (None, None, None);
                        no = (n(11), None);
                        let groom = Names { surname: surname(g(18), g(19)), first: g(20), patr: g(21) };
                        if let Some(mut p) = fam.person("groom", 10, groom, Some("М".into()), g(17), g(15), None)? {
                            p.marriage_order = marriage_order(g(22));
                            p.age_years = n(23);
                            if let Some(v) = g(23).filter(|_| n(23).is_none()) {
                                notes.push(format!("возраст (жених): {v}"));
                            }
                            persons.push(p);
                        }
                        let bride = Names { surname: g(37), first: g(38), patr: g(39) };
                        if let Some(mut p) = fam.person("bride", 20, bride, Some("Ж".into()), g(36), g(34), None)? {
                            p.marriage_order = marriage_order(g(40));
                            p.age_years = n(41);
                            if let Some(v) = g(41).filter(|_| n(41).is_none()) {
                                notes.push(format!("возраст (невеста): {v}"));
                            }
                            persons.push(p);
                        }
                        for (who, c) in [("жених", 24u32), ("невеста", 42)] {
                            let more = age_text([None, g(c), g(c + 1), g(c + 2)]);
                            if let Some(a) = more {
                                notes.push(format!("возраст ({who}): {a}"));
                            }
                        }
                        for (role, order, c) in [("groom_relative", 30i64, 27u32), ("bride_relative", 50, 45)] {
                            let names = Names { surname: surname(g(c + 3), g(c + 4)), first: g(c + 5), patr: g(c + 6) };
                            if let Some(mut p) = fam.person(role, order, names, Some("М".into()), g(c + 2), g(c), None)? {
                                p.kinship = Some("отец".into());
                                persons.push(p);
                            }
                        }
                        for k in 0..6u32 {
                            let c = 52 + 4 * k;
                            let role = ["witness1", "witness2", "witness3", "witness4", "witness5", "witness6"][k as usize];
                            let order = [60i64, 70, 80, 90, 92, 94][k as usize];
                            let side = if k < 3 { "по жениху" } else { "по невесте" };
                            let names = match g(c + 3) {
                                Some(fio) => fam.split_fio(&fio, false)?,
                                None => Names::default(),
                            };
                            if let Some(p) = fam.person(role, order, names, None, g(c), g(c + 1), Some(side.into()))? {
                                persons.push(p);
                            }
                        }
                        // Родственник жениха или невесты — в свою роль с
                        // родством; родственник поручителя — в примечание.
                        for k in 0..2u32 {
                            let c = 76 + 6 * k;
                            let Some(fio) = g(c + 5) else { continue };
                            let whose = g(c).map(|w| normalize(&w)).unwrap_or_default();
                            let role = if whose.starts_with("жених") { Some(("groom_relative", 35i64)) }
                                       else if whose.starts_with("невест") { Some(("bride_relative", 55)) } else { None };
                            // Второй родственник той же стороны — в примечание: форма
                            // показывает одного (ревьюер 21.10.2026).
                            let taken = role.is_some_and(|(r, _)| persons.iter().any(|p| p.role_code == r));
                            match role.filter(|_| !taken) {
                                Some((role, order)) => {
                                    let kinship = g(c + 1);
                                    let names = fam.split_fio(&fio, false)?;
                                    let gender = kinship.as_deref().and_then(kin_gender);
                                    if let Some(mut p) = fam.person(role, order, names, gender, g(c + 2), g(c + 3), None)? {
                                        p.kinship = kinship;
                                        persons.push(p);
                                    }
                                }
                                None => {
                                    let whose = [g(c), g(c + 1)].into_iter().flatten().collect::<Vec<_>>().join(", ");
                                    let parts: Vec<String> = [g(c + 2), Some(fio), g(c + 3).map(|p| format!("({p})"))]
                                        .into_iter().flatten().collect();
                                    notes.push(format!("родственник ({whose}): {}", parts.join(" ")));
                                }
                            }
                        }
                    }
                    _ => {
                        event = date(13, 14, 15, &mut report);
                        rite = date(16, 17, 18, &mut report);
                        let dead_sex = sex(g(12));
                        let num_rec = n(11);
                        no = if dead_sex.as_deref() == Some("Ж") { (None, num_rec) } else { (num_rec, None) };
                        let dead = Names { surname: surname(g(22), g(23)), first: g(24), patr: g(25) };
                        let age = age_text([g(26), g(27), g(28), g(29)]);
                        let cause = g(38);
                        let has = !dead.is_empty() || g(21).is_some() || age.is_some() || cause.is_some();
                        if has {
                            let mut p = match fam.person("deceased", 10, dead, dead_sex.clone(), g(21), g(19), None)? {
                                Some(p) => p,
                                // Без имени — «личность не установлена», как в форме.
                                None => PersonInput {
                                    role_code: "deceased".into(), sort_order: 10, surname: None, first_name: None,
                                    patronymic: None, surname_modern: None, first_name_modern: None,
                                    patronymic_modern: None, maiden_surname: None, gender: dead_sex.clone(), rank: g(21),
                                    confession: None, place: g(19), note: None, uncertain: None, age_years: None,
                                    marriage_order: None, kinship: None, age_months: None, age_weeks: None,
                                    age_days: None, age_text: None, death_cause: None,
                                },
                            };
                            // Умерший без имени — запись «личность не установлена»
                            // со званием и местом, а не «без имени» в примечании.
                            fam.orphans.retain(|o| !o.starts_with(role_title("deceased")));
                            if dead_sex.is_none() && p.gender.as_deref() == Some("Ж") {
                                no = (None, num_rec);
                            }
                            p.age_years = n(26);
                            p.age_months = n(27);
                            p.age_weeks = n(28);
                            p.age_days = n(29);
                            // «0,5» и «3,5 нед» — числа тем же разбором, что в форме
                            // (ревьюер 21.10.2026: в Акшине 469 возрастов «0,5»).
                            let whole = [26u32, 27, 28, 29].iter().all(|t| g(*t).is_none() || n(*t).is_some());
                            if let Some(a) = age.as_deref().filter(|_| !whole).and_then(crate::age::parse_age) {
                                (p.age_years, p.age_months, p.age_weeks, p.age_days) = (a.years, a.months, a.weeks, a.days);
                            }
                            p.age_text = age;
                            p.death_cause = cause;
                            persons.push(p);
                        }
                        let kin = Names { surname: surname(g(34), g(35)), first: g(36), patr: g(37) };
                        let kinship = g(30);
                        if let Some(mut p) = fam.person("deceased_relative", 40, kin, kinship.as_deref().and_then(kin_gender),
                                                        g(33), g(31), None)? {
                            let kinship = kinship.unwrap_or_else(|| {
                                let guess = if p.gender.as_deref() == Some("Ж") { "мать" } else { "отец" };
                                report.notes.push(format!("{here}: родство родственника умершего не указано — записано «{guess}»"));
                                guess.to_string()
                            });
                            if p.gender.is_none() {
                                p.gender = kin_gender(&kinship);
                            }
                            p.kinship = Some(kinship);
                            persons.push(p);
                        }
                        if let Some(where_) = g(51) {
                            notes.push(format!("где погребён: {where_}"));
                        }
                    }
                }
                // Причт. У смертей исповедовал и погребал обычно один
                // священник — тогда он один, с пометкой «исповедовал» (так
                // выгрузка поставит его в обе колонки).
                let clergy_cols: &[(u32, u32)] = match section {
                    1 => &[(60, 61), (64, 65), (66, 67)],
                    2 => &[(88, 89), (92, 93), (94, 95)],
                    _ => &[(43, 44), (47, 48), (49, 50)],
                };
                let mut clergy: Vec<(Option<String>, String, Option<String>)> = Vec::new();
                for (rc, fc) in clergy_cols {
                    if let Some(fio) = g(*fc) {
                        clergy.push((g(*rc), fio, None));
                    }
                }
                if section == 3 {
                    if let Some(fio) = g(40) {
                        let same = clergy.iter().position(|c| normalize(&c.1) == normalize(&fio));
                        match same {
                            Some(k) => clergy[k].2 = Some("исповедовал".into()),
                            // В конец: выгрузка раскладывает причт по сану, и
                            // первым священником должен остаться погребавший
                            // (ревьюер 21.10.2026, Акшино, СМЕРТЬ, строка 475).
                            None if clergy.len() < 3 => clergy.push((g(39), fio, Some("исповедовал".into()))),
                            None => notes.push(format!("исповедовал: {}", [g(39), Some(fio)].into_iter().flatten()
                                .collect::<Vec<_>>().join(" "))),
                        }
                    }
                }
                for (k, (rank, fio, note)) in clergy.into_iter().enumerate() {
                    // «священник Алексей Оранский» (Шуя): звание внутри ФИО.
                    let mut words: Vec<&str> = fio.split_whitespace().collect();
                    let mut lead = Vec::new();
                    while words.len() > 1 && words[0].chars().next().is_some_and(char::is_lowercase) {
                        lead.push(words.remove(0));
                    }
                    let rank = rank.or_else(|| Some(lead.join(" ")).filter(|v| !v.is_empty()));
                    let names = fam.split_fio(&words.join(" "), true)?;
                    let role = ["clergy1", "clergy2", "clergy3"][k];
                    if let Some(p) = fam.person(role, 100 + 10 * k as i64, names, Some("М".into()), rank, None, note)? {
                        persons.push(p);
                    }
                }
                let other: &[(&str, u32, u32)] = match section {
                    1 => &[("", 62, 63)],
                    2 => &[("", 90, 91)],
                    _ => &[(" (исповедь)", 41, 42), (" (погребение)", 45, 46)],
                };
                for (what, ch, pl) in other {
                    let parts: Vec<String> = [g(*ch), g(*pl)].into_iter().flatten().collect();
                    if !parts.is_empty() {
                        notes.push(format!("церковь другого прихода{what}: {}", parts.join(", ")));
                    }
                }
                notes.append(&mut fam.orphans);
                let year = book_year(i, row, cols);
                // Дело записи — дело её реквизитов (Роман 26.10.2026), и
                // декабрьская запись остаётся в своей книге.
                let row_case = case_key(row, cols, year).and_then(|k| cases.get(&k));
                if let Some(case) = row_case {
                    if let Some(v) = g(7).filter(|v| case.village.as_deref().is_some_and(|c| normalize(c) != normalize(v))) {
                        notes.push(format!("Н.П. события: {v}"));
                    }
                }
                let (pom, author) = match section { 1 => (g(68), g(69)), 2 => (g(96), g(97)), _ => (g(52), g(53)) };
                notes.extend(pom);
                notes.extend(author);

                let (ed, em, ey) = event;
                let (rd, rm, ry) = rite;
                let (event_year, rite_year) = if section == 2 { (ey.or(year), None) } else { (ey.or(year), ry.or(year)) };
                if event_year.is_none() && rite_year.is_none() {
                    report.notes.push(format!(
                        "{here}: нет года — запись перенесена без года; в списках по годам её не видно, в выгрузке она идёт только со всем приходом"));
                }
                if let Some(y) = year.filter(|y| odd.contains(y)) {
                    report.notes.push(format!(
                        "{here}: год {y} стоит далеко от остальных ({usual}) — не опечатка ли? Запись отнесена к делу {y} года"));
                }
                if persons.is_empty() {
                    report.notes.push(format!("{here}: в строке нет ни одного имени — запись перенесена как есть"));
                }
                let note = Some(notes.join("; ")).filter(|v| !v.is_empty());
                let mut entry_note = None;
                let main_role = match section { 1 => None, 2 => Some("groom"), _ => Some("deceased") };
                match main_role.and_then(|role| persons.iter_mut().find(|p| p.role_code == role)) {
                    Some(p) => p.note = note,
                    None => {
                        // Невеста без жениха — примечание ей; иначе в запись.
                        match persons.iter_mut().find(|p| section == 2 && p.role_code == "bride") {
                            Some(p) => p.note = note,
                            None => entry_note = note,
                        }
                    }
                }
                let case_id = row_case.map(|c| c.id).unwrap_or(first_case);
                let entry = EntryInput {
                    id: None,
                    case_id,
                    section,
                    page: g(6),
                    no_male: no.0,
                    no_female: no.1,
                    event_day: ed,
                    event_month: em,
                    event_year,
                    rite_day: if section == 2 { None } else { rd },
                    rite_month: if section == 2 { None } else { rm },
                    rite_year,
                    note: entry_note,
                    uncertain: None,
                    scan_file: None,
                    persons,
                };
                let key = twin_key(section, year, ed, em, &entry.persons);
                let saved = save_entry_in_tx(conn, &entry, &parish_key).map_err(|e| format!("{here}: {e}"))?;
                // Сохранение привязывает запись к делу её года; дело импорта —
                // по реквизитам строки (п. 88), его и ставим.
                if row_case.is_some() {
                    conn.execute(&statement("entry_set_case")?, rusqlite::named_params! { ":id": saved.id, ":case_id": case_id })
                        .map_err(|e| e.to_string())?;
                }
                if let Some(key) = key {
                    match twins.get(&key) {
                        Some((fi0, f0, s0, r0, id0)) if *fi0 != fi => {
                            let first_at = at(f0, s0, *r0);
                            report.notes.push(format!("{here}: та же запись, что {first_at} — перенесены обе"));
                            review("duplicate", &format!("та же запись есть в файле «{f0}», лист «{s0}», строка {r0}"),
                                   &review_sheet, r, Some(saved.id))?;
                            if twins_flagged.insert(*id0) {
                                review("duplicate", &format!("та же запись есть в файле «{file}», лист «{name}», строка {r}"),
                                       &format!("{s0} | {f0}"), *r0, Some(*id0))?;
                            }
                        }
                        Some(_) => {}
                        None => {
                            twins.insert(key, (fi, file.clone(), name.to_string(), r, saved.id));
                        }
                    }
                }
                for line in &report.notes[notes_from..] {
                    let text = line.split_once(": ").map(|x| x.1.to_string()).unwrap_or_else(|| line.clone());
                    if !text.starts_with("та же запись") {
                        review("note", &text, &review_sheet, r, Some(saved.id))?;
                    }
                }
                for (kind, text) in fam.imp.pending.drain(..) {
                    review(kind, &text, &review_sheet, r, Some(saved.id))?;
                }
                match section {
                    1 => report.births += 1,
                    2 => report.marriages += 1,
                    _ => report.deaths += 1,
                }
            }
        }
    }

    // Дело без единой строки с годом — без года, даже если сохранение записи
    // другого года успело его «удочерить» (п. 95).
    for case in cases.values().filter(|c| c.year.is_none()) {
        conn.execute(&statement("case_clear_year")?, rusqlite::named_params! { ":id": case.id })
            .map_err(|e| e.to_string())?;
    }
    // Дела, которые сохранение завело под год записи, а импорт её переставил, —
    // пустые: убрать.
    conn.execute(&statement("case_drop_empty_after")?, rusqlite::named_params! { ":last": order.len() as i64 })
        .map_err(|e| e.to_string())?;

    // Пункты листов location всех файлов.
    for (file, book) in &books {
        for (r, text) in add_places(conn, &book.locations, &format!("файл «{file}», лист «location»"))? {
            review("note", &text, &format!("location | {file}"), r, None)?;
            report.notes.push(format!("{}: {text}", at(file, "location", r)));
        }
    }
    // Пункты записей без карточки — на сверку, по одному на пункт (долг п. 80):
    // без типа и уезда пункт не найти на Familio и не отличить от тёзки.
    let bare: Vec<(String, i64, i64)> = {
        let mut st = conn.prepare(&statement("place_without_card")?).map_err(|e| e.to_string())?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())?
    };
    for (place, entry, n) in &bare {
        conn.execute(&review_add, rusqlite::named_params! {
            ":kind": "note",
            ":text": format!("пункт «{place}» без карточки (нет ни типа, ни уезда, ни губернии), упоминаний {n} — заполните карточку у поля НП"),
            ":sheet": Option::<String>::None, ":row": Option::<i64>::None, ":entry_id": entry,
        }).map_err(|e| e.to_string())?;
    }
    if !bare.is_empty() {
        report.notes.push(format!("пунктов без карточки: {} — на сверке ({})", bare.len(),
                                  bare.iter().take(10).map(|b| b.0.as_str()).collect::<Vec<_>>().join(", ")));
    }
    if kin_of_godparents > 0 {
        report.notes.push(format!(
            "родственников восприемников и родителей: {kin_of_godparents} — в форме для них места нет, они в примечании записи"));
    }

    report.persons = count("SELECT count(*) FROM person_mention WHERE role_code NOT LIKE 'clergy%'")?;
    report.places = count("SELECT count(*) FROM place")? - places_before;
    let top = |map: BTreeMap<String, usize>| -> Vec<String> {
        let mut names: Vec<(String, usize)> = map.into_iter().collect();
        names.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        names.iter().take(20).map(|(n, k)| if *k > 1 { format!("{n} ({k})") } else { n.clone() }).collect()
    };
    report.unknown_names = fam.imp.unknown_total;
    report.unknown_examples = top(std::mem::take(&mut fam.imp.unknown));
    report.unknown_patronymics = fam.imp.unknown_patr_total;
    report.unknown_patronymic_examples = top(std::mem::take(&mut fam.imp.unknown_patr));
    Ok(report)
}

/// Приход по файлам: самый частый «Н.П. события» всех файлов.
pub fn parish_village(seen: &[FileSeen]) -> String {
    let all: Vec<String> = seen.iter().filter(|s| !s.village.is_empty()).map(|s| s.village.clone()).collect();
    most_common(&all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn seed() -> Option<PathBuf> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../resources/seed.sqlite");
        p.exists().then_some(p)
    }

    fn fresh(tag: &str, seed: &Path) -> (PathBuf, Connection) {
        let path = std::env::temp_dir().join(format!("genmetric-familio-{tag}-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::fs::copy(seed, &path).unwrap();
        let conn = Connection::open(&path).unwrap();
        (path, conn)
    }

    fn import(conn: &Connection, files: &[(String, Vec<u8>)]) -> ImportReport {
        conn.execute_batch("BEGIN").unwrap();
        let rep = import_into(conn, files).unwrap();
        conn.execute_batch("COMMIT").unwrap();
        rep
    }

    fn about() -> crate::export::About {
        crate::export::About {
            title: "Тест".into(), years: String::new(), min_year: String::new(), description: String::new(),
            author: "Тест Тестов".into(), profile: String::new(), telegram: String::new(), is_new: true, previous: String::new(),
        }
    }

    /// Люди прихода в современном написании — то, что выгрузка Familio
    /// передаёт и что должно вернуться обратно.
    fn people(c: &Connection) -> Vec<String> {
        let mut st = c.prepare(
            "SELECT e.section || '|' || coalesce(e.page, '') || '|' || coalesce(e.no_male, e.no_female, '')
                    || '|' || coalesce(e.event_day, '') || '.' || coalesce(e.event_month, '') || '.' || coalesce(e.event_year, '')
                    || '|' || coalesce(e.rite_day, '') || '.' || coalesce(e.rite_month, '') || '.' || coalesce(e.rite_year, '')
                    || '|' || CASE WHEN p.role_code LIKE 'witness%' THEN 'witness' ELSE p.role_code END
                    || '|' || coalesce(p.first_name_modern, p.first_name, '') || ' ' || coalesce(p.patronymic_modern, p.patronymic, '')
                    || ' ' || coalesce(p.surname, '')
                    || '|' || coalesce(p.rank, '') || '|' || coalesce(pl.name, '')
                    || '|' || coalesce(p.kinship, '') || '|' || coalesce(p.age_years, '')
                    || '|' || coalesce(p.death_cause, '') || '|' || coalesce(c.fond, '') || '/' || coalesce(c.delo, '')
                    || '|' || coalesce(c.year, '')
               FROM entry e JOIN person_mention p ON p.entry_id = e.id JOIN mk_case c ON c.id = e.case_id
               LEFT JOIN place pl ON pl.id = p.place_id
              -- Родственник одним родством, без имени, — заготовка формы, не данные: в Familio не выгружается.
              WHERE trim(coalesce(p.first_name, '') || coalesce(p.surname, '') || coalesce(p.patronymic, '')) <> ''
              ORDER BY e.section, coalesce(e.rite_year, e.event_year), e.id, p.role_code").unwrap();
        let rows = st.query_map([], |r| r.get::<_, String>(0)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    /// Критерий спеки: наша собственная выгрузка Familio загружается обратно
    /// без потерь — записи, люди, даты, пункты, причт.
    #[test]
    fn own_export_comes_back() {
        let Some(seed) = seed() else {
            crate::seed_missing("тест импорта Familio пропущен");
            return;
        };
        let (path_a, a) = fresh("own-a", &seed);
        a.execute_batch("BEGIN").unwrap();
        crate::import::import_into(&a, include_bytes!("../../../db/fixtures/indexer.xlsx")).unwrap();
        a.execute_batch("COMMIT").unwrap();
        // «Фамилия-2»: вторая фамилия в скобках уходит в свою колонку и возвращается.
        assert_eq!(a.execute("UPDATE person_mention SET surname = 'Глухов (Логунков)' WHERE id = (SELECT min(id) FROM person_mention WHERE role_code = 'father')", []).unwrap(), 1);
        let (bytes, _) = crate::export::familio_bytes(&a, &[], &about()).unwrap();
        let birth = read_sheet(&bytes, "РОЖДЕНИЕ").unwrap();
        assert!(birth.values().any(|r| r.get(&24).map(|v| v == "Логунков").unwrap_or(false)), "Фамилия-2 — в колонке X: {:?}",
                birth.values().map(|r| (r.get(&23).cloned(), r.get(&24).cloned())).collect::<Vec<_>>());

        let seen = inspect("own.xlsx", &bytes).unwrap();
        assert_eq!((seen.births, seen.marriages, seen.deaths), (4, 2, 4), "{seen:?}");
        assert_eq!(seen.village, "Никольское");
        assert!(seen.missing.is_empty(), "{:?}", seen.missing);

        let (path_b, b) = fresh("own-b", &seed);
        let rep = import(&b, &[("own.xlsx".into(), bytes)]);
        assert_eq!((rep.births, rep.marriages, rep.deaths), (4, 2, 4), "{rep:?}");
        let (pa, pb) = (people(&a), people(&b));
        let only_a: Vec<&String> = pa.iter().filter(|x| !pb.contains(x)).collect();
        let only_b: Vec<&String> = pb.iter().filter(|x| !pa.contains(x)).collect();
        assert!(only_a.is_empty() && only_b.is_empty(), "\nбыло:\n{}\nстало:\n{}",
                only_a.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n"),
                only_b.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n"));
        drop((a, b));
        let _ = std::fs::remove_file(path_a);
        let _ = std::fs::remove_file(path_b);
    }

    /// Файл шаблона с заданными строками: (лист, номер колонки → значение).
    fn file(rows: &[(&str, Vec<(u32, &str)>)]) -> Vec<u8> {
        // Листы шаблона — с примерами записей: пустые листы их стирают, как в выгрузке.
        let mut fills: Vec<crate::xlsx::SheetRows> = ["РОЖДЕНИЕ", "БРАК", "СМЕРТЬ", "location"].iter()
            .map(|s| crate::xlsx::SheetRows { sheet: s.to_string(), first_row: if *s == "location" { 3 } else { 4 }, rows: vec![] })
            .collect();
        for (sheet, cells) in rows {
            let width = cells.iter().map(|c| c.0).max().unwrap_or(1) as usize;
            let mut row = vec![None; width];
            for (c, v) in cells {
                row[*c as usize - 1] = Some(v.to_string());
            }
            match fills.iter_mut().find(|f| f.sheet == *sheet) {
                Some(f) => f.rows.push(row),
                None => unreachable!(),
            }
        }
        let author = crate::xlsx::CellValue { sheet: "about".into(), cell: "B9".into(), value: String::new() };
        crate::xlsx::fill_template(FAMILIO_TEMPLATE, &fills, &[author], &[]).unwrap()
    }

    fn birth(year: &str, child: &str, father: &str, surname: &str) -> (&'static str, Vec<(u32, &'static str)>) {
        let leak = |s: &str| -> &'static str { Box::leak(s.to_string().into_boxed_str()) };
        ("РОЖДЕНИЕ", vec![(1, "1"), (2, "ГАХК"), (3, "56"), (5, "12"), (7, "Тестово"), (9, "Никольская"), (11, "1"), (12, "муж"),
                          (13, "3"), (14, "5"), (15, leak(year)), (18, leak(year)), (19, leak(child)), (20, "Заречье"), (22, "крестьянин"),
                          (23, leak(surname)), (25, leak(father)), (26, "Петрович"), (35, "Боровков Иван Федотович")])
    }

    /// Порядок слов «ФИО» одной колонкой — по словарю имён (Роман 19.10).
    #[test]
    fn fio_order() {
        let Some(seed) = seed() else { return };
        let (path, conn) = fresh("fio", &seed);
        let mut fam = Familio {
            imp: Importer { conn: &conn, unknown: BTreeMap::new(), unknown_total: 0, unknown_patr: BTreeMap::new(),
                            unknown_patr_total: 0, pending: Vec::new() },
            is_name: HashMap::new(),
            orphans: Vec::new(),
        };
        let mut split = |t: &str, clergy: bool| {
            let n = fam.split_fio(t, clergy).unwrap();
            [n.surname, n.first, n.patr].map(|v| v.unwrap_or_default()).join("/")
        };
        assert_eq!(split("Боровков Иван Федотович", false), "Боровков/Иван/Федотович");
        assert_eq!(split("Иван Федотович Боровков", false), "Боровков/Иван/Федотович");
        assert_eq!(split("Талантина(?) Екатерина Іоаннова", false), "Талантина(?)/Екатерина/Іоаннова");
        assert_eq!(split("Акилина Лаврентьевна", false), "/Акилина/Лаврентьевна");
        assert_eq!(split("Усанов Евфросиния Ивановна", false), "Усанов/Евфросиния/Ивановна");
        assert_eq!(split("Усанов Евфросинья Ивановна", false), "Усанов/Евфросинья/Ивановна", "имя вне словаря");
        assert_eq!(split("Александр Аверьянов", true), "Аверьянов/Александр/", "у причта — имя и фамилия");
        assert_eq!(split("Мухин Степан", false), "Мухин/Степан/");
        assert_eq!(split("Ильина Анастасия Ивановна", false), "Ильина/Анастасия/Ивановна", "фамилия, похожая на имя");
        assert_eq!(split("Игнатьевич", false), "//Игнатьевич");
        assert_eq!(split("Иванова Мароа Михайлова", false), "Иванова/Мароа/Михайлова", "долг п. 79");
        assert_eq!(split("Воробьева Мароа(?) Иосиина", false), "Воробьева/Мароа(?)/Иосиина", "долг п. 79");
        assert_eq!(split("Евфимиан Артемов Сидоров", false), "Сидоров/Евфимиан/Артемов", "долг п. 82: имя вне словаря, «Имя Отчество Фамилия»");
        assert_eq!(split("Петрова Улияна Иванова", false), "Петрова/Улияна/Иванова", "п. 96: середина на «-на» вне словаря отчеств — имя");
        assert_eq!(split("Евфимиан Артемьев Сидоров", false), "Сидоров/Евфимиан/Артемьев", "п. 96: середина из словаря отчеств — отчество");
        drop(fam);
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn small_parts() {
        assert_eq!(strip_word(Some("Воронежская губ.".into()), &["губерния", "губ."]).as_deref(), Some("Воронежская"));
        assert_eq!(strip_word(Some("Свияжский уезд".into()), &["уезд", "у."]).as_deref(), Some("Свияжский"));
        assert_eq!(strip_word(Some("Казанская".into()), &["губ."]).as_deref(), Some("Казанская"));
        assert_eq!(age_text([Some("53".into()), None, None, None]).as_deref(), Some("53"));
        assert_eq!(age_text([None, Some("3".into()), None, None]).as_deref(), Some("3 мес"));
        assert_eq!(age_text([Some("1".into()), Some("3".into()), None, None]).as_deref(), Some("1 г 3 мес"));
        assert_eq!(crate::age::parse_age("1 г 3 мес").map(|a| (a.years, a.months)), Some((Some(1), Some(3))));
        assert_eq!(age_text([None, None, Some("3,5".into()), None]).as_deref(), Some("3,5 нед"));
        assert_eq!(soft_sign("Игнатьевич"), "Игнатиевич");
        assert_eq!(soft_sign("Евфросинья"), "Евфросиния");
        assert_eq!(marriage_order(Some("2".into())).as_deref(), Some("Вторым браком"));
        assert_eq!(surname(Some("Глухов".into()), Some("Логунков".into())).as_deref(), Some("Глухов (Логунков)"));
        assert!(strong_patronymic("Лаврентьевна") && strong_patronymic("Козьмич") && !strong_patronymic("Артемов"));
    }

    /// Колонки — по подписям: в «Талызино» нет «Девичьей М», всё правее
    /// сдвинуто на одну колонку.
    #[test]
    fn columns_by_labels() {
        let template = read_sheet(FAMILIO_TEMPLATE, "РОЖДЕНИЕ").unwrap();
        let mut shifted = Sheet::new();
        for (r, row) in &template {
            let mut out = Row::new();
            for (c, v) in row {
                match c.cmp(&29) {
                    std::cmp::Ordering::Less => { out.insert(*c, v.clone()); }
                    std::cmp::Ordering::Equal => {
                        // Объединённая группа «Мать» начинается левее — её текст остаётся.
                    }
                    std::cmp::Ordering::Greater => { out.insert(c - 1, v.clone()); }
                }
            }
            shifted.insert(*r, out);
        }
        let cols = map_columns(&template, &shifted);
        assert_eq!(cols.to_file.get(&30), Some(&29), "«Имя М» — на колонку левее");
        assert_eq!(cols.to_file.get(&35), Some(&34), "«ФИО» восприемника №1");
        assert_eq!(cols.to_file.get(&39), Some(&38), "«ФИО» восприемника №2");
        assert_eq!(cols.to_file.get(&69), Some(&68));
        assert!(!cols.to_file.contains_key(&29));
        assert_eq!(cols.missing.iter().map(|t| cols.titles[t].clone()).collect::<Vec<_>>(), vec!["девичья м".to_string()]);
    }

    /// Спека, дополнение 20.10: несколько файлов — один приход; одна и та же
    /// запись в двух файлах — обе в базе и обе на сверке; строка без года —
    /// в базе и на сверке; пустая строка шаблона («#N/A») — не запись.
    #[test]
    fn several_files() {
        let Some(seed) = seed() else { return };
        let a = file(&[birth("1889", "Нил", "Кузьма", "Петров"), birth("1889", "Ия", "Лука", "Сидоров"),
                       ("РОЖДЕНИЕ", vec![(1, "3"), (8, "#N/A"), (21, "#N/A")])]);
        let b = file(&[birth("1890", "Иов", "Фома", "Глухов"), birth("1889", "Нил", "Кузьма", "Петров"),
                       ("РОЖДЕНИЕ", vec![(1, "3"), (6, "17"), (19, "Петр"), (25, "Иван")])]);
        let seen = inspect("a.xlsx", &a).unwrap();
        assert_eq!((seen.births, seen.marriages, seen.deaths, seen.years.clone()), (2, 0, 0, vec![1889]), "{seen:?}");
        assert!(seen.empty_rows >= 1 && seen.indexer.is_empty(), "{seen:?}");
        let (path, conn) = fresh("several", &seed);
        let rep = import(&conn, &[("a.xlsx".into(), a), ("b.xlsx".into(), b)]);
        assert_eq!((rep.births, rep.marriages, rep.deaths), (5, 0, 0), "{rep:?}");
        assert_eq!(rep.years, vec![1889, 1890]);
        let num = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        let text = |sql: &str| -> String { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(num("SELECT count(*) FROM mk_case"), 1, "одни реквизиты (ГАХК, ф. 56, д. 12) в обоих файлах — одно дело (Роман 26.10.2026)");
        assert_eq!(num("SELECT count(DISTINCT parish_key) FROM mk_case"), 1);
        assert_eq!(num("SELECT count(*) FROM review_item WHERE kind = 'duplicate'"), 2, "дубль — обе записи на сверке");
        assert_eq!(text("SELECT group_concat(sheet || ' ' || row, '; ') FROM (SELECT * FROM review_item WHERE kind = 'duplicate' ORDER BY sheet)"),
                   "РОЖДЕНИЕ | a.xlsx 4; РОЖДЕНИЕ | b.xlsx 5");
        assert_eq!(num("SELECT count(*) FROM entry WHERE event_year IS NULL AND rite_year IS NULL"), 1, "строка без года — в базе");
        assert_eq!(num("SELECT count(*) FROM review_item WHERE kind = 'note' AND text LIKE '%нет года%' AND sheet = 'РОЖДЕНИЕ | b.xlsx' AND row = 6"), 1);
        assert_eq!(text("SELECT surname FROM person_mention WHERE role_code = 'godparent1' LIMIT 1"), "Боровков");
        assert_eq!(text("SELECT patronymic_modern FROM person_mention WHERE role_code = 'father' LIMIT 1"), "Петрович");
        // Долг п. 80: пункта «Заречье» на листе location нет — один раз на сверке.
        assert_eq!(num("SELECT count(*) FROM review_item WHERE text LIKE 'пункт «Заречье» без карточки%' AND entry_id IS NOT NULL"), 1);
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    /// Ревьюер 21.10.2026: исповедовавший — не первым (выгрузка поставила бы
    /// его погребавшим), церкви двух блоков — порознь; реквизиты строки,
    /// отличные от дела года, — не молча; без блока «исповедовал» колонки
    /// погребавшего остаются его.
    #[test]
    fn death_clergy_and_case() {
        let Some(seed) = seed() else { return };
        let death = |delo: &'static str, conf: &'static str| ("СМЕРТЬ", vec![
            (1, "1"), (2, "ГА"), (3, "56"), (5, delo), (7, "Тестово"), (9, "Никольская"), (11, "1"), (12, "муж"),
            (15, "1889"), (18, "1889"), (24, "Петр"), (26, "0,5"), (39, "священник"), (40, conf), (45, "Спасская"),
            (46, "Старое Тестово"), (43, "священник"), (44, "Алексей Элпидинский"), (49, "псаломщик"), (50, "Иван Петров")]);
        let bytes = file(&[death("12", "Петр Лентовский"), death("13", "Алексей Элпидинский")]);
        let (path, conn) = fresh("clergy", &seed);
        import(&conn, &[("d.xlsx".into(), bytes)]);
        let text = |sql: &str| -> String { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(text("SELECT group_concat(role_code || ':' || first_name || ' ' || surname || ':' || coalesce(note, ''), '; ')
                           FROM (SELECT * FROM person_mention WHERE role_code LIKE 'clergy%' AND entry_id = 1 ORDER BY sort_order)"),
                   "clergy1:Алексей Элпидинский:; clergy2:Иван Петров:; clergy3:Петр Лентовский:исповедовал");
        assert_eq!(text("SELECT group_concat(coalesce(note, ''), '; ') FROM (SELECT * FROM person_mention WHERE role_code LIKE 'clergy%' AND entry_id = 2 ORDER BY sort_order)"),
                   "исповедовал; ", "тот же священник — один, с пометкой");
        let note = text("SELECT note FROM person_mention WHERE role_code = 'deceased' AND entry_id = 2");
        assert!(note.contains("церковь другого прихода (погребение): Спасская, Старое Тестово"), "{note}");
        assert_eq!(text("SELECT CAST(age_months AS TEXT) || '/' || age_text FROM person_mention WHERE role_code = 'deceased' AND entry_id = 1"), "6/0,5");
        // Роман 26.10.2026: одни реквизиты — одно дело; запись того же года с
        // другим делом — в своём деле, а не в деле года.
        assert_eq!(text("SELECT group_concat(c.delo || ':' || c.year, ' ') FROM (SELECT c.* FROM entry e JOIN mk_case c ON c.id = e.case_id ORDER BY e.id) c"),
                   "12:1889 13:1889");
        drop(conn);
        let _ = std::fs::remove_file(path);
        // Нет блока «кто исповедывал» (колонки 39–42): погребавший — в своих колонках.
        let template = read_sheet(FAMILIO_TEMPLATE, "СМЕРТЬ").unwrap();
        let mut cut = Sheet::new();
        for (r, row) in &template {
            cut.insert(*r, row.iter().filter(|(c, _)| !(39..=42).contains(*c))
                .map(|(c, v)| (if *c > 42 { c - 4 } else { *c }, v.clone())).collect());
        }
        let cols = map_columns(&template, &cut);
        assert_eq!((cols.to_file.get(&44), cols.to_file.get(&40)), (Some(&40), None));
    }

    /// Роман 26.10.2026: «декабрьская запись 1890 года физически находится в
    /// книге „дело 7“ — она должна остаться именно в „деле 7“». Год дела — год
    /// большинства записей; правка записи без смены года дело не меняет.
    #[test]
    fn december_stays_in_its_book() {
        let Some(seed) = seed() else { return };
        let row = |delo: &'static str, year: &'static str, child: &'static str| ("РОЖДЕНИЕ", vec![
            (1, "1"), (2, "ГА"), (3, "56"), (5, delo), (7, "Тестово"), (9, "Никольская"), (11, "1"), (12, "муж"),
            (15, year), (18, year), (19, child), (23, "Петров"), (25, "Кузьма"), (26, "Петрович")]);
        let bytes = file(&[row("7", "1890", "Нил"), row("7", "1891", "Ия"), row("7", "1891", "Иов"), row("15", "1890", "Лука")]);
        let (path, conn) = fresh("december", &seed);
        import(&conn, &[("d.xlsx".into(), bytes)]);
        let text = |sql: &str| -> String { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(text("SELECT group_concat(delo || ':' || year, ' ') FROM (SELECT * FROM mk_case ORDER BY id)"), "15:1890 7:1891",
                   "два дела по реквизитам, год — год большинства записей");
        let in_case = |child: &str| -> String { text(&format!(
            "SELECT c.delo FROM person_mention p JOIN entry e ON e.id = p.entry_id JOIN mk_case c ON c.id = e.case_id \
              WHERE p.role_code = 'child' AND p.first_name = '{child}'")) };
        assert_eq!((in_case("Нил"), in_case("Лука")), ("7".to_string(), "15".to_string()), "декабрь 1890 — в деле 7, а не в деле своего года");
        // Правка «Нила» без смены года — дело 7 остаётся.
        let id: i64 = conn.query_row("SELECT entry_id FROM person_mention WHERE first_name = 'Нил'", [], |r| r.get(0)).unwrap();
        let e: (Option<i64>, Option<i64>) = conn.query_row("SELECT event_year, rite_year FROM entry WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        let entry = EntryInput {
            id: Some(id), case_id: 1, section: 1, page: Some("5".into()), no_male: Some(1), no_female: None,
            event_day: None, event_month: None, event_year: e.0, rite_day: None, rite_month: None, rite_year: e.1,
            note: None, uncertain: None, scan_file: None,
            persons: vec![fam_child(&conn)],
        };
        crate::records::save_entry(&conn, &entry).unwrap();
        assert_eq!(in_case("Нил"), "7", "правка без смены года дело не меняет");
        drop(conn);
        let _ = std::fs::remove_file(path);
    }

    fn fam_child(conn: &Connection) -> PersonInput {
        let mut fam = Familio { imp: Importer { conn, unknown: BTreeMap::new(), unknown_total: 0, unknown_patr: BTreeMap::new(),
                                                unknown_patr_total: 0, pending: Vec::new() }, is_name: HashMap::new(), orphans: Vec::new() };
        fam.person("child", 10, Names { first: Some("Нил".into()), ..Default::default() }, Some("М".into()), None, None, None).unwrap().unwrap()
    }

    /// Фикстуры сквозной проверки (scripts/e2e/windows.py, шаг 16а): наша
    /// выгрузка Familio синтетического индексатора — весь приход и один 1890
    /// год. Пишет файлы в db/fixtures: `cargo test familio_fixtures -- --ignored`.
    #[test]
    #[ignore]
    fn familio_fixtures() {
        let seed = seed().expect("resources/seed.sqlite");
        let (path, a) = fresh("fixture", &seed);
        a.execute_batch("BEGIN").unwrap();
        crate::import::import_into(&a, include_bytes!("../../../db/fixtures/indexer.xlsx")).unwrap();
        a.execute_batch("COMMIT").unwrap();
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../db/fixtures");
        let (all, _) = crate::export::familio_bytes(&a, &[], &about()).unwrap();
        let (one, _) = crate::export::familio_bytes(&a, &[1890], &about()).unwrap();
        std::fs::write(dir.join("familio.xlsx"), &all).unwrap();
        std::fs::write(dir.join("familio-1890.xlsx"), &one).unwrap();
        let (pb, b) = fresh("fixture-b", &seed);
        let rep = import(&b, &[("familio.xlsx".into(), all), ("familio-1890.xlsx".into(), one)]);
        println!("{rep:?}");
        println!("дублей на сверке: {}", b.query_row("SELECT count(*) FROM review_item WHERE kind = 'duplicate'", [], |r| r.get::<_, i64>(0)).unwrap());
        drop((a, b));
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(pb);
    }

    /// Шесть образцов Романа (19.10.2026) — тест разработчика, файлы вне
    /// репозитория: GENMETRIC_FAMILIO_DIR=…/_reference/Familio_образцы_20261019.
    #[test]
    #[ignore]
    fn samples() {
        let seed = seed().expect("resources/seed.sqlite");
        let dir = std::env::var("GENMETRIC_FAMILIO_DIR").expect("GENMETRIC_FAMILIO_DIR");
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path())
            .filter(|p| p.extension().map(|e| e == "xlsx").unwrap_or(false)).collect();
        files.sort();
        for f in files {
            let name = f.file_name().unwrap().to_string_lossy().to_string();
            let bytes = std::fs::read(&f).unwrap();
            let t = std::time::Instant::now();
            let seen = inspect(&name, &bytes).unwrap();
            println!("\n##### {name}: {seen:?} ({:?})", t.elapsed());
            let (path, conn) = fresh("sample", &seed);
            let t = std::time::Instant::now();
            let rep = import(&conn, &[(name.clone(), bytes)]);
            println!("импорт {:?}: рождений {}, браков {}, смертей {}, персон {}, пунктов {}, пустых строк {}, годы {:?}",
                     t.elapsed(), rep.births, rep.marriages, rep.deaths, rep.persons, rep.places, rep.empty_rows, rep.years);
            println!("имена не сверены: {} {:?}", rep.unknown_names, rep.unknown_examples);
            println!("отчества не сверены: {} {:?}", rep.unknown_patronymics, rep.unknown_patronymic_examples);
            println!("оговорки ({}): {:#?}", rep.notes.len(), rep.notes.iter().take(12).collect::<Vec<_>>());
            assert_eq!((rep.births, rep.marriages, rep.deaths), (seen.births, seen.marriages, seen.deaths));
            let show = |sql: &str| {
                let mut st = conn.prepare(sql).unwrap();
                let rows: Vec<String> = st.query_map([], |r| r.get::<_, String>(0)).unwrap().map(|r| r.unwrap()).collect();
                for r in rows { println!("  {r}"); }
            };
            show("SELECT p.role_code || ': ' || coalesce(p.rank, '') || ' | ' || coalesce(p.first_name, '') || ' / ' || coalesce(p.patronymic, '')
                         || ' / ' || coalesce(p.surname, '') || ' | ' || coalesce(p.first_name_modern, '') || ' ' || coalesce(p.patronymic_modern, '')
                         || ' | ' || coalesce(pl.name, '') || ' | ' || coalesce(p.note, '')
                    FROM person_mention p LEFT JOIN place pl ON pl.id = p.place_id
                   WHERE p.entry_id IN (SELECT min(id) FROM entry GROUP BY section) ORDER BY p.entry_id, p.sort_order");
            show("SELECT 'дело ' || year || ': ' || coalesce(archive, '') || ' ф.' || coalesce(fond, '') || ' оп.' || coalesce(opis, '')
                         || ' д.' || coalesce(delo, '') || ' | ' || coalesce(church, '') || ' | ' || coalesce(village, '') || ' | '
                         || coalesce(uyezd, '') || ' | ' || coalesce(guberniya, '') || ' | ' || coalesce(indexer, '')
                    FROM mk_case ORDER BY year LIMIT 3");
            drop(conn);
            let _ = std::fs::remove_file(path);
        }
    }
}
