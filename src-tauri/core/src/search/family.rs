//! Семья целиком, часть 1 (спека 2026-10-15-semya-celikom): кто в памяти
//! поиска один и тот же человек.
//!
//! Роман 12.10.2026 о том, как он сам узнаёт человека: опорная точка —
//! рождение (имя, родители, НП, дата), а возраст в браке и смерти «может
//! „гулять“ (плюс-минус несколько лет — это норма)». Здесь то же самое:
//!
//! - ребёнок из записи о рождении получает полное ИОФ — отчество по имени
//!   отца, фамилию отца (п. 1.1);
//! - умерший без отчества с родственником «отец» или «мать» — то же по
//!   родственнику, и его смерть прикрепляется к рождению (п. 1.2);
//! - взрослый с тем же ИОФ и НП, если возраст не противоречит, получает это
//!   рождение (п. 1.3).
//!
//! Связь берётся только однозначная; несколько кандидатов — «возможно, тот
//! же человек» у обеих сторон, без слияния. Ничего не пишется в базу: всё
//! считается заново при каждой перестройке памяти поиска.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::{kind_of, Kind, Row};
use crate::records::{surname_pair, NO_NAME};
use crate::text::{normalize, normalize_name};

/// Отчества по имени отца: нормализованное имя → (мужское, женское).
pub(super) type Patronymics = HashMap<String, (String, String)>;

/// Персона поиска: одно досье.
pub(super) struct Person {
    /// С этим ключом и НП спрашивают досье. У взрослого — ИОФ в современном
    /// написании, как до 17.10.2026; у ребёнка и умершего младенца — с номером
    /// записи: тёзки-братья одной семьи — разные люди.
    pub key: String,
    pub place: String,
    /// Упоминания — индексы строк памяти, по порядку записей.
    pub rows: Vec<usize>,
    /// Строка ребёнка в записи о его собственном рождении.
    pub birth: Option<usize>,
    /// Год смерти, если она есть и после неё событий нет.
    pub died: Option<i64>,
    /// «Возможно, тот же человек» — индексы других персон.
    pub possible: BTreeSet<usize>,
    /// Персона влилась в другую (рождение — во взрослого): её ключ ведёт туда.
    pub merged_into: Option<usize>,
}

/// Допуск по возрасту — его ответ 3 от 16.10.2026: «± 5 лет подходит».
const AGE_SLACK: i64 = 5;
/// Смерть младенца — не позже чем через столько лет после рождения (замер на
/// его базе: 82 % смертей с отцом находят рождение в этих пределах).
const INFANT_SPAN: i64 = 5;

fn named(row: &Row) -> bool {
    !row.first.is_empty() && row.first != NO_NAME && !row.first.starts_with(NO_NAME)
}

/// «Иван Семенович» → ключ «иван семенович»: им сверяют родителя ребёнка с
/// родственником умершего.
fn io_key(row: &Row) -> String {
    format!("{} {}", row.first_n, row.patr_n)
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// Фамилия отца в форме пола ребёнка: «Кулаевъ» → «Кулаева» у дочери.
pub(super) fn gendered(surname: &str, female: bool) -> String {
    let (m, f) = surname_pair(surname);
    capital(if female { &f } else { &m })
}

/// Отчество по имени отца из словаря; нет в словаре — нет отчества.
pub(super) fn patronymic(dict: &Patronymics, father_first: &str, female: bool) -> Option<String> {
    dict.get(&normalize_name(father_first)).map(|(m, f)| if female { f.clone() } else { m.clone() }).filter(|s| !s.trim().is_empty())
}

/// Полное ИОФ строки: дописывает отчество и фамилию, ставит НП, если своего нет.
fn complete(row: &mut Row, patr: Option<String>, surname: Option<String>, place: &str) {
    let first = if row.first_modern.is_empty() { row.first.clone() } else { row.first_modern.clone() };
    let own_surname = !row.surname.is_empty();
    let surname = if own_surname { Some(row.surname.clone()) } else { surname };
    let words: Vec<String> = [Some(first), patr.clone(), surname.clone()].into_iter().flatten().filter(|s| !s.is_empty()).collect();
    if let Some(p) = &patr {
        row.patr_n = normalize_name(p);
        row.words.push(row.patr_n.clone());
    }
    if let Some(s) = &surname {
        row.sur_base = surname_pair(s).0;
        if !own_surname {
            row.words.push(normalize_name(s));
        }
    }
    row.key = words.iter().map(|w| normalize_name(w)).collect::<Vec<_>>().join(" ");
    row.full = Some(words.join(" "));
    if row.place.is_empty() && !place.is_empty() {
        row.place = place.to_string();
        row.place_norm = normalize(place);
    }
}

/// Полное ИОФ ребёнка и умершего младенца (п. 1.1 и 1.2). Строки идут
/// записями подряд (`search_mentions` упорядочен по записи).
pub(super) fn derive(rows: &mut [Row], dict: &Patronymics) {
    let mut start = 0;
    while start < rows.len() {
        let id = rows[start].entry_id;
        let mut end = start;
        while end < rows.len() && rows[end].entry_id == id {
            end += 1;
        }
        let find = |rows: &[Row], pred: &dyn Fn(&Row) -> bool| (start..end).find(|&i| pred(&rows[i]));
        let father = find(rows, &|r| r.role == "father" && named(r));
        let mother = find(rows, &|r| r.role == "mother" && named(r));
        // Родственник умершего «отец» или «мать» — по записанному родству.
        let kin = |rows: &[Row], word: &str| find(rows, &|r| {
            r.role.starts_with("deceased_") && named(r)
                && normalize(r.kinship.as_deref().unwrap_or("")).starts_with(word)
        });
        let rel_father = kin(rows, "отец");
        let rel_mother = kin(rows, "мать");
        for i in start..end {
            if !named(&rows[i]) {
                continue;
            }
            let female = rows[i].gender.as_deref() == Some("Ж");
            let (f, m) = match rows[i].role.as_str() {
                "child" => (father, mother),
                "deceased" if rows[i].patr_n.is_empty() && (rel_father.is_some() || rel_mother.is_some()) => (rel_father, rel_mother),
                _ => continue,
            };
            let patr = f.and_then(|f| patronymic(dict, &rows[f].first_n, female));
            let surname = f.map(|f| rows[f].surname.clone()).filter(|s| !s.is_empty()).map(|s| gendered(&s, female));
            let place = f.map(|f| rows[f].place.clone()).filter(|p| !p.is_empty())
                .or_else(|| m.map(|m| rows[m].place.clone())).unwrap_or_default();
            let (fio, mio) = (f.map(|f| io_key(&rows[f])).unwrap_or_default(), m.map(|m| io_key(&rows[m])).unwrap_or_default());
            let row = &mut rows[i];
            complete(row, patr, surname, &place);
            row.father_io = fio;
            row.mother_io = mio;
        }
        start = end;
    }
}

/// Родитель тот же: по отцу, если он есть у обеих записей, иначе по матери.
fn same_parent(a: &Row, b: &Row) -> bool {
    // Фамилии записаны у обоих и разные — разные семьи, даже если отцы
    // тёзки по имени и отчеству (ревьюер 17.10.2026: ложное слияние хуже
    // пропущенного).
    if !a.sur_base.is_empty() && !b.sur_base.is_empty() && a.sur_base != b.sur_base {
        return false;
    }
    if !a.father_io.is_empty() && !b.father_io.is_empty() {
        a.father_io == b.father_io
    } else {
        !a.mother_io.is_empty() && a.mother_io == b.mother_io
    }
}

/// Диапазон возможных годов рождения по упоминанию (спека
/// 2026-10-19-oshibki-svyazej-i-dovodka, Ж1.2). Роман 19.10.2026: «Если
/// диапазоны возможных годов рождения из разных записей пересекаются — это
/// может быть один человек. Если не пересекаются — это однозначно разные
/// люди». Возрасты ролей — индексатора А.К.: родители 17–55, восприемники
/// 6–60, поручители 17–60. None — упоминание ничего не говорит (год не
/// записан, причт, умерший без возраста).
pub(super) fn born_range(r: &Row) -> Option<(i64, i64)> {
    let y = r.year?;
    let ages = |lo: i64, hi: i64| Some((y - hi, y - lo));
    match r.role.trim_end_matches(|c: char| c.is_ascii_digit()) {
        "child" => Some((y, y)),
        "father" | "mother" => ages(17, 55),
        "godparent" => ages(6, 60),
        "witness" => ages(17, 60),
        "groom" | "bride" => match r.age_years {
            Some(a) if a > 0 => ages(a - AGE_SLACK, a + AGE_SLACK),
            _ => ages(15, 70),
        },
        "deceased" => match r.age_years {
            Some(a) if a > 0 && a <= 10 => ages(a - 1, a + 1),
            Some(a) if a > 10 => ages(a - AGE_SLACK, a + AGE_SLACK),
            _ if r.age_small => ages(0, 1),
            _ => None,
        },
        role if role.starts_with("clergy") => None,
        _ => ages(6, 100),
    }
}

/// Пересечение двух диапазонов; None у одного — берётся другой.
fn meet(a: Option<(i64, i64)>, b: Option<(i64, i64)>) -> Result<Option<(i64, i64)>, ()> {
    match (a, b) {
        (Some((a0, a1)), Some((b0, b1))) => {
            let (lo, hi) = (a0.max(b0), a1.min(b1));
            if lo <= hi { Ok(Some((lo, hi))) } else { Err(()) }
        }
        (x, None) | (None, x) => Ok(x),
    }
}

/// Общий диапазон упоминаний; Err — не сходятся.
fn range_of(rows: &[Row], mine: &[usize]) -> Result<Option<(i64, i64)>, ()> {
    mine.iter().try_fold(None, |acc, &i| meet(acc, born_range(&rows[i])))
}

/// Ключ досье, который не меняется при пересохранении записи: номер
/// упоминания меняется (запись переписывает людей заново), а номер записи и
/// место человека в ней — нет (долг п. 74).
fn stable(r: &Row) -> String {
    format!("{}.{}", r.entry_id, r.sort_order)
}

/// Персона с диапазонами, которые не сходятся, — несколько людей (Ж1.2):
/// умершая 2,5 лет и восприемница-тёзка того же пункта. Части — по порядку
/// лет; «возможно» между ними не ставится: это однозначно разные люди.
fn split_by_age(persons: &mut Vec<Person>, owner: &mut [usize], rows: &[Row], first_adult: usize, maybe: &mut Vec<(usize, usize)>) {
    for p in first_adult..persons.len() {
        if persons[p].rows.is_empty() || kind_of(&rows[persons[p].rows[0]].role) == Kind::Clergy || range_of(rows, &persons[p].rows).is_ok() {
            continue;
        }
        let mut mine = persons[p].rows.clone();
        mine.sort_by_key(|&i| (rows[i].year.unwrap_or(i64::MAX), i));
        // Главная часть — год, который покрывает больше всего диапазонов: одна
        // ошибка в возрасте не должна отрезать у человека половину жизни
        // (ревьюер 19.10.2026: жених «50» вместо «20»).
        let ranges: Vec<(i64, i64)> = mine.iter().filter_map(|&i| born_range(&rows[i])).collect();
        // Конец диапазона (−1) раньше начала (+1) в том же году.
        let mut ends: Vec<(i64, i32)> = ranges.iter().flat_map(|&(lo, hi)| [(lo, 1), (hi + 1, -1)]).collect();
        ends.sort();
        let (mut depth, mut best, mut point) = (0, 0, i64::MIN);
        for (at, step) in ends {
            depth += step;
            if depth > best {
                best = depth;
                point = at;
            }
        }
        let mut main: (Option<(i64, i64)>, Vec<usize>) = (None, Vec::new());
        let mut rest: Vec<usize> = Vec::new();
        for &i in &mine {
            match born_range(&rows[i]) {
                Some((lo, hi)) if lo > point || hi < point => rest.push(i),
                r => {
                    main.0 = meet(main.0, r).unwrap_or(main.0);
                    main.1.push(i);
                }
            }
        }
        let mut parts: Vec<(Option<(i64, i64)>, Vec<usize>)> = vec![main];
        for &i in &rest {
            let r = born_range(&rows[i]);
            match parts.iter_mut().find(|(range, _)| meet(*range, r).is_ok()) {
                Some((range, list)) => {
                    *range = meet(*range, r).unwrap_or(*range);
                    list.push(i);
                }
                None => parts.push((r, vec![i])),
            }
        }
        let key = persons[p].key.clone();
        let place = persons[p].place.clone();
        persons[p].rows = parts[0].1.clone();
        persons[p].rows.sort_by_key(|&i| (rows[i].entry_id, i));
        for (_, mut list) in parts.into_iter().skip(1) {
            list.sort_by_key(|&i| (rows[i].entry_id, i));
            let tag = stable(&rows[list[0]]);
            persons.push(Person { key: format!("{key}~{tag}"), place: place.clone(), rows: list.clone(), birth: None, died: None,
                                  possible: BTreeSet::new(), merged_into: None });
            let n = persons.len() - 1;
            for &i in &list {
                owner[i] = n;
            }
            // Отрезана одна запись со взрослым возрастом — может быть и ошибкой
            // в возрасте («50» вместо «20»): «возможно». Ребёнок и умерший
            // младенец (до 10 лет) — однозначно другой человек (Роман 19.10.2026).
            let r = &rows[list[0]];
            let infant = r.role == "child" || (r.role == "deceased" && (r.age_small || r.age_years.is_some_and(|a| a <= 10)));
            if list.len() == 1 && !infant {
                maybe.push((p, n));
            }
        }
    }
}

/// Фамилии сходятся: 2 — одна и та же, 1 — её нет хотя бы у одной стороны,
/// 0 — разные. «Обеих нет» — не довод: у крестьян фамилии редки (18 % отцов).
fn surname_score(a: &str, b: &str) -> u8 {
    if !a.is_empty() && a == b { 2 } else if a.is_empty() || b.is_empty() { 1 } else { 0 }
}

/// Женщина: мать, невеста, умершая или записанная с полом «Ж».
fn female(p: &Person, rows: &[Row]) -> bool {
    p.rows.iter().any(|&i| matches!(rows[i].role.as_str(), "mother" | "bride") || rows[i].gender.as_deref() == Some("Ж"))
}

/// Переложить упоминания персоны `from` в `to`; `from` ведёт к `to`.
fn absorb(persons: &mut [Person], owner: &mut [usize], rows: &[Row], to: usize, from: usize) {
    let moved = std::mem::take(&mut persons[from].rows);
    for &i in &moved {
        owner[i] = to;
    }
    persons[to].rows.extend(moved);
    persons[to].rows.sort_by_key(|&i| (rows[i].entry_id, i));
    persons[from].merged_into = Some(to);
}

/// Супруг упоминания (п. 2.1): отец ↔ мать, жених ↔ невеста, умерший ↔
/// родственник «жена», «муж», «супруг».
fn spouses(rows: &[Row]) -> Vec<Option<usize>> {
    let mut out = vec![None; rows.len()];
    let mut start = 0;
    while start < rows.len() {
        let mut end = start;
        while end < rows.len() && rows[end].entry_id == rows[start].entry_id {
            end += 1;
        }
        let find = |pred: &dyn Fn(&Row) -> bool| (start..end).find(|&i| named(&rows[i]) && pred(&rows[i]));
        let pairs = [
            (find(&|r| r.role == "father"), find(&|r| r.role == "mother")),
            (find(&|r| r.role == "groom"), find(&|r| r.role == "bride")),
            (find(&|r| r.role == "deceased"), find(&|r| {
                r.role.starts_with("deceased_") && {
                    let k = normalize(r.kinship.as_deref().unwrap_or(""));
                    r.role.starts_with("deceased_spouse") || k.starts_with("жена") || k.starts_with("муж") || k.starts_with("супруг")
                }
            })),
        ];
        for (a, b) in pairs {
            if let (Some(a), Some(b)) = (a, b) {
                out[a] = Some(b);
                out[b] = Some(a);
            }
        }
        start = end;
    }
    out
}

/// Один и тот же человек по имени: имя то же, отчество и фамилия — те же
/// или их нет у одного. Женщине фамилию не сверяем: в браке она меняется.
fn same_human(a: &Row, b: &Row) -> bool {
    let woman = |r: &Row| matches!(r.role.as_str(), "mother" | "bride") || r.gender.as_deref() == Some("Ж");
    let fits = |x: &str, y: &str| x == y || x.is_empty() || y.is_empty();
    !a.first_n.is_empty() && a.first_n == b.first_n && fits(&a.patr_n, &b.patr_n)
        && (woman(a) || woman(b) || fits(&a.sur_base, &b.sur_base))
}

/// Тот же супруг для разделения тёзок (п. 2.1): то же имя (и фамилия мужа,
/// если она есть у обоих). Отчество жены не сверяем: на его базе у одного
/// мужа жена то «Дарья Григорьевна», то «Дарья Васильевна» вперемешку по
/// годам — это описка, а не две жены; делить такого человека — терять его
/// досье (замер 17.10.2026).
///
/// Имя сравнивается по корню вариантов словаря («Васса» и «Василиса»,
/// «Андреян» и «Адриан» — одно, `first_c`); то же отчество при разных именах
/// («Евфросиния» и «Ефросиния Ивановна», «Мария» и «Марфа Ксенофонтова») —
/// тоже один человек: на его базе это варианты и описки, а не вторая жена
/// (ревьюер 17.10.2026: половина разделений была ложной).
fn same_spouse(a: &Row, b: &Row) -> bool {
    let woman = |r: &Row| matches!(r.role.as_str(), "mother" | "bride") || r.gender.as_deref() == Some("Ж");
    let fits = |x: &str, y: &str| x == y || x.is_empty() || y.is_empty();
    let name = (!a.first_c.is_empty() && a.first_c == b.first_c) || (!a.patr_n.is_empty() && a.patr_n == b.patr_n);
    name && (woman(a) || woman(b) || fits(&a.sur_base, &b.sur_base))
}

/// Похожие имена — варианты написания одного: общее начало из четырёх и
/// более букв или не больше двух правок («соломония» — «соломонида»,
/// «евфросиния» — «ефросиния»).
fn alike(a: &str, b: &str) -> bool {
    let (x, y): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if x.iter().zip(&y).take_while(|(p, q)| p == q).count() >= 4 {
        return true;
    }
    let mut prev: Vec<usize> = (0..=y.len()).collect();
    for (i, cx) in x.iter().enumerate() {
        let mut cur = vec![i + 1; y.len() + 1];
        for (j, cy) in y.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(cx != cy)).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    // Короткие частые имена (Мария/Дарья, Анна/Инна) — не больше одной правки.
    prev[y.len()] <= if x.len().min(y.len()) < 6 { 1 } else { 2 }
}

/// Своя смерть одной раньше событий другой — это разные люди.
fn died_before(rows: &[Row], a: &[usize], b: &[usize]) -> bool {
    let death = |m: &[usize]| m.iter().filter(|&&i| rows[i].role == "deceased").filter_map(|&i| rows[i].year).min();
    let last = |m: &[usize]| m.iter().filter(|&&i| rows[i].role != "deceased").filter_map(|&i| rows[i].year).max();
    death(a).zip(last(b)).is_some_and(|(d, l)| d < l) || death(b).zip(last(a)).is_some_and(|(d, l)| d < l)
}

/// Второй брак — вдовый: «вторым браком», «третьим…», «вдовец», «вдова».
fn remarried(r: &Row) -> bool {
    let text = normalize(&format!("{} {}", r.marriage_order.as_deref().unwrap_or(""), r.rank.as_deref().unwrap_or("")));
    ["втор", "трет", "четв", "вдов"].iter().any(|w| text.contains(w))
}

/// Тёзки с разными супругами — разные досье (п. 2.1). Части с вдовым вторым
/// браком (или смертью прежнего супруга до нового) — один человек.
fn split_by_spouse(persons: &mut Vec<Person>, owner: &mut [usize], rows: &[Row], spouse: &[Option<usize>],
                   first_adult: usize, maybe: &mut Vec<(usize, usize)>) {
    let mut deaths: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut by_entry: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, r) in rows.iter().enumerate() {
        if r.role == "deceased" && named(r) {
            deaths.entry(r.first_n.as_str()).or_default().push(i);
        }
        by_entry.entry(r.entry_id).or_default().push(i);
    }
    // Смерть своего ребёнка — к той семье, где он родился: родственник
    // умершего «отец» («мать») → умерший → его рождение (п. 1.2) → отец
    // (мать) в записи о рождении. Иначе смерть осталась бы в досье «без
    // супруга», отдельно от семьи (Иван Семенович, Костино, 19.10.2026).
    let anchor = |persons: &Vec<Person>, owner: &[usize], i: usize| -> Option<usize> {
        let r = &rows[i];
        let kin = normalize(r.kinship.as_deref().unwrap_or(""));
        let want = if kin.starts_with("отец") { "father" } else if kin.starts_with("мать") { "mother" } else { return None };
        if !r.role.starts_with("deceased_") {
            return None;
        }
        let dead = *by_entry.get(&r.entry_id)?.iter().find(|&&d| rows[d].role == "deceased")?;
        let born = persons.get(owner[dead])?.birth?;
        by_entry.get(&rows[born].entry_id)?.iter().copied().find(|&f| rows[f].role == want)
    };
    for p in first_adult..persons.len() {
        let first = &rows[persons[p].rows[0]];
        if kind_of(&first.role) == Kind::Clergy {
            continue;
        }
        // Части по супругам, по годам: представитель — самый полный супруг.
        struct Part { rep: usize, rows: Vec<usize>, from: i64, to: i64 }
        let mut parts: Vec<Part> = Vec::new();
        let mut lone: Vec<usize> = Vec::new();
        let mut mine = persons[p].rows.clone();
        mine.sort_by_key(|&i| (rows[i].year.unwrap_or(i64::MAX), i));
        for &i in &mine {
            let Some(s) = spouse[i] else { lone.push(i); continue };
            // Упоминание без года склейке не мешает (ревьюер 19.10.2026).
            let y = rows[i].year;
            match parts.iter_mut().find(|part| same_spouse(&rows[part.rep], &rows[s])) {
                Some(part) => {
                    part.rows.push(i);
                    if let Some(y) = y {
                        part.from = part.from.min(y);
                        part.to = part.to.max(y);
                    }
                    if rows[part.rep].patr_n.is_empty() && !rows[s].patr_n.is_empty() {
                        part.rep = s;
                    }
                }
                None => parts.push(Part { rep: s, rows: vec![i], from: y.unwrap_or(i64::MAX), to: y.unwrap_or(i64::MIN) }),
            }
        }
        if parts.len() < 2 {
            continue;
        }
        // Склеить подряд идущие части, если брак вдовый.
        let mut segments: Vec<Vec<usize>> = vec![parts[0].rows.clone()];
        let mut seg_to = parts[0].to;
        for k in 1..parts.len() {
            let (prev, cur) = (&parts[k - 1], &parts[k]);
            let marker = cur.rows.iter().any(|&i| matches!(rows[i].role.as_str(), "groom" | "bride") && remarried(&rows[i]));
            let gone = deaths.get(rows[prev.rep].first_n.as_str()).into_iter().flatten().any(|&d| {
                let dr = &rows[d];
                same_human(&rows[prev.rep], dr)
                    && (rows[prev.rep].place_norm.is_empty() || dr.place_norm == rows[prev.rep].place_norm)
                    && dr.year.is_some_and(|y| y >= prev.to.min(cur.from) && y <= cur.from)
            });
            // Вдовий брак склеивает, только если прежняя семья кончилась до
            // новой: у Ивана Семеновича из Костино брак 1888 года «Третьим
            // браком», а дети от прежней жены рождались и после — это двое
            // мужчин (Роман 19.10.2026: «Православный крестьянин не мог быть
            // женат одновременно на двух женщинах»).
            if (marker || gone) && seg_to <= cur.from {
                segments.last_mut().unwrap().extend(cur.rows.iter().copied());
                seg_to = seg_to.max(cur.to);
            } else {
                segments.push(cur.rows.clone());
                seg_to = cur.to;
            }
        }
        if segments.len() < 2 {
            continue;
        }
        // Смерти своих детей — к семье, где ребёнок родился.
        let mut segments = segments;
        lone.retain(|&i| {
            let Some(parent) = anchor(persons, owner, i) else { return true };
            match segments.iter_mut().find(|seg| seg.contains(&parent)) {
                Some(seg) => { seg.push(i); false }
                None => true,
            }
        });
        // Упоминания без супруга остаются у прежней персоны; части — новыми.
        // Без них прежней персоной становится первая часть.
        let key = persons[p].key.clone();
        let place = persons[p].place.clone();
        let mut made: Vec<usize> = Vec::new();
        let start = if lone.is_empty() { 1 } else { 0 };
        persons[p].rows = if lone.is_empty() { segments[0].clone() } else { lone.clone() };
        persons[p].rows.sort_by_key(|&i| (rows[i].entry_id, i));
        for seg in &segments[start..] {
            let mut seg = seg.clone();
            seg.sort_by_key(|&i| (rows[i].entry_id, i));
            let tag = stable(&rows[seg[0]]);
            persons.push(Person { key: format!("{key}~{tag}"), place: place.clone(), rows: seg.clone(), birth: None, died: None,
                                  possible: BTreeSet::new(), merged_into: None });
            let n = persons.len() - 1;
            for &i in &seg {
                owner[i] = n;
            }
            made.push(n);
        }
        let all: Vec<usize> = std::iter::once(p).chain(made).collect();
        for (x, &a) in all.iter().enumerate() {
            for &b in &all[x + 1..] {
                maybe.push((a, b));
            }
        }
    }
}

/// С фамилией и без (п. 2.2) и женщина после замужества (п. 2.3): кого
/// слить — через общий супруг.
fn unite(persons: &mut Vec<Person>, owner: &mut [usize], rows: &[Row], spouse: &[Option<usize>],
         first_adult: usize, maybe: &mut Vec<(usize, usize)>) {
    let mut parent: Vec<usize> = (0..persons.len()).collect();
    fn root(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    // Диапазон года рождения у каждого корня: несовместимых не сливаем
    // (Ж1.2), и цепочка слияний это помнит.
    let mut ranges: Vec<Option<(i64, i64)>> = persons.iter().map(|p| range_of(rows, &p.rows).ok().flatten()).collect();
    let join = |parent: &mut [usize], ranges: &mut [Option<(i64, i64)>], a: usize, b: usize| -> bool {
        let (a, b) = (root(parent, a), root(parent, b));
        if a == b {
            return true;
        }
        let Ok(both) = meet(ranges[a], ranges[b]) else { return false };
        let (lo, hi) = (a.min(b), a.max(b));
        parent[hi] = lo;
        ranges[lo] = both;
        true
    };
    let live: Vec<usize> = (first_adult..persons.len())
        .filter(|&p| !persons[p].rows.is_empty() && kind_of(&rows[persons[p].rows[0]].role) != Kind::Clergy && named(&rows[persons[p].rows[0]]))
        .collect();
    let surname = |p: usize| persons[p].rows.iter().map(|&i| rows[i].sur_base.as_str()).find(|s| !s.is_empty()).unwrap_or("").to_string();
    let wives = |p: usize| persons[p].rows.iter().filter_map(|&i| spouse[i]).collect::<Vec<usize>>();
    // Те, кого уже развели по супругам, между собой не сливаются.
    let split: BTreeSet<(usize, usize)> = maybe.iter().flat_map(|&(a, b)| [(a, b), (b, a)]).collect();

    // 2.2: те же имя, отчество и пункт, у одного фамилии нет.
    let mut bucket: HashMap<(String, String, String), Vec<usize>> = HashMap::new();
    for &p in &live {
        let r = &rows[persons[p].rows[0]];
        if !r.patr_n.is_empty() {
            bucket.entry((r.first_n.clone(), r.patr_n.clone(), r.place_norm.clone())).or_default().push(p);
        }
    }
    let mut keys: Vec<_> = bucket.keys().cloned().collect();
    keys.sort();
    for k in keys {
        let list = &bucket[&k];
        // Безфамильный сливается, только если подходит ровно один фамильный и
        // жена та же по имени и отчеству: «Мария» — самое частое имя, и
        // слияние по одному имени склеило бы через безфамильного «Кулаева» и
        // «Сидорова» (ревьюер 17.10.2026).
        for &a in list.iter().filter(|&&a| surname(a).is_empty()) {
            let wa = wives(a);
            let mut fit: Vec<usize> = Vec::new();
            for &b in list.iter().filter(|&&b| !surname(b).is_empty()) {
                if split.contains(&(a, b)) {
                    continue;
                }
                let wb = wives(b);
                if wa.is_empty() || wb.is_empty() {
                    if meet(ranges[root(&mut parent, a)], ranges[root(&mut parent, b)]).is_ok() {
                        maybe.push((a, b));
                    }
                } else if wa.iter().any(|&x| wb.iter().any(|&y| same_spouse(&rows[x], &rows[y]) && same_human(&rows[x], &rows[y])))
                    && meet(ranges[root(&mut parent, a)], ranges[root(&mut parent, b)]).is_ok() {
                    fit.push(b);
                }
            }
            match fit[..] {
                [b] => { join(&mut parent, &mut ranges, a, b); }
                _ => maybe.extend(fit.into_iter().map(|b| (a, b))),
            }
        }
    }

    // 2.3: женщина — по мужу. Муж — его персона после 2.1 и 2.2. Имя — по
    // корню вариантов, а то же отчество у того же мужа при разных именах —
    // тоже одна женщина («Соломония» и «Соломонида Яковлевна» у Семена
    // Алексеева, Роман 19.10.2026; в словаре они не связаны).
    let mut by_husband: HashMap<usize, Vec<(usize, String, String)>> = HashMap::new();
    for &p in &live {
        if !female(&persons[p], rows) {
            continue;
        }
        for &i in &persons[p].rows {
            let Some(h) = spouse[i] else { continue };
            if !matches!(rows[i].role.as_str(), "mother" | "bride") {
                continue;
            }
            let husband = root(&mut parent, owner[h]);
            let list = by_husband.entry(husband).or_default();
            if !list.iter().any(|(q, _, _)| *q == p) {
                list.push((p, rows[i].first_c.clone(), rows[i].patr_n.clone()));
            }
        }
    }
    let mut keys: Vec<_> = by_husband.keys().cloned().collect();
    keys.sort();
    for k in keys {
        let list = by_husband[&k].clone();
        for (x, (a, na, pa)) in list.iter().enumerate() {
            for (b, nb, pb) in &list[x + 1..] {
                // То же отчество — только при похожих именах (Соломония и
                // Соломонида, Евфросиния и Ефросиния): у вдовца Степана
                // Тимофеева Наталья и Анна Ивановны — две жены (ревьюер 19.10.2026).
                let same = (na == nb && (pa == pb || pa.is_empty() || pb.is_empty())) || (!pa.is_empty() && pa == pb && alike(na, nb));
                if same && !split.contains(&(*a, *b)) && !died_before(rows, &persons[*a].rows, &persons[*b].rows) {
                    join(&mut parent, &mut ranges, *a, *b);
                }
            }
        }
    }

    // Кто остаётся досье: у кого больше упоминаний матерью (деревня мужа —
    // там она живёт), затем больше упоминаний вообще. До этого досье брало НП
    // невесты, и Ctrl+F из поля матери не находил её (ревьюер 17.10.2026).
    let mut sets: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &p in &live {
        sets.entry(root(&mut parent, p)).or_default().push(p);
    }
    for members in sets.into_values().filter(|m| m.len() > 1) {
        let weight = |p: usize| {
            let mothers = persons[p].rows.iter().filter(|&&i| rows[i].role == "mother").count();
            (mothers, persons[p].rows.len(), std::cmp::Reverse(p))
        };
        let keep = *members.iter().max_by_key(|&&p| weight(p)).unwrap();
        for p in members.into_iter().filter(|&p| p != keep) {
            absorb(persons, owner, rows, keep, p);
        }
    }
}

/// Персоны прихода: взрослые — по ИОФ и НП, как до 17.10.2026; дети и умершие
/// младенцы — каждый своим досье; затем связи п. 1.2 и 1.3.
pub(super) fn build(rows: &mut [Row]) -> (Vec<Person>, Vec<Option<usize>>) {
    let mut persons: Vec<Person> = Vec::new();
    let new_person = |persons: &mut Vec<Person>, key: String, place: String, rows: Vec<usize>, birth: Option<usize>| {
        persons.push(Person { key, place, rows, birth, died: None, possible: BTreeSet::new(), merged_into: None });
        persons.len() - 1
    };
    let mut owner: Vec<usize> = vec![usize::MAX; rows.len()];

    // Рождения: у каждого ребёнка своё досье.
    let mut births: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for i in 0..rows.len() {
        if rows[i].role != "child" {
            continue;
        }
        let r = &rows[i];
        let p = new_person(&mut persons, format!("{}#{}", r.key, stable(r)), r.place.clone(), vec![i], Some(i));
        owner[i] = p;
        if named(r) && r.year.is_some() {
            births.entry((r.first_n.clone(), r.place_norm.clone())).or_default().push(p);
        }
    }

    // Смерти младенцев (п. 1.2): раньше умершие забирают рождение первыми —
    // у второй Марии той же семьи (первая умерла) своё рождение.
    let mut infants: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].role == "deceased" && rows[i].full.is_some()).collect();
    infants.sort_by_key(|&i| (rows[i].year.unwrap_or(i64::MAX), rows[i].entry_id));
    let mut taken: BTreeSet<usize> = BTreeSet::new();
    let mut maybe: Vec<(usize, usize)> = Vec::new();
    for d in infants {
        let dr = &rows[d];
        let mut cands: Vec<usize> = Vec::new();
        if let Some(dy) = dr.year {
            for &b in births.get(&(dr.first_n.clone(), dr.place_norm.clone())).into_iter().flatten() {
                let br = &rows[persons[b].rows[0]];
                let by = br.year.unwrap_or(i64::MAX);
                // Возраст годами больше младенческого (неженатый сын 25 лет с
                // отцом-родственником) — окно по возрасту ± 5 лет.
                let window = match dr.age_years {
                    Some(a) if a > INFANT_SPAN => (dy - a - by).abs() <= AGE_SLACK,
                    _ => dy - by <= INFANT_SPAN,
                };
                if !taken.contains(&b) && by <= dy && window && same_parent(dr, br) {
                    cands.push(b);
                }
            }
            // Возраст в записи сужает выбор, если в нём кто-то остаётся.
            let near: Vec<usize> = cands.iter().copied().filter(|&b| {
                let by = rows[persons[b].rows[0]].year.unwrap_or(0);
                match dr.age_years.filter(|a| *a > 0) {
                    Some(a) => (dy - a - by).abs() <= 1,
                    None => dr.age_small && dy - by <= 1,
                }
            }).collect();
            if !near.is_empty() {
                cands = near;
            }
        }
        if let [b] = cands[..] {
            persons[b].rows.push(d);
            owner[d] = b;
            taken.insert(b);
        } else {
            let p = new_person(&mut persons, format!("{}#{}", dr.key, stable(dr)), dr.place.clone(), vec![d], None);
            owner[d] = p;
            maybe.extend(cands.into_iter().map(|b| (p, b)));
        }
    }

    // Взрослые — по ИОФ и НП.
    let first_adult = persons.len();
    let mut groups: HashMap<(String, String), usize> = HashMap::new();
    for i in 0..rows.len() {
        if owner[i] != usize::MAX {
            continue;
        }
        let k = (rows[i].key.clone(), rows[i].place.clone());
        let p = match groups.get(&k) {
            Some(&p) => p,
            None => {
                let p = new_person(&mut persons, k.0.clone(), k.1.clone(), Vec::new(), None);
                groups.insert(k, p);
                p
            }
        };
        persons[p].rows.push(i);
        owner[i] = p;
    }

    // Часть 2: тёзки с разными супругами (п. 2.1), с фамилией и без (2.2),
    // женщина после замужества (2.3).
    split_by_age(&mut persons, &mut owner, rows, first_adult, &mut maybe);
    let spouse = spouses(rows);
    split_by_spouse(&mut persons, &mut owner, rows, &spouse, first_adult, &mut maybe);
    unite(&mut persons, &mut owner, rows, &spouse, first_adult, &mut maybe);

    // Рождение → взрослый (п. 1.3): только взаимная пара с лучшей фамилией.
    let mut by_io: HashMap<(String, String, String), Vec<usize>> = HashMap::new();
    for (&(_, _), list) in births.iter() {
        for &b in list {
            let r = &rows[persons[b].rows[0]];
            // Умерший младенцем взрослым не станет.
            if persons[b].rows.len() == 1 && !r.patr_n.is_empty() {
                by_io.entry((r.first_n.clone(), r.patr_n.clone(), r.place_norm.clone())).or_default().push(b);
            }
        }
    }
    let mut pairs: Vec<(usize, usize, u8)> = Vec::new();
    // Женщина, известная только матерью, рождение не получает — только
    // «возможно» (п. 2.3, долг п. 69): в деревню мужа она чаще пришла со стороны.
    let mut only_maybe: BTreeSet<usize> = BTreeSet::new();
    for g in first_adult..persons.len() {
        if persons[g].merged_into.is_some() || persons[g].rows.is_empty() {
            continue;
        }
        let mine = &persons[g].rows;
        // Происхождение — по невесте (девичья фамилия, деревня девичества).
        let origin = mine.iter().map(|&i| &rows[i]).find(|r| r.role == "bride").unwrap_or(&rows[mine[0]]);
        if origin.patr_n.is_empty() || origin.place_norm.is_empty() || kind_of(&origin.role) == Kind::Clergy || !named(origin) {
            continue;
        }
        if female(&persons[g], rows) && origin.role != "bride" {
            only_maybe.insert(g);
        }
        let Some(list) = by_io.get(&(origin.first_n.clone(), origin.patr_n.clone(), origin.place_norm.clone())) else { continue };
        let sur = if origin.role == "bride" { origin.sur_base.as_str() }
                  else { mine.iter().map(|&i| rows[i].sur_base.as_str()).find(|s| !s.is_empty()).unwrap_or("") };
        for &b in list {
            let br = &rows[persons[b].rows[0]];
            let score = surname_score(sur, &br.sur_base);
            // Год рождения — в общем диапазоне взрослого (Ж1.2).
            let fits = |by: i64| range_of(rows, mine).is_ok_and(|r| r.is_none_or(|(lo, hi)| lo <= by && by <= hi));
            if score > 0 && br.year.is_some_and(fits) {
                pairs.push((g, b, score));
            }
        }
    }
    let best = |side: &dyn Fn(&(usize, usize, u8)) -> usize| {
        let mut top: HashMap<usize, (u8, Vec<usize>)> = HashMap::new();
        for (n, p) in pairs.iter().enumerate() {
            let e = top.entry(side(p)).or_insert((0, Vec::new()));
            if p.2 > e.0 {
                *e = (p.2, vec![n]);
            } else if p.2 == e.0 {
                e.1.push(n);
            }
        }
        top.into_values().filter_map(|(_, v)| if v.len() == 1 { Some(v[0]) } else { None }).collect::<BTreeSet<usize>>()
    };
    let (for_group, for_birth) = (best(&|p| p.0), best(&|p| p.1));
    for (n, &(g, b, _)) in pairs.iter().enumerate() {
        if for_group.contains(&n) && for_birth.contains(&n) && !only_maybe.contains(&g) {
            absorb(&mut persons, &mut owner, rows, g, b);
            persons[g].birth = persons[b].birth;
        } else {
            maybe.push((g, b));
        }
    }

    // «Возможно» — между тем, что осталось после слияний.
    let resolve = |persons: &[Person], mut p: usize| {
        while let Some(q) = persons[p].merged_into {
            p = q;
        }
        p
    };
    for (a, b) in maybe {
        let (a, b) = (resolve(&persons, a), resolve(&persons, b));
        if a != b {
            persons[a].possible.insert(b);
            persons[b].possible.insert(a);
        }
    }

    // Умер: своя смерть есть, событий позже неё нет (п. 1.6).
    for p in persons.iter_mut() {
        let death = p.rows.iter().filter(|&&i| rows[i].role == "deceased").filter_map(|&i| rows[i].year).max();
        p.died = death.filter(|&d| !p.rows.iter().any(|&i| rows[i].year.is_some_and(|y| y > d)));
    }
    for (i, row) in rows.iter_mut().enumerate() {
        row.person = owner[i];
    }
    (persons, spouse)
}
