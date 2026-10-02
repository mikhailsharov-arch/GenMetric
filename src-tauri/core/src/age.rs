//! Возраст умершего — разбор текста из книги на годы, месяцы, недели, дни.
//!
//! Перенос `src/age.ts` один в один: форма смертей разбирает возраст при
//! сохранении, а импорт из Excel (спека 2026-10-02, п. 4.3) обязан разобрать
//! его так же — иначе «3 мес» из Excel и «3 мес», набранные руками, легли бы в
//! базу по-разному. Примеры в тестах — те же, что в `scripts/test_age.mjs`;
//! правишь одно — правь другое.

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Age {
    pub years: Option<i64>,
    pub months: Option<i64>,
    pub weeks: Option<i64>,
    pub days: Option<i64>,
}

/// Единицы, от длинных к коротким: «месяцев» раньше «мес» и «м».
const UNITS: &[&str] = &[
    "лет", "года", "год", "месяцев", "месяца", "месяц", "мес", "недель", "неделя", "недели", "нед",
    "дней", "день", "дня", "дн", "л", "г", "м", "н", "д",
];

/// 0 — годы, 1 — месяцы, 2 — недели, 3 — дни.
fn rank(unit: &str) -> usize {
    match unit.chars().next() {
        Some('л') | Some('г') => 0,
        Some('м') => 1,
        Some('н') => 2,
        _ => 3,
    }
}

fn is_small_cyrillic(c: char) -> bool {
    ('а'..='я').contains(&c)
}

/// 1–3 цифры с начала; возвращает число и остаток.
fn number(s: &[char]) -> Option<(i64, &[char])> {
    let n = s.iter().take_while(|c| c.is_ascii_digit()).count();
    if n == 0 || n > 3 {
        return None;
    }
    let v: String = s[..n].iter().collect();
    Some((v.parse().ok()?, &s[n..]))
}

fn skip_spaces(s: &[char]) -> &[char] {
    let n = s.iter().take_while(|c| c.is_whitespace()).count();
    &s[n..]
}

fn starts_with(s: &[char], unit: &str) -> bool {
    let u: Vec<char> = unit.chars().collect();
    s.len() >= u.len() && s[..u.len()] == u[..]
}

/// Составной возраст: «1 год 3 мес», «2 г. 6 м.», «1 мес, 2 нед и 3 дня» —
/// каждая часть с единицей, от лет к дням, без повторов и дробей.
fn compound(t: &[char]) -> Option<Age> {
    let mut out = Age::default();
    let mut rest = t;
    let mut last: Option<usize> = None;
    let mut parts = 0;
    while !rest.is_empty() {
        let (value, after) = number(rest)?;
        let after = skip_spaces(after);
        // Единица, после которой не идёт буква: «1 ги» — не «г».
        let unit = UNITS.iter().find(|u| {
            starts_with(after, u)
                && !after.get(u.chars().count()).copied().map(is_small_cyrillic).unwrap_or(false)
        })?;
        let mut after = &after[unit.chars().count()..];
        if after.first() == Some(&'.') {
            after = &after[1..];
        }
        after = skip_spaces(after);
        if after.first() == Some(&',') {
            after = skip_spaces(&after[1..]);
        } else if after.first() == Some(&'и') && after.get(1).map(|c| c.is_whitespace()).unwrap_or(false) {
            after = skip_spaces(&after[1..]);
        }
        let r = rank(unit);
        if last.map(|l| r <= l).unwrap_or(false) {
            return None; // повтор или обратный порядок
        }
        last = Some(r);
        match r {
            0 => out.years = Some(value),
            1 => out.months = Some(value),
            2 => out.weeks = Some(value),
            _ => out.days = Some(value),
        }
        rest = after;
        parts += 1;
    }
    if parts >= 2 { Some(out) } else { None }
}

/// None — текст не разобран (или пуст); иначе заполнены разобранные части.
pub fn parse_age(text: &str) -> Option<Age> {
    // Старая орфография («9 лѣтъ», «3 мѣс.») — к современной.
    let lowered = text.trim().to_lowercase().replace('ё', "е").replace('ѣ', "е");
    let mut t: Vec<char> = Vec::with_capacity(lowered.len());
    let chars: Vec<char> = lowered.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        // «ъ» перед пробелом, точкой или концом строки отбрасывается.
        if *c == 'ъ' && chars.get(i + 1).map(|n| n.is_whitespace() || *n == '.').unwrap_or(true) {
            continue;
        }
        t.push(*c);
    }
    if t.is_empty() {
        return None;
    }
    single(&t).or_else(|| compound(&t))
}

/// Одна часть: «5», «75 лет», «1,5 мес», «3 мес.».
fn single(t: &[char]) -> Option<Age> {
    let (whole, mut rest) = number(t)?;
    let mut frac = 0.0_f64;
    if matches!(rest.first(), Some('.') | Some(',')) {
        let digits = rest[1..].iter().take_while(|c| c.is_ascii_digit()).count();
        if (1..=2).contains(&digits) {
            let s: String = rest[1..1 + digits].iter().collect();
            frac = format!("0.{s}").parse().ok()?;
            rest = &rest[1 + digits..];
        }
    }
    let mut rest = skip_spaces(rest);
    if rest.last() == Some(&'.') {
        rest = &rest[..rest.len() - 1];
    }
    let unit: String = rest.iter().collect();
    if !unit.is_empty() && !UNITS.contains(&unit.as_str()) {
        return None;
    }
    let has_frac = frac != 0.0;
    let mut out = Age::default();
    match if unit.is_empty() { 0 } else { rank(&unit) } {
        0 => {
            out.years = Some(whole);
            if has_frac {
                out.months = Some((frac * 12.0).round() as i64);
            }
        }
        1 => {
            out.months = Some(whole);
            if has_frac {
                out.days = Some((frac * 30.0).round() as i64);
            }
        }
        2 => {
            out.weeks = Some(whole);
            if has_frac {
                out.days = Some((frac * 7.0).round() as i64);
            }
        }
        _ => {
            if has_frac {
                return None; // «1,5 дня» — такого в книгах нет, остаётся текстом
            }
            out.days = Some(whole);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(y: Option<i64>, m: Option<i64>, w: Option<i64>, d: Option<i64>) -> Option<Age> {
        Some(Age { years: y, months: m, weeks: w, days: d })
    }

    /// Те же примеры, что в scripts/test_age.mjs.
    #[test]
    fn same_as_form() {
        assert_eq!(parse_age("5"), a(Some(5), None, None, None));
        assert_eq!(parse_age("75 лет"), a(Some(75), None, None, None));
        assert_eq!(parse_age("3 мес"), a(None, Some(3), None, None));
        assert_eq!(parse_age("3 мес."), a(None, Some(3), None, None));
        assert_eq!(parse_age("1,5 мес"), a(None, Some(1), None, Some(15)));
        assert_eq!(parse_age("2 нед"), a(None, None, Some(2), None));
        assert_eq!(parse_age("5 дней"), a(None, None, None, Some(5)));
        assert_eq!(parse_age("1 дня"), a(None, None, None, Some(1)));
        assert_eq!(parse_age("1 день"), a(None, None, None, Some(1)));
        assert_eq!(parse_age("1 год"), a(Some(1), None, None, None));
        assert_eq!(parse_age("  "), None);
        assert_eq!(parse_age("около года"), None);
        assert_eq!(parse_age("5 лет."), a(Some(5), None, None, None));
        assert_eq!(parse_age("9 лѣтъ"), a(Some(9), None, None, None));
        assert_eq!(parse_age("3 мѣс."), a(None, Some(3), None, None));
        assert_eq!(parse_age("1 годъ"), a(Some(1), None, None, None));
        assert_eq!(parse_age("1 год 3 мес"), a(Some(1), Some(3), None, None));
        assert_eq!(parse_age("2 г. 6 м."), a(Some(2), Some(6), None, None));
        assert_eq!(parse_age("1 мес, 2 нед и 3 дня"), a(None, Some(1), Some(2), Some(3)));
        assert_eq!(parse_age("1 год 2 года"), None);
        assert_eq!(parse_age("около 1 год 3 мес"), None);
        assert_eq!(parse_age("1 ги 3 ми"), None);
        assert_eq!(parse_age("3 мес 1 год"), None);
        assert_eq!(parse_age("10 лет 11 месяцев"), a(Some(10), Some(11), None, None));
        assert_eq!(parse_age("3мес"), a(None, Some(3), None, None));
    }
}
