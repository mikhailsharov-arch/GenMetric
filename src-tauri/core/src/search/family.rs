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

use std::collections::{BTreeSet, HashMap};

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

/// Отцом, матерью, восприемником, поручителем человек не моложе этого.
const ADULT_AGE: i64 = 12;
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

/// Возраст взрослого не противоречит рождению в году `born` (п. 1.3).
fn fits_age(rows: &[Row], mine: &[usize], born: i64) -> bool {
    mine.iter().all(|&i| {
        let r = &rows[i];
        let Some(year) = r.year else { return true };
        let dead = r.role == "deceased";
        if year < born + if dead { 0 } else { ADULT_AGE } {
            return false;
        }
        match r.age_years {
            Some(age) if matches!(r.role.as_str(), "groom" | "bride" | "deceased") => (year - age - born).abs() <= AGE_SLACK,
            _ => true,
        }
    })
}

/// Фамилии сходятся: 2 — одна и та же, 1 — её нет хотя бы у одной стороны,
/// 0 — разные. «Обеих нет» — не довод: у крестьян фамилии редки (18 % отцов).
fn surname_score(a: &str, b: &str) -> u8 {
    if !a.is_empty() && a == b { 2 } else if a.is_empty() || b.is_empty() { 1 } else { 0 }
}

/// Персоны прихода: взрослые — по ИОФ и НП, как до 17.10.2026; дети и умершие
/// младенцы — каждый своим досье; затем связи п. 1.2 и 1.3.
pub(super) fn build(rows: &mut [Row]) -> Vec<Person> {
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
        let p = new_person(&mut persons, format!("{}#{}", r.key, r.id), r.place.clone(), vec![i], Some(i));
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
            let p = new_person(&mut persons, format!("{}#{}", dr.key, dr.id), dr.place.clone(), vec![d], None);
            owner[d] = p;
            maybe.extend(cands.into_iter().map(|b| (p, b)));
        }
    }

    // Взрослые — по ИОФ и НП.
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
    for (_, &g) in groups.iter() {
        let first = &rows[persons[g].rows[0]];
        if first.patr_n.is_empty() || first.place_norm.is_empty() || kind_of(&first.role) == Kind::Clergy || !named(first) {
            continue;
        }
        let Some(list) = by_io.get(&(first.first_n.clone(), first.patr_n.clone(), first.place_norm.clone())) else { continue };
        let sur = persons[g].rows.iter().map(|&i| rows[i].sur_base.as_str()).find(|s| !s.is_empty()).unwrap_or("");
        for &b in list {
            let br = &rows[persons[b].rows[0]];
            let score = surname_score(sur, &br.sur_base);
            if score > 0 && br.year.is_some_and(|by| fits_age(rows, &persons[g].rows, by)) {
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
        if for_group.contains(&n) && for_birth.contains(&n) {
            let moved = std::mem::take(&mut persons[b].rows);
            for &i in &moved {
                owner[i] = g;
            }
            persons[g].rows.extend(moved);
            persons[g].rows.sort_by_key(|&i| (rows[i].entry_id, i));
            persons[g].birth = persons[b].birth;
            persons[b].merged_into = Some(g);
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
    persons
}
