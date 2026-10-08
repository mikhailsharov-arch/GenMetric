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

/// Название пункта в программе: у деревни-тёзки с комментарием — «Хмельничное
/// (Столпино)» (Роман 06.10.2026). Так пункт стоит в поле НП, в подсказке и в
/// памяти персон; в выгрузки идёт чистое название (`place_clean`). Копия для
/// окна — `placeLabel` в `src/names.ts`: правишь одно — правь другое.
pub fn place_label(name: &str, comment: &str) -> String {
    let (name, comment) = (name.trim(), comment.trim());
    if comment.is_empty() { name.to_string() } else { format!("{name} ({comment})") }
}

/// Чистое название: метка без комментария. Комментарий берётся из колонки, а
/// не угадывается по скобкам: «Никольское (Старое)» может быть и названием.
pub fn place_clean<'a>(label: &'a str, comment: &str) -> &'a str {
    let (label, comment) = (label.trim(), comment.trim());
    if comment.is_empty() {
        return label;
    }
    label.strip_suffix(&format!(" ({comment})")).map(str::trim_end).unwrap_or(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_comment() {
        assert_eq!(place_label("Хмельничное", "Столпино"), "Хмельничное (Столпино)");
        assert_eq!(place_label(" Хмельничное ", "  "), "Хмельничное");
        assert_eq!(place_clean("Хмельничное (Столпино)", "Столпино"), "Хмельничное");
        assert_eq!(place_clean("Хмельничное", ""), "Хмельничное");
        // Скобки в названии без комментария — часть названия.
        assert_eq!(place_clean("Никольское (Старое)", ""), "Никольское (Старое)");
        // Комментарий в колонке есть, а метка другая — метку не режем.
        assert_eq!(place_clean("Хмельничное", "Столпино"), "Хмельничное");
        assert_eq!(place_clean(&place_label("д.Балахонка, Заобнорская волость", "чужой приход"), "чужой приход"),
                   "д.Балахонка, Заобнорская волость");
    }

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
