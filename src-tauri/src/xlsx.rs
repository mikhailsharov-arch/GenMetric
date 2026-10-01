//! Заполнение готового файла Excel (шаблона) строками выгрузки.
//!
//! Зачем шаблон, а не новый файл. Роман 30.09.2026 про выгрузку в Familio:
//! «Столбцы шаблона … просто остаются пустыми, удалять их из шаблона нельзя».
//! В образце Familio (db/export/familio_template.xlsx) кроме листов с данными
//! есть описание, списки, справочник имён, условное форматирование и проверки
//! ввода — воспроизводить это заново значило бы разойтись с образцом. Поэтому
//! берётся сам образец, и в нём меняются только строки данных.
//!
//! Как устроено. Файл xlsx — zip с XML внутри. Лист находится по имени через
//! xl/workbook.xml и его связи; в листе строки с номером от `first_row` и
//! ниже (примеры из образца) удаляются, на их место встают наши. Шапка,
//! ширины колонок, объединения в шапке, условное форматирование — остаются.
//! Отдельные ячейки (лист about) заменяются по адресу.
//!
//! Попутно, чтобы Excel открыл файл без «восстановления содержимого»:
//! - ссылки (hyperlinks) из удалённых и заменённых ячеек убираются;
//! - объединения ячеек в удалённых строках убираются;
//! - xl/calcChain.xml убирается: он перечисляет формулы удалённых ячеек;
//!   Excel пересчитает формулы при открытии (fullCalcOnLoad);
//! - границы dimension, autoFilter и _FilterDatabase растягиваются на данные.
//!
//! Текст пишется прямо в ячейку (inlineStr), sharedStrings не трогается.
//! Стиль ячейки берётся из первой строки данных образца той же колонки —
//! рамки и формат остаются как в образце.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

/// Строки одного листа: с какой строки писать и что.
pub struct SheetRows {
    pub sheet: String,
    /// Номер первой строки данных (1 — первая строка листа).
    pub first_row: u32,
    /// Значения по колонкам с A; None — пустая ячейка.
    pub rows: Vec<Vec<Option<String>>>,
}

/// Одна ячейка по адресу: лист about, «B4».
pub struct CellValue {
    pub sheet: String,
    pub cell: String,
    pub value: String,
}

/// Номер колонки (с 1) по буквам: A → 1, Z → 26, AA → 27.
pub fn col_index(letters: &str) -> u32 {
    letters.bytes().fold(0, |n, b| n * 26 + (b.to_ascii_uppercase() - b'A' + 1) as u32)
}

/// Буквы колонки по номеру (с 1).
pub fn col_letters(mut n: u32) -> String {
    let mut out = Vec::new();
    while n > 0 {
        let r = (n - 1) % 26;
        out.push(b'A' + r as u8);
        n = (n - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// «BQ103» → ("BQ", 103).
fn split_ref(r: &str) -> (String, u32) {
    let letters: String = r.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let digits: String = r.chars().skip(letters.len()).take_while(|c| c.is_ascii_digit()).collect();
    (letters, digits.parse().unwrap_or(0))
}

/// Экранирование текста для XML. Управляющие символы (кроме табуляции и
/// перевода строки) в XML 1.0 запрещены — Excel откажется открыть файл.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' => out.push(ch),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"")
        .replace("&apos;", "'").replace("&amp;", "&")
}

/// Значение атрибута в открывающем теге: attr(`<sheet name="x" …>`, "name").
fn attr(tag: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(pos) = tag[from..].find(name) {
        let at = from + pos;
        let before_ok = at == 0 || tag.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = &tag[at + name.len()..];
        if before_ok && rest.starts_with("=\"") {
            let body = &rest[2..];
            return body.find('"').map(|end| unescape(&body[..end]));
        }
        from = at + name.len();
    }
    None
}

/// Все элементы `<name …/>` или `<name …>…</name>` в тексте — (начало, конец).
/// Вложенных элементов с тем же именем в нужных нам местах не бывает.
fn elements(xml: &str, name: &str) -> Vec<(usize, usize)> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(pos) = xml[from..].find(&open) {
        let start = from + pos;
        let after = xml.as_bytes().get(start + open.len()).copied().unwrap_or(b' ');
        if !(after == b' ' || after == b'>' || after == b'/') {
            from = start + open.len();
            continue;
        }
        let Some(gt) = xml[start..].find('>') else { break };
        let tag_end = start + gt + 1;
        let end = if xml[..tag_end].ends_with("/>") {
            tag_end
        } else {
            match xml[tag_end..].find(&close) {
                Some(p) => tag_end + p + close.len(),
                None => break,
            }
        };
        out.push((start, end));
        from = end;
    }
    out
}

/// Открывающий тег элемента.
fn open_tag(element: &str) -> &str {
    &element[..element.find('>').map(|p| p + 1).unwrap_or(element.len())]
}

/// Путь листа внутри zip по имени листа.
fn sheet_path(workbook: &str, rels: &str, sheet: &str) -> Option<String> {
    let rid = elements(workbook, "sheet").into_iter()
        .map(|(s, e)| open_tag(&workbook[s..e]).to_string())
        .find(|t| attr(t, "name").as_deref() == Some(sheet))
        .and_then(|t| attr(&t, "r:id"))?;
    let target = elements(rels, "Relationship").into_iter()
        .map(|(s, e)| rels[s..e].to_string())
        .find(|t| attr(t, "Id").as_deref() == Some(rid.as_str()))
        .and_then(|t| attr(&t, "Target"))?;
    let target = target.trim_start_matches('/');
    Some(if target.starts_with("xl/") { target.to_string() } else { format!("xl/{target}") })
}

/// Ячейка с текстом или числом. Числа — числами только там, где они в
/// образце числа (решает вызывающий через `numeric`); иначе — текстом,
/// как «05» в дате, иначе потерялся бы ведущий ноль.
fn cell_xml(r: &str, style: Option<&str>, value: &str, numeric: bool) -> String {
    let s = style.map(|s| format!(" s=\"{s}\"")).unwrap_or_default();
    // Больше 32 767 символов в ячейке Excel считает повреждением файла.
    let value: String = if value.chars().count() > 32_000 { value.chars().take(32_000).collect() } else { value.to_string() };
    let value = value.as_str();
    if numeric {
        format!("<c r=\"{r}\"{s}><v>{}</v></c>", escape(value))
    } else {
        format!("<c r=\"{r}\"{s} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>", escape(value))
    }
}

/// Число без ведущих нулей и без дробной части — пишется числом.
fn is_plain_number(v: &str) -> bool {
    !v.is_empty() && v.len() < 10 && v.bytes().all(|b| b.is_ascii_digit())
        && !(v.len() > 1 && v.starts_with('0'))
}

/// Заменить строки данных листа. Возвращает новый XML и последнюю строку.
fn fill_sheet(xml: &str, fill: &SheetRows, numeric_cols: &[u32]) -> Result<(String, u32), String> {
    let data_start = xml.find("<sheetData").ok_or("в листе нет sheetData")?;
    let (inner_start, inner_end, data_end) = if let Some(p) = xml[data_start..].find("/>")
        .filter(|p| !xml[data_start..data_start + p].contains('>'))
    {
        // <sheetData/> — пустой лист.
        (data_start + p, data_start + p, data_start + p + 2)
    } else {
        let open_end = data_start + xml[data_start..].find('>').ok_or("sheetData")? + 1;
        let close = xml[open_end..].find("</sheetData>").ok_or("sheetData не закрыт")? + open_end;
        (open_end, close, close + "</sheetData>".len())
    };
    let inner = &xml[inner_start..inner_end];

    // Стили колонок — из первой строки данных образца.
    let mut styles: BTreeMap<u32, String> = BTreeMap::new();
    // Высота и стиль строки данных — ровно как у строки образца (Роман
    // 01.10.2026: в выгрузке Excel «слетает … высота строк»). Ничего сверх
    // образца: с customHeight, которого у Familio нет, длинный текст в его
    // листах обрезался бы (проверяющий #40).
    let mut row_attrs = String::new();
    let mut kept = String::new();
    for (s, e) in elements(inner, "row") {
        let row = &inner[s..e];
        let r: u32 = attr(open_tag(row), "r").and_then(|v| v.parse().ok()).unwrap_or(0);
        if r < fill.first_row {
            kept.push_str(row);
            continue;
        }
        if r == fill.first_row {
            for name in ["s", "customFormat", "ht", "customHeight"] {
                if let Some(v) = attr(open_tag(row), name) {
                    let _ = write!(row_attrs, " {name}=\"{v}\"");
                }
            }
            for (cs, ce) in elements(row, "c") {
                let tag = open_tag(&row[cs..ce]);
                if let (Some(cref), Some(style)) = (attr(tag, "r"), attr(tag, "s")) {
                    styles.insert(col_index(&split_ref(&cref).0), style);
                }
            }
        }
    }

    let mut rows_xml = String::new();
    let mut last_row = fill.first_row.saturating_sub(1);
    let mut max_col = 0u32;
    for (i, values) in fill.rows.iter().enumerate() {
        let r = fill.first_row + i as u32;
        last_row = r;
        let _ = write!(rows_xml, "<row r=\"{r}\"{row_attrs}>");
        for (j, v) in values.iter().enumerate() {
            let col = j as u32 + 1;
            let Some(v) = v.as_deref().filter(|v| !v.is_empty()) else { continue };
            max_col = max_col.max(col);
            let cref = format!("{}{}", col_letters(col), r);
            let numeric = numeric_cols.contains(&col) && is_plain_number(v);
            rows_xml.push_str(&cell_xml(&cref, styles.get(&col).map(String::as_str), v, numeric));
        }
        rows_xml.push_str("</row>");
    }

    let mut out = String::with_capacity(xml.len() + rows_xml.len());
    out.push_str(&xml[..data_start]);
    out.push_str("<sheetData>");
    out.push_str(&kept);
    out.push_str(&rows_xml);
    out.push_str("</sheetData>");
    out.push_str(&xml[data_end..]);

    // Ссылки и объединения в удалённых строках.
    let out = drop_children(&out, "hyperlinks", "hyperlink", |tag| {
        attr(tag, "ref").map(|r| split_ref(&r).1 >= fill.first_row).unwrap_or(false)
    });
    let out = drop_children(&out, "mergeCells", "mergeCell", |tag| {
        attr(tag, "ref").map(|r| split_ref(r.split(':').next().unwrap_or("")).1 >= fill.first_row).unwrap_or(false)
    });
    // Одно значение и листу (autoFilter, dimension), и имени _FilterDatabase в
    // книге — на пустом листе они расходились на строку (ревьюер #40).
    let last_row = last_row.max(fill.first_row);
    let out = stretch_ranges(&out, last_row);
    let _ = max_col;
    Ok((out, last_row))
}

use std::fmt::Write as _;

/// Убрать из обёртки (<hyperlinks>) дочерние элементы `child` по условию.
/// Остальное содержимое обёртки не трогается: 01.10.2026 первая версия
/// собирала обёртку заново из одних `child` и теряла в [Content_Types].xml
/// все <Default> — Excel открывал файл с «восстановлением» (инцидент,
/// Роман 01.10). Обёртка, в которой после этого не осталось ни одного
/// элемента, убирается целиком: пустой <hyperlinks/> Excel считает
/// повреждением.
fn drop_children(xml: &str, wrapper: &str, child: &str, drop: impl Fn(&str) -> bool) -> String {
    let Some(&(ws, we)) = elements(xml, wrapper).first() else { return xml.to_string() };
    let block = &xml[ws..we];
    let tag = open_tag(block);
    if tag.ends_with("/>") {
        return xml.to_string();
    }
    let close = format!("</{wrapper}>");
    let body = &block[tag.len()..block.len() - close.len()];
    let mut kept_body = String::with_capacity(body.len());
    let mut from = 0;
    let mut left = 0usize;
    for (s, e) in elements(body, child) {
        kept_body.push_str(&body[from..s]);
        if drop(open_tag(&body[s..e])) {
            // вырезаем
        } else {
            kept_body.push_str(&body[s..e]);
            left += 1;
        }
        from = e;
    }
    kept_body.push_str(&body[from..]);
    let mut out = String::with_capacity(xml.len());
    out.push_str(&xml[..ws]);
    if !kept_body.trim().is_empty() {
        // count="…" у mergeCells — число оставшихся.
        let tag = match attr(tag, "count") {
            Some(old) => tag.replacen(&format!("count=\"{old}\""), &format!("count=\"{left}\""), 1),
            None => tag.to_string(),
        };
        out.push_str(&tag);
        out.push_str(&kept_body);
        out.push_str(&close);
    }
    out.push_str(&xml[we..]);
    out
}

/// Растянуть dimension и autoFilter до последней строки данных.
fn stretch_ranges(xml: &str, last_row: u32) -> String {
    let mut out = xml.to_string();
    for name in ["dimension", "autoFilter"] {
        if let Some(&(s, e)) = elements(&out, name).first() {
            let tag = open_tag(&out[s..e]).to_string();
            if let Some(r) = attr(&tag, "ref") {
                if let Some((a, b)) = r.split_once(':') {
                    let (letters, _) = split_ref(b);
                    let new = format!("{a}:{letters}{last_row}");
                    let new_tag = tag.replacen(&format!("ref=\"{r}\""), &format!("ref=\"{new}\""), 1);
                    out.replace_range(s..s + tag.len(), &new_tag);
                }
            }
        }
    }
    out
}

/// Заменить ячейки по адресам (лист about). Ссылка, висевшая на ячейке в
/// образце (профиль автора образца), уходит вместе со старым значением.
fn set_cells(xml: &str, cells: &[&CellValue]) -> Result<String, String> {
    let mut out = xml.to_string();
    for cv in cells {
        let (letters, row) = split_ref(&cv.cell);
        let col = col_index(&letters);
        let rows = elements(&out, "row");
        let found = rows.iter().copied().find(|&(s, e)| {
            attr(open_tag(&out[s..e]), "r").and_then(|v| v.parse::<u32>().ok()) == Some(row)
        });
        let (s, e) = found.ok_or_else(|| format!("в образце нет строки {row}"))?;
        let row_xml = out[s..e].to_string();
        let tag = open_tag(&row_xml).to_string();
        let self_closing = tag.ends_with("/>");
        let body = if self_closing { "" } else { &row_xml[tag.len()..row_xml.len() - "</row>".len()] };
        let mut cells_in: Vec<(u32, String)> = elements(body, "c").into_iter().map(|(a, b)| {
            let c = body[a..b].to_string();
            let r = attr(open_tag(&c), "r").unwrap_or_default();
            (col_index(&split_ref(&r).0), c)
        }).collect();
        let style = cells_in.iter().find(|(c, _)| *c == col)
            .and_then(|(_, x)| attr(open_tag(x), "s"));
        cells_in.retain(|(c, _)| *c != col);
        if !cv.value.is_empty() {
            // «1886» в about!B6 — числом, как в образце.
            cells_in.push((col, cell_xml(&cv.cell, style.as_deref(), &cv.value, is_plain_number(&cv.value))));
        }
        cells_in.sort_by_key(|(c, _)| *c);
        let open = if self_closing { format!("{}>", &tag[..tag.len() - 2]) } else { tag.clone() };
        let mut new_row = open;
        for (_, c) in &cells_in {
            new_row.push_str(c);
        }
        new_row.push_str("</row>");
        out.replace_range(s..e, &new_row);
        let cell = cv.cell.clone();
        out = drop_children(&out, "hyperlinks", "hyperlink", |t| attr(t, "ref").as_deref() == Some(cell.as_str()));
    }
    Ok(out)
}

/// Заполнить шаблон. `numeric` — колонки, которые пишутся числами, по листам.
pub fn fill_template(
    template: &[u8],
    fills: &[SheetRows],
    cells: &[CellValue],
    numeric: &[(&str, Vec<u32>)],
) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(template)).map_err(|e| format!("шаблон: {e}"))?;
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).map_err(|e| format!("шаблон: {e}"))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| format!("шаблон: {e}"))?;
        files.push((f.name().to_string(), buf));
    }
    let text = |files: &[(String, Vec<u8>)], name: &str| -> Result<String, String> {
        files.iter().find(|(n, _)| n == name)
            .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
            .ok_or_else(|| format!("в шаблоне нет {name}"))
    };
    let mut workbook = text(&files, "xl/workbook.xml")?;
    let mut wb_rels = text(&files, "xl/_rels/workbook.xml.rels")?;

    let mut replaced: BTreeMap<String, String> = BTreeMap::new();
    let mut sheet_names: Vec<String> = fills.iter().map(|f| f.sheet.clone()).collect();
    for c in cells {
        if !sheet_names.contains(&c.sheet) {
            sheet_names.push(c.sheet.clone());
        }
    }
    let mut last_rows: BTreeMap<String, u32> = BTreeMap::new();
    for name in &sheet_names {
        let path = sheet_path(&workbook, &wb_rels, name).ok_or_else(|| format!("в шаблоне нет листа «{name}»"))?;
        let mut xml = match replaced.get(&path) { Some(x) => x.clone(), None => text(&files, &path)? };
        if let Some(fill) = fills.iter().find(|f| &f.sheet == name) {
            let cols = numeric.iter().find(|(s, _)| s == name).map(|(_, c)| c.as_slice()).unwrap_or(&[]);
            let (x, last) = fill_sheet(&xml, fill, cols)?;
            xml = x;
            last_rows.insert(name.clone(), last);
        }
        let mine: Vec<&CellValue> = cells.iter().filter(|c| &c.sheet == name).collect();
        if !mine.is_empty() {
            xml = set_cells(&xml, &mine)?;
        }
        replaced.insert(path, xml);
    }

    // _FilterDatabase: скрытые имена автофильтров — на новую длину листа.
    for (s, e) in elements(&workbook.clone(), "definedName").into_iter().rev() {
        let el = workbook[s..e].to_string();
        if attr(open_tag(&el), "name").as_deref() != Some("_xlnm._FilterDatabase") {
            continue;
        }
        let body = &el[open_tag(&el).len()..el.len() - "</definedName>".len()];
        let Some((sheet, range)) = body.split_once('!') else { continue };
        let Some(last) = last_rows.get(sheet.trim_matches('\'')) else { continue };
        let Some((a, b)) = range.split_once(':') else { continue };
        let letters: String = b.trim_start_matches('$').chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        let new_body = format!("{sheet}!{a}:${letters}${last}");
        let new_el = format!("{}{}</definedName>", open_tag(&el), new_body);
        workbook.replace_range(s..e, &new_el);
    }

    // calcChain — перечень формул, часть которых удалена вместе со строками.
    // Без него Excel строит порядок пересчёта заново; пересчитать при открытии.
    let had_chain = files.iter().any(|(n, _)| n == "xl/calcChain.xml");
    if had_chain {
        files.retain(|(n, _)| n != "xl/calcChain.xml");
        wb_rels = drop_children(&wb_rels, "Relationships", "Relationship", |t| {
            attr(t, "Target").map(|v| v.ends_with("calcChain.xml")).unwrap_or(false)
        });
        let ct = text(&files, "[Content_Types].xml")?;
        let ct = drop_children(&ct, "Types", "Override", |t| {
            attr(t, "PartName").map(|v| v.ends_with("calcChain.xml")).unwrap_or(false)
        });
        replaced.insert("[Content_Types].xml".into(), ct);
    }
    if let Some(&(s, e)) = elements(&workbook, "calcPr").first() {
        let tag = open_tag(&workbook[s..e]).to_string();
        if !tag.contains("fullCalcOnLoad") {
            let new = tag.replacen("<calcPr", "<calcPr fullCalcOnLoad=\"1\"", 1);
            workbook.replace_range(s..s + tag.len(), &new);
        }
    }
    replaced.insert("xl/workbook.xml".into(), workbook);
    replaced.insert("xl/_rels/workbook.xml.rels".into(), wb_rels);

    let mut out = Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut out);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in &files {
            zw.start_file(name.as_str(), opts).map_err(|e| format!("запись xlsx: {e}"))?;
            match replaced.get(name) {
                Some(x) => zw.write_all(x.as_bytes()),
                None => zw.write_all(bytes),
            }.map_err(|e| format!("запись xlsx: {e}"))?;
        }
        zw.finish().map_err(|e| format!("запись xlsx: {e}"))?;
    }
    Ok(out.into_inner())
}

/// Прочитать лист готового файла — для проверок: строки → (колонка → текст).
#[cfg(test)]
pub fn read_sheet(xlsx: &[u8], sheet: &str) -> Result<BTreeMap<u32, BTreeMap<u32, String>>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(xlsx)).map_err(|e| e.to_string())?;
    let mut get = |name: &str| -> Result<String, String> {
        let mut f = archive.by_name(name).map_err(|e| e.to_string())?;
        let mut s = String::new();
        f.read_to_string(&mut s).map_err(|e| e.to_string())?;
        Ok(s)
    };
    let workbook = get("xl/workbook.xml")?;
    let rels = get("xl/_rels/workbook.xml.rels")?;
    let path = sheet_path(&workbook, &rels, sheet).ok_or("нет листа")?;
    let xml = get(&path)?;
    let shared: Vec<String> = get("xl/sharedStrings.xml").map(|ss| {
        elements(&ss, "si").into_iter().map(|(a, b)| {
            let si = &ss[a..b];
            elements(si, "t").into_iter().map(|(x, y)| {
                let t = &si[x..y];
                if t.ends_with("/>") { String::new() }
                else { unescape(&t[open_tag(t).len()..t.len() - 4]) }
            }).collect::<String>()
        }).collect()
    }).unwrap_or_default();
    let mut out = BTreeMap::new();
    for (s, e) in elements(&xml, "row") {
        let row = &xml[s..e];
        let r: u32 = attr(open_tag(row), "r").and_then(|v| v.parse().ok()).unwrap_or(0);
        let mut cells = BTreeMap::new();
        for (cs, ce) in elements(row, "c") {
            let c = &row[cs..ce];
            let cref = attr(open_tag(c), "r").unwrap_or_default();
            let v = if let Some(p) = c.find("<t") {
                let body = &c[p..];
                let st = body.find('>').map(|x| x + 1).unwrap_or(0);
                let en = body.find("</t>").unwrap_or(st);
                unescape(&body[st..en])
            } else if let Some(p) = c.find("<v>") {
                let en = c.find("</v>").unwrap_or(p + 3);
                let v = &c[p + 3..en];
                if attr(open_tag(c), "t").as_deref() == Some("s") {
                    v.parse::<usize>().ok().and_then(|i| shared.get(i).cloned()).unwrap_or_default()
                } else {
                    format!("#{v}")
                }
            } else {
                String::new()
            };
            cells.insert(col_index(&split_ref(&cref).0), v);
        }
        out.insert(r, cells);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAMILIO: &[u8] = include_bytes!("../../db/export/familio_template.xlsx");

    #[test]
    fn letters() {
        assert_eq!(col_letters(1), "A");
        assert_eq!(col_letters(26), "Z");
        assert_eq!(col_letters(27), "AA");
        assert_eq!(col_letters(69), "BQ");
        assert_eq!(col_index("BQ"), 69);
        assert_eq!(col_index("CS"), 97);
    }

    #[test]
    fn fills_familio() {
        let rows = vec![
            vec![Some("1".into()), Some("ГА <Костромской> & области".into()), None, None, None, Some("873".into())],
            vec![Some("2".into()), None, None, None, None, Some("873об".into())],
        ];
        let out = fill_template(FAMILIO, &[
            SheetRows { sheet: "РОЖДЕНИЕ".into(), first_row: 4, rows },
            SheetRows { sheet: "location".into(), first_row: 3, rows: vec![] },
        ], &[
            CellValue { sheet: "about".into(), cell: "B4".into(), value: "Метрические книги с. Борисоглебское".into() },
            CellValue { sheet: "about".into(), cell: "B10".into(), value: "".into() },
        ], &[("РОЖДЕНИЕ", vec![1])]).unwrap();
        let birth = read_sheet(&out, "РОЖДЕНИЕ").unwrap();
        assert_eq!(birth[&2][&1], "№ п/п", "шапка на месте");
        assert_eq!(birth[&4][&1], "#1", "номер числом");
        assert_eq!(birth[&4][&2], "ГА <Костромской> & области");
        assert_eq!(birth[&5][&6], "873об");
        assert!(!birth.contains_key(&6), "примеры образца удалены");
        let loc = read_sheet(&out, "location").unwrap();
        assert!(loc.keys().all(|r| *r < 3), "примеры НП удалены");
        let about = read_sheet(&out, "about").unwrap();
        assert_eq!(about[&4][&2], "Метрические книги с. Борисоглебское");
        assert!(!about[&10].contains_key(&2), "пустое значение — пустая ячейка");
        // Файл снова читается как zip, calcChain нет.
        let mut z = zip::ZipArchive::new(Cursor::new(out.as_slice())).unwrap();
        assert!(z.by_name("xl/calcChain.xml").is_err());
        let mut wb = String::new();
        z.by_name("xl/workbook.xml").unwrap().read_to_string(&mut wb).unwrap();
        assert!(wb.contains("fullCalcOnLoad=\"1\""));
        assert!(wb.contains("РОЖДЕНИЕ!$A$3:$BP$5"), "фильтр растянут на данные: {wb}");
        let mut about_xml = String::new();
        let rels = {
            let mut r = String::new();
            z.by_name("xl/_rels/workbook.xml.rels").unwrap().read_to_string(&mut r).unwrap();
            r
        };
        assert!(!rels.contains("calcChain"));
        let p = sheet_path(&wb, &rels, "about").unwrap();
        z.by_name(&p).unwrap().read_to_string(&mut about_xml).unwrap();
        assert!(!about_xml.contains("ref=\"B10\""), "ссылка образца на профиль убрана");
        assert!(about_xml.contains("ref=\"B24\""), "остальные ссылки на месте");
        let mut ct = String::new();
        z.by_name("[Content_Types].xml").unwrap().read_to_string(&mut ct).unwrap();
        assert!(!ct.contains("calcChain"));
        // Инцидент 01.10.2026: вместе с записью о calcChain пропадали все <Default>.
        for ext in ["rels", "xml", "bin", "vml"] {
            assert!(ct.contains(&format!("<Default Extension=\"{ext}\"")), "нет Default для .{ext}");
        }
        let template_ct = {
            let mut t = zip::ZipArchive::new(Cursor::new(FAMILIO)).unwrap();
            let mut x = String::new();
            t.by_name("[Content_Types].xml").unwrap().read_to_string(&mut x).unwrap();
            x
        };
        assert_eq!(elements(&ct, "Default").len(), elements(&template_ct, "Default").len());
        assert_eq!(elements(&ct, "Override").len() + 1, elements(&template_ct, "Override").len());
    }
}
