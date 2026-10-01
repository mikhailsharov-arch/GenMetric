//! Ключи поиска. Обязаны совпадать с norm() в db/build_seed.py.

use unicode_normalization::UnicodeNormalization;

/// Ключ для поиска по префиксу.
///
/// Обязан совпадать с norm() в db/build_seed.py, иначе подсказки перестанут
/// находиться. Встроенный в SQLite COLLATE NOCASE кириллицу не понимает,
/// поэтому нормализуем сами.
pub fn normalize(input: &str) -> String {
    // NFC — как в norm() на Python: «й» одной буквой и «и» + краткая отдельным
    // знаком (так бывает в тексте, вставленном из PDF или с macOS) дают один
    // ключ. До 01.10.2026 Rust этого не делал, Python — делал (техдолг Д7).
    let composed: String = input.nfc().collect();
    let lowered = composed.trim().to_lowercase().replace('ё', "е");
    lowered.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Ключ для сверки одного слова ИОФ со словарём.
///
/// Книги — в дореформенной орфографии: «Иванъ», «Петровъ». Конечный «ъ»
/// отбрасывается до поиска, и такое имя считается известным без окна сверки.
/// Заказчик 28.08.2026: в упражнении 9 из 31 имени не опознались только
/// из-за «ъ». Заодно «і» → «и», «ѣ» → «е», «ѳ» → «ф» — те же книги
/// («Ѳома» → «Фома», ревьюер 23.09.2026).
pub fn normalize_name(input: &str) -> String {
    let n = normalize(input).replace('і', "и").replace('ѣ', "е").replace('ѳ', "ф");
    n.strip_suffix('ъ').map(str::to_string).unwrap_or(n)
}

/// normalize_name для каждого слова: «крестьянскій сынъ» → «крестьянский сын».
pub fn normalize_words(input: &str) -> String {
    input.split_whitespace().map(normalize_name).collect::<Vec<_>>().join(" ")
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nfc() {
        // «й» составное (и + U+0306) и цельное — один ключ.
        assert_eq!(normalize("Андре\u{438}\u{306}"), normalize("Андрей"));
        assert_eq!(normalize("Ё\u{301}лка"), normalize("Ё\u{301}лка"));
        assert_eq!(normalize("  Чертёж   Малый "), "чертеж малый");
    }

    #[test]
    fn old_spelling() {
        assert_eq!(normalize_name("Иванъ"), "иван");
        assert_eq!(normalize_name("Ѳома"), "фома");
        assert_eq!(normalize_words("крестьянскій сынъ"), "крестьянский сын");
    }
}
