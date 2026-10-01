//! GenMetric: логика без окна — запросы, нормализация, выгрузка в xlsx.
//!
//! Всё, что не требует Tauri, живёт здесь: так его тесты собираются на
//! любом Linux без GTK и идут в быстрой проверке конвейера
//! (`cargo test -p genmetric-core`). Программа (src-tauri/src) только
//! оборачивает это в команды окна.

pub mod export;
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

