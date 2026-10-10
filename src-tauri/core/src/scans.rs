//! Папка сканов дела (спека 2026-10-10-skany-ryadom-s-formoj).
//!
//! Роман 05.10.2026: «Формат файлов: JPG. Сканы одного дела лежат в одной
//! папке. Одно фото (один файл) — это всегда один разворот книги. … Четкого
//! единого формата или стандарта нейминга нет». Поэтому программа не читает
//! номер страницы из имени, а только ставит файлы по порядку имён — так, как
//! их ставит проводник: «лист 2» раньше «лист 10».

use std::cmp::Ordering;
use std::path::Path;

/// Файл скана: `.jpg` или `.jpeg` в любом регистре, не служебный (у macOS
/// рядом с картинкой на чужом диске лежит «._имя.jpg»).
pub fn is_scan(name: &str) -> bool {
    if name.starts_with('.') {
        return false;
    }
    let lower = name.to_lowercase();
    lower.ends_with(".jpg") || lower.ends_with(".jpeg")
}

/// Порядок имён «по-человечески»: цифры сравниваются как числа, буквы — без
/// учёта регистра. При равенстве решает обычное сравнение строк: порядок
/// всегда один и тот же.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (x, y): (Vec<char>, Vec<char>) = (a.to_lowercase().chars().collect(), b.to_lowercase().chars().collect());
    let (mut i, mut j) = (0, 0);
    while i < x.len() && j < y.len() {
        if x[i].is_ascii_digit() && y[j].is_ascii_digit() {
            let run = |s: &[char], from: usize| -> usize { s[from..].iter().take_while(|c| c.is_ascii_digit()).count() };
            let (n, m) = (run(&x, i), run(&y, j));
            // Числа любой длины: без ведущих нулей длиннее — больше.
            let trim = |s: &[char]| -> Vec<char> { let t: Vec<char> = s.iter().cloned().skip_while(|c| *c == '0').collect(); t };
            let (p, q) = (trim(&x[i..i + n]), trim(&y[j..j + m]));
            let by_value = p.len().cmp(&q.len()).then_with(|| p.cmp(&q));
            if by_value != Ordering::Equal {
                return by_value;
            }
            i += n;
            j += m;
        } else {
            if x[i] != y[j] {
                return x[i].cmp(&y[j]);
            }
            i += 1;
            j += 1;
        }
    }
    (x.len() - i).cmp(&(y.len() - j)).then_with(|| a.cmp(b))
}

/// Имена файлов сканов папки по порядку. Вложенные папки не читаются.
pub fn list(dir: &Path) -> Result<Vec<String>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("папка сканов не читается: {e}"))?;
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| !t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| is_scan(n))
        .collect();
    names.sort_by(|a, b| natural_cmp(a, b));
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_names_like_a_person() {
        let mut names = vec!["лист 10.jpg", "лист 2.JPG", "Лист 1.jpg", "IMG_0100.jpeg", "IMG_99.jpg", "a.jpg", "001об.jpg", "1.jpg", "01.jpg"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, vec!["01.jpg", "1.jpg", "001об.jpg", "a.jpg", "IMG_99.jpg", "IMG_0100.jpeg", "Лист 1.jpg", "лист 2.JPG", "лист 10.jpg"]);
        assert_eq!(natural_cmp("скан12345678901234567890123.jpg", "скан12345678901234567890124.jpg"), Ordering::Less, "числа длиннее u64");
        assert_eq!(natural_cmp("a.jpg", "a.jpg"), Ordering::Equal);
    }

    #[test]
    fn lists_only_scans() {
        let dir = std::env::temp_dir().join(format!("genmetric-scans-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("вложенная.jpg")).unwrap();
        for name in ["2.jpg", "10.JPEG", "1.jpg", "._1.jpg", "заметки.txt", "обложка.png", ".DS_Store"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        assert_eq!(list(&dir).unwrap(), vec!["1.jpg", "2.jpg", "10.JPEG"], "только jpg и jpeg, без служебных и без папок");
        assert!(list(&dir.join("нет такой")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
