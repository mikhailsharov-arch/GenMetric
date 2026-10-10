//! GenMetric: логика без окна — запросы, нормализация, выгрузка в xlsx.
//!
//! Всё, что не требует Tauri, живёт здесь: так его тесты собираются на
//! любом Linux без GTK и идут в быстрой проверке конвейера
//! (`cargo test -p genmetric-core`). Программа (src-tauri/src) только
//! оборачивает это в команды окна.

pub mod age;
pub mod db;
pub mod export;
pub mod import;
pub mod parish;
pub mod records;
pub mod scans;
pub mod search;
pub mod text;
pub mod xlsx;

/// Запросы записи и чтения. Тот же файл читает тест db/test_entry.py —
/// значит проверяется именно то, что работает у человека, а не похожая копия.
pub const STATEMENTS_SQL: &str = include_str!("../../../db/statements.sql");

/// Разбирает statements.sql на именованные блоки, разделённые «-- @имя».
/// Такой же разбор делает тест: формат намеренно простейший.
pub fn statement(name: &str) -> Result<String, String> {
    let mut current: Option<&str> = None;
    let mut buf: Vec<&str> = Vec::new();
    for line in STATEMENTS_SQL.lines() {
        let marker = line.trim();
        if let Some(rest) = marker.strip_prefix("-- @") {
            if current == Some(name) {
                return Ok(buf.join("\n"));
            }
            current = Some(rest.trim());
            buf.clear();
        } else if current == Some(name) {
            buf.push(line);
        }
    }
    if current == Some(name) {
        return Ok(buf.join("\n"));
    }
    Err(format!("в statements.sql нет блока «{name}»"))
}


/// Запрос подсказок для перечня `kind` и признак «запросу нужен пол».
///
/// Обёртка `suggest_ranked` с двумя подстановками: источник словаря и отбор
/// по полу в ветке частот. Имена и отчества сверяются по полу со словарём,
/// фамилии — по окончанию; остальным перечням пол не нужен, и параметра
/// `:gender` в их запросе нет (лишний именованный параметр rusqlite считает
/// ошибкой). Ту же сборку повторяет db/test_suggest.py.
pub fn suggest_sql(kind: &str) -> Result<(String, bool), String> {
    let clean = |block: String| block.trim().trim_end_matches(';').to_string();
    let (dict, usage_gender) = match kind {
        "first_name" => (statement("suggest_first_name")?, statement("usage_gender_filter")?),
        "patronymic" => (statement("suggest_patronymic")?, statement("usage_gender_filter")?),
        "surname" => {
            // В отборе фамилий внешняя строка названа по своей таблице.
            let filter = clean(statement("surname_gender_filter")?);
            (format!("{}\n{}", clean(statement("suggest_surname")?), filter.replace("{outer}", "lookup")),
             filter.replace("{outer}", "usage_stat"))
        }
        "place" => (statement("suggest_place")?, String::new()),
        _ => (statement("suggest_lookup")?, String::new()),
    };
    let sql = statement("suggest_ranked")?
        .replace("{dict}", &clean(dict))
        .replace("{usage_gender}", &clean(usage_gender.clone()));
    Ok((sql, !usage_gender.is_empty()))
}

/// Адрес поиска населённого пункта на Familio (Роман 03.10.2026, задача 3).
///
/// Страница «Места» читает два параметра: `title` — название, `georequisites`
/// — «Где искать». Проверено на сайте 06.10.2026: «Малово» + «Завражная» и
/// «Малово» + «Костромская Макарьевский Завражная» находят одну и ту же
/// деревню. Параметр `search` из постановки страница не читает — поле поиска
/// остаётся пустым. Адрес собирается здесь, а не в окне: команда открытия не
/// принимает произвольных ссылок.
pub fn familio_search_url(name: &str, guberniya: &str, uyezd: &str, volost: &str) -> String {
    fn encode(text: &str) -> String {
        let mut out = String::new();
        for b in text.trim().bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
        out
    }
    let where_to: Vec<&str> = [guberniya, uyezd, volost].into_iter().map(str::trim).filter(|v| !v.is_empty()).collect();
    let mut url = format!("https://familio.org/places?title={}", encode(name));
    if !where_to.is_empty() {
        url.push_str(&format!("&georequisites={}", encode(&where_to.join(" "))));
    }
    url
}

/// Выполнить `work`, превратив панику в обычную ошибку.
///
/// Файл Excel приходит от человека и может быть каким угодно: разбор чужого
/// формата — то место, где паника вероятнее всего (индекс за краем, деление,
/// `unwrap` на том, чего «не бывает»). Без перехвата она отравляла замок с
/// файлом (окно висело на «Читаю файл…»), а посреди импорта оставляла файл
/// прихода без строки в перечне. `what` — что делали, по-русски: попадёт в
/// сообщение человеку.
pub fn guarded<T>(what: &str, work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    match caught(work) {
        Ok(result) => result,
        Err(why) => Err(format!("{what}: внутренняя ошибка программы ({why}). Данные не изменены; пришлите этот текст и файл разработчику")),
    }
}

/// Работа с базой под перехватом паники — для команд окна. Обычная команда
/// исполняется в главном потоке, и паника в ней закрывала программу вместе с
/// набранным в форме. Здесь она становится ошибкой; начатую и не законченную
/// транзакцию откатываем сами (не каждая у нас — объект, который откатился
/// бы при раскрутке: есть и «BEGIN» текстом).
pub fn guarded_db<T>(conn: &rusqlite::Connection, body: impl FnOnce(&rusqlite::Connection) -> Result<T, String>) -> Result<T, String> {
    match caught(|| body(conn)) {
        Ok(result) => result,
        Err(why) => {
            if !conn.is_autocommit() {
                let _ = conn.execute_batch("ROLLBACK");
            }
            // Не «изменение отменено»: запись могла быть уже сохранена, а паника
            // случиться после — в сверке справочников (ревьюер 10.10.2026).
            Err(format!("внутренняя ошибка программы ({why}). Программа работает дальше. Если вы сохраняли запись — проверьте в списке «Набрано», сохранилась ли она; пришлите этот текст разработчику"))
        }
    }
}

/// Работа под перехватом паники: `Err` — её текст. Для тех, кому после паники
/// нужно ещё прибраться самим (откатить транзакцию) и сказать своё.
pub fn caught<T>(work: impl FnOnce() -> T) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).map_err(|payload| {
        payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "причина не названа".to_string())
    })
}

/// Тесту нужна база поставки (`resources/seed.sqlite`), а её нет. У разработчика
/// тест пропускается со строкой в выводе; в конвейере (переменная `CI`) — падает:
/// до 10.10.2026 пропуск был молчаливым, и сломанный шаг сборки поставки
/// оставил бы зелёными тесты, которые ничего не проверили.
#[cfg(test)]
pub(crate) fn seed_missing(what: &str) {
    if std::env::var_os("CI").is_some() {
        panic!("нет resources/seed.sqlite — в конвейере {what} быть не должен: поставку собирает шаг перед тестами");
    }
    eprintln!("нет resources/seed.sqlite — {what}");
}

#[cfg(test)]
mod tests {
    use super::{familio_search_url, guarded, guarded_db, suggest_sql};

    /// Паника в команде окна: ошибка вместо закрытой программы, начатое
    /// изменение отменено, соединение работает дальше.
    #[test]
    fn panic_in_db_work_is_an_error() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (v INTEGER)").unwrap();
        let out: Result<(), String> = guarded_db(&conn, |c| {
            c.execute_batch("BEGIN; INSERT INTO t VALUES (1)").unwrap();
            let none: Option<i64> = None;
            none.expect("значения нет");
            Ok(())
        });
        let text = out.unwrap_err();
        assert!(text.contains("внутренняя ошибка программы") && text.contains("значения нет"), "{text}");
        assert!(conn.is_autocommit(), "транзакция откатана");
        let n: i64 = conn.query_row("SELECT count(*) FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "начатое изменение отменено");
        // Объект-транзакция откатывается сам, второй ROLLBACK не нужен и не мешает.
        let out: Result<(), String> = guarded_db(&conn, |c| {
            let tx = c.unchecked_transaction().unwrap();
            tx.execute("INSERT INTO t VALUES (2)", []).unwrap();
            panic!("посреди транзакции");
        });
        assert!(out.is_err() && conn.is_autocommit());
        assert_eq!(guarded_db(&conn, |c| c.query_row("SELECT count(*) FROM t", [], |r| r.get::<_, i64>(0)).map_err(|e| e.to_string())), Ok(0));
        assert_eq!(guarded_db(&conn, |_| Err::<(), String>("обычная ошибка".into())), Err("обычная ошибка".into()));
    }

    #[test]
    fn familio_url() {
        assert_eq!(familio_search_url("Малово", "", "", "Завражная"),
                   "https://familio.org/places?title=%D0%9C%D0%B0%D0%BB%D0%BE%D0%B2%D0%BE&georequisites=%D0%97%D0%B0%D0%B2%D1%80%D0%B0%D0%B6%D0%BD%D0%B0%D1%8F");
        assert_eq!(familio_search_url(" Ново Село ", " ", "", ""), "https://familio.org/places?title=%D0%9D%D0%BE%D0%B2%D0%BE%20%D0%A1%D0%B5%D0%BB%D0%BE");
        // Знаки, которыми можно было бы подменить адрес, уходят в значение.
        let odd = familio_search_url("a&b=c#d/e?f", "Костромская", "Макарьевский", "");
        assert_eq!(odd, "https://familio.org/places?title=a%26b%3Dc%23d%2Fe%3Ff&georequisites=%D0%9A%D0%BE%D1%81%D1%82%D1%80%D0%BE%D0%BC%D1%81%D0%BA%D0%B0%D1%8F%20%D0%9C%D0%B0%D0%BA%D0%B0%D1%80%D1%8C%D0%B5%D0%B2%D1%81%D0%BA%D0%B8%D0%B9");
    }

    /// Запрос подсказок собирается и исполняется для каждого вида перечня;
    /// фамилии отбираются по полу (rusqlite строг к лишним параметрам —
    /// Python-тест этого не видит).
    #[test]
    fn suggest_sql_runs() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../../db/schema.sql")).unwrap();
        let add = |kind: &str, value: &str, n: i64| {
            conn.execute("INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count) VALUES (?1, 'parish', 'п', ?2, ?3, ?4)",
                         rusqlite::params![kind, value, crate::text::normalize(value), n]).unwrap();
            conn.execute("INSERT OR IGNORE INTO lookup (kind, value, value_norm) VALUES (?1, ?2, ?3)",
                         rusqlite::params![kind, value, crate::text::normalize(value)]).unwrap();
        };
        for (value, n) in [("Томилин", 12), ("Томилина", 9), ("Томский", 2), ("Томская", 1), ("Томенко", 5), ("Томилинъ", 1)] {
            add("surname", value, n);
        }
        // Без пары в приходе фамилия не прячется: «Сова» — мужская фамилия,
        // а одинокого «Петрова» женщине лучше показать, чем промолчать.
        for (value, n) in [("Сова", 3), ("Петров", 4), ("Палий", 2), ("Калина", 1)] {
            add("surname", value, n);
        }
        add("rank_m", "томящийся", 1);
        let run = |kind: &str, gender: Option<&str>| -> Vec<String> {
            let (sql, gendered) = suggest_sql(kind).unwrap();
            let (prefix, limit) = ("том%".to_string(), 20i64);
            let mut params: Vec<(&str, &dyn rusqlite::ToSql)> = vec![(":kind", &kind), (":prefix", &prefix), (":limit", &limit)];
            if gendered {
                params.push((":gender", &gender));
            }
            let mut stmt = conn.prepare(&sql).unwrap();
            let rows = stmt.query_map(&params[..], |r| r.get::<_, String>(0)).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        assert_eq!(run("surname", Some("Ж")), vec!["Томилина", "Томенко", "Томская"]);
        assert_eq!(run("surname", Some("М")), vec!["Томилин", "Томенко", "Томский", "Томилинъ"]);
        assert_eq!(run("surname", None).len(), 6, "пол неизвестен — все");
        let other = |prefix: &str, gender: &str| -> Vec<String> {
            let (sql, _) = suggest_sql("surname").unwrap();
            let (kind, prefix, limit, gender) = ("surname", format!("{prefix}%"), 20i64, Some(gender));
            let mut stmt = conn.prepare(&sql).unwrap();
            let rows = stmt.query_map(rusqlite::named_params! { ":kind": kind, ":prefix": prefix, ":limit": limit, ":gender": gender },
                                      |r| r.get::<_, String>(0)).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        assert_eq!(other("сов", "М"), vec!["Сова"]);
        assert_eq!(other("кал", "М"), vec!["Калина"]);
        assert_eq!(other("петр", "Ж"), vec!["Петров"]);
        assert_eq!(other("пал", "Ж"), vec!["Палий"]);
        // Не медленнее с ростом прихода: пара ищется по индексу. 6000 фамилий
        // и первая буква — доли секунды (без индекса было бы больше минуты).
        let tx = conn.unchecked_transaction().unwrap();
        for i in 0..3000 {
            for tail in ["ов", "ова"] {
                let v = format!("Кр{i}{tail}");
                tx.execute("INSERT INTO usage_stat (kind, scope, scope_key, value, value_norm, count) VALUES ('surname', 'global', '', ?1, ?2, 1)",
                           rusqlite::params![v, crate::text::normalize(&v)]).unwrap();
                tx.execute("INSERT INTO lookup (kind, value, value_norm) VALUES ('surname', ?1, ?2)",
                           rusqlite::params![v, crate::text::normalize(&v)]).unwrap();
            }
        }
        tx.commit().unwrap();
        let started = std::time::Instant::now();
        let women = other("к", "Ж");
        assert!(women.iter().all(|v| v.ends_with("ова") || v == "Калина"), "{women:?}");
        assert!(started.elapsed().as_millis() < 1500, "подсказка фамилий на 6000 значений: {:?}", started.elapsed());
        assert_eq!(run("rank_m", None), vec!["томящийся"]);
        for kind in ["first_name", "patronymic", "place", "archive"] {
            run(kind, Some("Ж"));
        }
    }

    #[test]
    fn panic_becomes_error() {
        assert_eq!(guarded("разбор", || Ok::<_, String>(7)), Ok(7));
        assert_eq!(guarded("разбор", || Err::<i32, _>("плохо".to_string())), Err("плохо".to_string()));
        let caught = guarded("разбор файла", || -> Result<i32, String> { panic!("индекс {} за краем", 9) }).unwrap_err();
        assert!(caught.starts_with("разбор файла: внутренняя ошибка") && caught.contains("индекс 9 за краем"), "{caught}");
    }
}
