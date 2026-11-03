//! Листы PDF (спека 2026-10-20-tiff-i-pdf, спринт 2 от 03.11.2026).
//!
//! Архивы отдают дело одним PDF: у файла Романа («Ф627-255-216 МК
//! 1875г_Чернитово.pdf», 43 страницы, 55 МБ) каждая страница — одна
//! картинка JPEG, текста и векторной графики нет. Роман 20.10: у всех его
//! PDF «каждая страница документа — это одна цельная фотография». Поэтому
//! страницу не рисуем: находим её картинку и отдаём окну байты JPEG как
//! есть. Страница без картинки, с несколькими картинками или с картинкой не
//! в JPEG — лист с пометкой «показать не может».
//!
//! Движка PDF здесь нет — только то, что нужно для поиска картинок:
//! таблица ссылок (обычная и потоком, сжатые объекты), дерево страниц,
//! словари. Таблица битая — места объектов ищутся проходом по файлу.
//!
//! Большой файл читается не целиком (п. 111, 03.11.2026): сначала хвост с
//! таблицей ссылок, дальше — только куски, где лежат нужные объекты. Разбор
//! идёт кругами: наткнулся на непрочитанное — запомнил, что дочитать, круг
//! отбрасывается, куски дочитываются, разбор заново. Картинки страниц
//! (почти весь вес файла) при этом не читаются.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Что показать на странице.
#[derive(Clone, Debug, PartialEq)]
pub enum Page {
    /// Одна картинка JPEG: где в файле лежат её байты и на сколько градусов
    /// по часовой страница повёрнута (/Rotate: 0, 90, 180, 270).
    Jpeg { offset: u64, len: u64, rotate: u16 },
    /// Показать нечего; почему — человеку.
    Unsupported(String),
}

/// Страницы PDF по порядку. Небольшой файл читается целиком, большой —
/// кусками, которые нужны разбору; дальше страница берётся из файла по
/// месту (`read_jpeg`), без перечитывания.
pub fn index_file(path: &Path) -> Result<Vec<Page>, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("файл не читается: {e}"))?;
    let full = f.metadata().map_err(|e| format!("файл не читается: {e}"))?.len() as usize;
    if full <= WHOLE {
        let data = std::fs::read(path).map_err(|e| format!("файл не читается: {e}"))?;
        return pages(&data);
    }
    let read = |a: usize, buf: &mut [u8]| -> Result<(), String> {
        f.seek(SeekFrom::Start(a as u64)).map_err(|e| format!("файл не читается: {e}"))?;
        f.read_exact(buf).map_err(|e| format!("файл не читается: {e}"))
    };
    match sparse(full, read)? {
        Some((pages, _)) => pages,
        // Кругов слишком много (странный файл) — прочитать целиком.
        None => pages(&std::fs::read(path).map_err(|e| format!("файл не читается: {e}"))?),
    }
}

/// Разбор кругами по кускам файла длиной `full`; `read` читает кусок с
/// места. Итог разбора и сколько байт прочитано; None — кругов не хватило.
#[allow(clippy::type_complexity)]
fn sparse(full: usize, mut read: impl FnMut(usize, &mut [u8]) -> Result<(), String>)
    -> Result<Option<(Result<Vec<Page>, String>, usize)>, String> {
    // Непрочитанное — нули: место под весь файл (как и при чтении целиком —
    // выигрыш в чтении с диска, не в памяти), байты — только нужные.
    let mut data = vec![0u8; full];
    let mut have: Vec<(usize, usize)> = Vec::new();
    let mut want = vec![(0, BLOCK), (full.saturating_sub(BLOCK), full)];
    let mut total = 0;
    for _ in 0..ROUNDS {
        let mut fresh = false;
        for (a, b) in want.drain(..) {
            let a = (a - a % BLOCK).min(full);
            let b = b.max(a + BLOCK).min(full);
            if a >= b || covered(&have, a, b) {
                continue;
            }
            read(a, &mut data[a..b])?;
            total += b - a;
            fresh = true;
            have.push((a, b));
            have = merged(have);
        }
        // Круг ничего не дочитал, а разбор снова просит — дальше кругами не
        // сойдётся: целиком.
        if !fresh && total > 0 {
            return Ok(None);
        }
        let (result, more) = pages_in(&data, &have);
        if more.is_empty() {
            return Ok(Some((result, total)));
        }
        want = more;
    }
    Ok(None)
}

/// Есть ли у JPEG своя пометка EXIF: сегменты от начала до картинки (SOS).
fn has_exif(jpeg: &[u8]) -> bool {
    let mut i = 2;
    while i + 4 <= jpeg.len() && jpeg[i] == 0xFF {
        let marker = jpeg[i + 1];
        if marker == 0xDA {
            return false;
        }
        let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
        if marker == 0xE1 && jpeg.get(i + 4..i + 10) == Some(b"Exif\0\0") {
            return true;
        }
        i += 2 + len;
    }
    false
}

/// Файл не больше этого читается целиком.
const WHOLE: usize = 8 << 20;
/// Кусок дочитывания.
const BLOCK: usize = 256 << 10;
/// Кругов разбора большого файла, потом — целиком.
const ROUNDS: usize = 64;
/// Запас до края прочитанного: объект, кончившийся у самого края, мог
/// оборваться (число «12» от «1234», словарь без слова «stream» после него).
const MARGIN: usize = 64;
/// Ошибка круга, которому не хватило прочитанного; человеку не показывается.
const MORE: &str = "PDF дочитывается";

/// Прочитан ли весь кусок [a, b).
fn covered(have: &[(usize, usize)], a: usize, b: usize) -> bool {
    have.iter().any(|&(x, y)| x <= a && b <= y)
}

/// Прочитанные куски — без наложений, по порядку.
fn merged(mut have: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    have.sort();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (a, b) in have {
        match out.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// Страница повёрнута (/Rotate) — картинке пометка EXIF «как повернуть»:
/// окно (WebView) показывает JPEG с учётом неё, перекодировать не нужно
/// (п. 109, 03.11.2026). Своя пометка — первой, сразу после начала JPEG:
/// окно берёт первую (проверено 03.11 в Chromium и WebKit, и в картинке, и в
/// WebGL просмотрщика). У JPEG своя пометка EXIF, а страница не повёрнута —
/// пометка «как есть» (1): программы просмотра PDF чужую пометку не
/// учитывают, и окно не должно (ревьюер 03.11.2026).
pub fn oriented(jpeg: Vec<u8>, rotate: u16) -> Vec<u8> {
    if !jpeg.starts_with(&[0xFF, 0xD8]) {
        return jpeg;
    }
    let orientation: u16 = match rotate {
        90 => 6,
        180 => 3,
        270 => 8,
        _ if has_exif(&jpeg) => 1,
        _ => return jpeg,
    };
    let mut seg = vec![0xFF, 0xE1, 0, 34];
    seg.extend_from_slice(b"Exif\0\0");
    // TIFF: порядок байт «MM», 42, первый каталог — со смещения 8.
    seg.extend_from_slice(&[b'M', b'M', 0, 42, 0, 0, 0, 8]);
    // Каталог из одной записи: 0x0112 Orientation, SHORT, 1 значение.
    seg.extend_from_slice(&[0, 1, 0x01, 0x12, 0, 3, 0, 0, 0, 1]);
    seg.extend_from_slice(&orientation.to_be_bytes());
    seg.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    let mut out = Vec::with_capacity(jpeg.len() + seg.len());
    out.extend_from_slice(&jpeg[..2]);
    out.extend_from_slice(&seg);
    out.extend_from_slice(&jpeg[2..]);
    out
}

/// Байты JPEG страницы из файла.
pub fn read_jpeg(path: &Path, offset: u64, len: u64) -> Result<Vec<u8>, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("файл не открывается: {e}"))?;
    f.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; usize::try_from(len).map_err(|_| "картинка страницы слишком велика".to_string())?];
    f.read_exact(&mut buf).map_err(|e| format!("картинка страницы не читается: {e}"))?;
    Ok(buf)
}

/// Страницы PDF из байтов файла.
pub fn pages(data: &[u8]) -> Result<Vec<Page>, String> {
    pages_in(data, &[(0, data.len())]).0
}

/// Один круг разбора: `have` — прочитанные куски `data`. Второе — что
/// дочитать; не пусто — результат круга не в счёт.
fn pages_in(data: &[u8], have: &[(usize, usize)]) -> (Result<Vec<Page>, String>, Vec<(usize, usize)>) {
    let mut doc = Doc { data, have, want: Vec::new(), at: HashMap::new(), streams: HashMap::new(), busy: HashSet::new() };
    let result = doc.all_pages();
    (result, doc.want)
}

// ---------------------------------------------------------------------------
//  Объекты PDF
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Obj {
    Null,
    Bool(bool),
    Num(f64),
    Str,
    Name(String),
    Arr(Vec<Obj>),
    Dict(Vec<(String, Obj)>),
    Ref(u32),
    /// Поток: словарь и место начала данных в своём буфере.
    Stream(Vec<(String, Obj)>, usize),
}

impl Obj {
    fn get(&self, key: &str) -> Option<&Obj> {
        match self {
            Obj::Dict(d) | Obj::Stream(d, _) => d.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn name(&self) -> Option<&str> {
        if let Obj::Name(n) = self { Some(n) } else { None }
    }
    fn num(&self) -> Option<f64> {
        if let Obj::Num(n) = self { Some(*n) } else { None }
    }
}

const ENCRYPTED: &str = "PDF зашифрован (защищён паролем) — его страницы программа показать не может";
/// Распакованный поток служебных данных больше этого — битый или злонамеренный файл.
const INFLATE_LIMIT: u64 = 64 << 20;

/// Глубина вложения массивов и словарей: битый файл не должен уронить
/// программу переполнением стека.
const DEEP: usize = 200;

struct Lexer<'a> {
    b: &'a [u8],
    i: usize,
}

fn is_ws(c: u8) -> bool {
    matches!(c, 0 | 9 | 10 | 12 | 13 | 32)
}

fn is_delim(c: u8) -> bool {
    matches!(c, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

impl<'a> Lexer<'a> {
    fn new(b: &'a [u8], i: usize) -> Self {
        Lexer { b, i }
    }

    fn skip_ws(&mut self) {
        while self.i < self.b.len() {
            let c = self.b[self.i];
            if is_ws(c) {
                self.i += 1;
            } else if c == b'%' {
                while self.i < self.b.len() && self.b[self.i] != b'\n' && self.b[self.i] != b'\r' {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn starts(&self, s: &[u8]) -> bool {
        self.b.get(self.i..self.i + s.len()) == Some(s)
    }

    /// Слово до пробела или разделителя.
    fn word(&mut self) -> &'a [u8] {
        let from = self.i;
        while self.i < self.b.len() && !is_ws(self.b[self.i]) && !is_delim(self.b[self.i]) {
            self.i += 1;
        }
        &self.b[from..self.i]
    }

    fn int(&mut self) -> Option<u64> {
        self.skip_ws();
        let from = self.i;
        while self.i < self.b.len() && self.b[self.i].is_ascii_digit() {
            self.i += 1;
        }
        if self.i == from {
            return None;
        }
        std::str::from_utf8(&self.b[from..self.i]).ok()?.parse().ok()
    }

    fn obj(&mut self, depth: usize) -> Result<Obj, String> {
        if depth > DEEP {
            return Err("слишком глубокое вложение".into());
        }
        self.skip_ws();
        let c = self.peek().ok_or("файл оборвался")?;
        match c {
            b'/' => {
                self.i += 1;
                let raw = self.word();
                Ok(Obj::Name(name_text(raw)))
            }
            b'<' if self.starts(b"<<") => {
                self.i += 2;
                let mut d = Vec::new();
                loop {
                    self.skip_ws();
                    if self.starts(b">>") {
                        self.i += 2;
                        break;
                    }
                    match self.obj(depth + 1)? {
                        Obj::Name(k) => {
                            let v = self.obj(depth + 1)?;
                            d.push((k, v));
                        }
                        _ => return Err("ключ словаря — не имя".into()),
                    }
                }
                Ok(Obj::Dict(d))
            }
            b'<' => {
                let Some(end) = find(self.b, b">", self.i) else {
                    // До конца прочитанного: разбор кусками увидит край и
                    // дочитает (ревьюер 03.11.2026).
                    self.i = self.b.len();
                    return Err("строка не закрыта".into());
                };
                self.i = end + 1;
                Ok(Obj::Str)
            }
            b'(' => {
                let mut level = 0usize;
                while self.i < self.b.len() {
                    match self.b[self.i] {
                        b'\\' => self.i += 1,
                        b'(' => level += 1,
                        b')' => {
                            level -= 1;
                            if level == 0 {
                                self.i += 1;
                                return Ok(Obj::Str);
                            }
                        }
                        _ => {}
                    }
                    self.i += 1;
                }
                Err("строка не закрыта".into())
            }
            b'[' => {
                self.i += 1;
                let mut a = Vec::new();
                loop {
                    self.skip_ws();
                    if self.peek() == Some(b']') {
                        self.i += 1;
                        break;
                    }
                    a.push(self.obj(depth + 1)?);
                }
                Ok(Obj::Arr(a))
            }
            b'0'..=b'9' | b'+' | b'-' | b'.' => {
                let from = self.i;
                let w = self.word();
                let n: f64 = std::str::from_utf8(w).ok().and_then(|t| t.parse().ok())
                    .ok_or_else(|| format!("не число в позиции {from}"))?;
                // «12 0 R» — ссылка на объект.
                if w.iter().all(u8::is_ascii_digit) {
                    let back = self.i;
                    if self.int().is_some() {
                        self.skip_ws();
                        if self.peek() == Some(b'R') && self.b.get(self.i + 1).map_or(true, |c| is_ws(*c) || is_delim(*c)) {
                            self.i += 1;
                            return Ok(Obj::Ref(n as u32));
                        }
                    }
                    self.i = back;
                }
                Ok(Obj::Num(n))
            }
            _ => {
                let w = self.word();
                match w {
                    b"true" => Ok(Obj::Bool(true)),
                    b"false" => Ok(Obj::Bool(false)),
                    b"null" => Ok(Obj::Null),
                    _ => Err(format!("непонятное слово в позиции {}", self.i)),
                }
            }
        }
    }

    /// «N G obj … endobj»; у потока — место начала данных.
    fn indirect(&mut self) -> Result<(u32, Obj), String> {
        let num = self.int().ok_or("нет номера объекта")? as u32;
        self.int().ok_or("нет поколения объекта")?;
        self.skip_ws();
        if self.word() != b"obj" {
            return Err("нет слова obj".into());
        }
        let obj = self.obj(0)?;
        self.skip_ws();
        if let Obj::Dict(d) = obj {
            if self.starts(b"stream") {
                self.i += 6;
                if self.starts(b"\r\n") {
                    self.i += 2;
                } else if matches!(self.peek(), Some(b'\n') | Some(b'\r')) {
                    self.i += 1;
                }
                return Ok((num, Obj::Stream(d, self.i)));
            }
            return Ok((num, Obj::Dict(d)));
        }
        Ok((num, obj))
    }
}

/// Имя с «#xx» — байтом в шестнадцатеричной записи.
fn name_text(raw: &[u8]) -> String {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'#' && i + 2 < raw.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&raw[i + 1..i + 3]).unwrap_or("zz"), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(raw[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= hay.len() || needle.is_empty() {
        return None;
    }
    hay[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

// ---------------------------------------------------------------------------
//  Документ: где лежат объекты
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Loc {
    /// Место в файле.
    At(usize),
    /// В сжатом потоке объектов: номер потока и порядковый номер в нём.
    In(u32, usize),
}

struct Doc<'a> {
    data: &'a [u8],
    /// Прочитанные куски `data` (у файла в памяти — весь он).
    have: &'a [(usize, usize)],
    /// Что дочитать: разбор упёрся в непрочитанное.
    want: Vec<(usize, usize)>,
    at: HashMap<u32, Loc>,
    /// Распакованные потоки объектов: место первого объекта (/First) и тело.
    streams: HashMap<u32, (usize, Vec<u8>)>,
    /// Потоки объектов, которые сейчас распаковываются: /Length потока,
    /// указывающий внутрь него же, иначе ходил бы по кругу до переполнения стека.
    busy: HashSet<u32>,
}

impl<'a> Doc<'a> {
    fn all_pages(&mut self) -> Result<Vec<Page>, String> {
        let data = self.data;
        if !data.starts_with(b"%PDF") && find(&data[..self.avail(0)], b"%PDF", 0).map_or(true, |i| i > 1024) {
            return Err("это не PDF".into());
        }
        let root = match self.read_xref() {
            Ok(root) => root,
            Err(e) if e == ENCRYPTED => return Err(e),
            // Таблице не хватило прочитанного — дочитать, а не искать объекты
            // проходом по всему файлу.
            Err(_) if !self.want.is_empty() => return Err(MORE.into()),
            // Таблица ссылок битая или её нет — места объектов по самому файлу.
            Err(_) => self.rebuild()?,
        };
        let catalog = self.resolve(&root, 0)?;
        let pages_ref = catalog.get("Pages").ok_or("в PDF нет перечня страниц")?.clone();
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        self.walk(&pages_ref, None, None, &mut out, &mut seen, 0)?;
        if out.is_empty() {
            return Err("в PDF нет страниц".into());
        }
        Ok(out)
    }

    /// Конец прочитанного куска, где лежит `at` (не прочитан — сам `at`).
    fn avail(&self, at: usize) -> usize {
        self.have.iter().find(|&&(a, b)| a <= at && at < b).map_or(at, |&(_, b)| b)
    }

    /// Чтение с места `at` — только по прочитанному: дальше — «конец файла».
    fn lex(&self, at: usize) -> Lexer<'a> {
        let data: &'a [u8] = self.data;
        let end = self.avail(at).max(at).min(data.len());
        Lexer::new(&data[..end], at)
    }

    /// Чтение с `at` дошло до края прочитанного (`reached` — где встало):
    /// дочитать следующий кусок. true — круг не в счёт.
    fn short(&mut self, at: usize, reached: usize) -> bool {
        let end = self.avail(at).max(at);
        if end < self.data.len() && reached + MARGIN > end {
            self.want.push((end, end + BLOCK));
            return true;
        }
        false
    }

    /// Кусок [a, b) прочитан; нет — запомнить, что дочитать.
    fn ensure(&mut self, a: usize, b: usize) -> bool {
        if self.avail(a) >= b {
            return true;
        }
        self.want.push((a, b));
        false
    }

    /// Таблица ссылок от «startxref» по цепочке /Prev; возвращает /Root.
    fn read_xref(&mut self) -> Result<Obj, String> {
        let tail = self.data.len().saturating_sub(4096);
        if !self.ensure(tail, self.data.len()) {
            return Err(MORE.into());
        }
        let sx = rfind(&self.data[tail..], b"startxref").ok_or("нет startxref")? + tail;
        let mut lx = self.lex(sx + 9);
        let mut next = Some(lx.int().ok_or("startxref без числа")? as usize);
        let mut root = None;
        let mut seen = HashSet::new();
        while let Some(off) = next.take() {
            if !seen.insert(off) || off >= self.data.len() {
                break;
            }
            let trailer = self.xref_section(off)?;
            if root.is_none() {
                root = trailer.get("Root").cloned();
            }
            // Картинки зашифрованного PDF — не JPEG, а шифр: окно получило бы мусор.
            if trailer.get("Encrypt").is_some() {
                return Err(ENCRYPTED.into());
            }
            // Смешанный файл: обычная таблица и таблица потоком.
            if let Some(stm) = trailer.get("XRefStm").and_then(Obj::num) {
                let _ = self.xref_section(stm as usize);
            }
            next = trailer.get("Prev").and_then(Obj::num).map(|p| p as usize);
        }
        let root = root.ok_or("в таблице ссылок нет /Root")?;
        // Проверка, что таблица не врёт: каталог читается.
        self.resolve(&root, 0)?;
        Ok(root)
    }

    /// Одна таблица ссылок; возвращает её словарь (trailer). Уже известные
    /// объекты не перезаписываются: первой читается самая новая таблица.
    fn xref_section(&mut self, off: usize) -> Result<Obj, String> {
        let mut lx = self.lex(off);
        let r = self.xref_parse(&mut lx);
        if self.short(off, lx.i) {
            return Err(MORE.into());
        }
        r
    }

    fn xref_parse(&mut self, lx: &mut Lexer<'a>) -> Result<Obj, String> {
        lx.skip_ws();
        if lx.starts(b"xref") {
            lx.i += 4;
            loop {
                lx.skip_ws();
                if lx.starts(b"trailer") {
                    lx.i += 7;
                    return lx.obj(0);
                }
                let first = lx.int().ok_or("таблица ссылок не читается")? as u32;
                let count = lx.int().ok_or("таблица ссылок не читается")? as u32;
                for k in 0..count {
                    let pos = lx.int().ok_or("строка таблицы ссылок не читается")? as usize;
                    lx.int().ok_or("строка таблицы ссылок не читается")?;
                    lx.skip_ws();
                    let kind = lx.peek();
                    lx.i += 1;
                    if kind == Some(b'n') {
                        self.at.entry(first + k).or_insert(Loc::At(pos));
                    }
                }
            }
        }
        // Таблица ссылок потоком (PDF 1.5).
        let (_, obj) = lx.indirect()?;
        let Obj::Stream(_, _) = &obj else { return Err("таблица ссылок не найдена".into()) };
        let raw = self.stream_bytes(&obj, self.data)?;
        let w: Vec<usize> = match obj.get("W") {
            Some(Obj::Arr(a)) => a.iter().map(|x| x.num().unwrap_or(0.0) as usize).collect(),
            _ => return Err("у таблицы ссылок нет /W".into()),
        };
        if w.len() != 3 || w.iter().any(|x| *x > 8) {
            return Err("у таблицы ссылок странная /W".into());
        }
        let size = obj.get("Size").and_then(Obj::num).unwrap_or(0.0) as u32;
        let ranges: Vec<u32> = match obj.get("Index") {
            Some(Obj::Arr(a)) => a.iter().map(|x| x.num().unwrap_or(0.0) as u32).collect(),
            _ => vec![0, size],
        };
        let row = w[0] + w[1] + w[2];
        if row == 0 {
            return Err("у таблицы ссылок пустая строка".into());
        }
        let field = |r: &[u8], from: usize, n: usize| r[from..from + n].iter().fold(0u64, |acc, b| acc << 8 | *b as u64);
        let mut rows = raw.chunks_exact(row);
        for pair in ranges.chunks(2) {
            let (first, count) = (pair[0], *pair.get(1).unwrap_or(&0));
            for k in 0..count {
                let Some(r) = rows.next() else { break };
                let kind = if w[0] == 0 { 1 } else { field(r, 0, w[0]) };
                let a = field(r, w[0], w[1]);
                let b = field(r, w[0] + w[1], w[2]);
                let loc = match kind {
                    1 => Loc::At(a as usize),
                    2 => Loc::In(a as u32, b as usize),
                    _ => continue,
                };
                self.at.entry(first + k).or_insert(loc);
            }
        }
        Ok(obj)
    }

    /// Таблицы нет или она битая: объекты — по «N G obj» в самом файле,
    /// каталог — по /Root последнего trailer или по /Type /Catalog.
    fn rebuild(&mut self) -> Result<Obj, String> {
        // Проход по всему файлу — нужен весь файл.
        if !self.ensure(0, self.data.len()) {
            return Err(MORE.into());
        }
        self.at.clear();
        let d = self.data;
        let mut i = 0;
        while let Some(p) = find(d, b"obj", i) {
            i = p + 3;
            // Перед «obj» — «N G» с начала строки.
            let mut s = p;
            let mut parts = 0;
            while s > 0 && parts < 2 {
                while s > 0 && is_ws(d[s - 1]) { s -= 1; }
                let end = s;
                while s > 0 && d[s - 1].is_ascii_digit() { s -= 1; }
                if s == end { break; }
                parts += 1;
            }
            if parts == 2 && (s == 0 || is_ws(d[s - 1])) && d.get(p + 3).map_or(true, |c| is_ws(*c) || is_delim(*c)) {
                if let Some(num) = Lexer::new(d, s).int() {
                    self.at.insert(num as u32, Loc::At(s));
                }
            }
        }
        if let Some(t) = rfind(d, b"trailer") {
            if let Ok(tr) = Lexer::new(d, t + 7).obj(0) {
                if let Some(root) = tr.get("Root") {
                    if self.resolve(root, 0).is_ok() {
                        return Ok(root.clone());
                    }
                }
            }
        }
        let nums: Vec<u32> = self.at.keys().copied().collect();
        for n in nums {
            if let Ok(o) = self.load(n) {
                if o.get("Type").and_then(Obj::name) == Some("Catalog") {
                    return Ok(Obj::Ref(n));
                }
            }
        }
        Err("в PDF не найден каталог страниц".into())
    }

    fn load(&mut self, num: u32) -> Result<Obj, String> {
        match *self.at.get(&num).ok_or_else(|| format!("нет объекта {num}"))? {
            Loc::At(off) => {
                if off >= self.data.len() {
                    return Err(format!("объект {num} за концом файла"));
                }
                let mut lx = self.lex(off);
                let r = lx.indirect();
                if self.short(off, lx.i) {
                    return Err(MORE.into());
                }
                let (got, obj) = r?;
                if got != num {
                    return Err(format!("на месте объекта {num} — объект {got}"));
                }
                Ok(obj)
            }
            Loc::In(stm, idx) => {
                if !self.streams.contains_key(&stm) {
                    let holder = match self.at.get(&stm) {
                        Some(&Loc::At(off)) => {
                            let mut lx = self.lex(off);
                            let r = lx.indirect();
                            if self.short(off, lx.i) {
                                return Err(MORE.into());
                            }
                            r?.1
                        }
                        _ => return Err(format!("нет потока объектов {stm}")),
                    };
                    if !self.busy.insert(stm) {
                        return Err(format!("поток объектов {stm} ссылается сам на себя"));
                    }
                    let raw = self.stream_bytes(&holder, self.data);
                    self.busy.remove(&stm);
                    let raw = raw?;
                    let first = holder.get("First").and_then(Obj::num).unwrap_or(0.0) as usize;
                    self.streams.insert(stm, (first, raw));
                }
                // В начале потока — пары «номер место», объекты — с /First.
                let (first, raw) = (self.streams[&stm].0, &self.streams[&stm].1);
                let mut lx = Lexer::new(raw, 0);
                let mut place = None;
                for k in 0..=idx {
                    let n = lx.int().ok_or("поток объектов не читается")?;
                    let p = lx.int().ok_or("поток объектов не читается")?;
                    if k == idx {
                        if n as u32 != num {
                            return Err(format!("в потоке объектов на месте {num} — {n}"));
                        }
                        place = Some(p as usize);
                    }
                }
                let at = first + place.unwrap_or(0);
                if at >= raw.len() {
                    return Err(format!("объект {num} за концом потока"));
                }
                Lexer::new(raw, at).obj(0)
            }
        }
    }

    /// Объект по ссылке (ссылки на ссылки — тоже); не ссылка — он сам.
    fn resolve(&mut self, o: &Obj, depth: usize) -> Result<Obj, String> {
        match o {
            Obj::Ref(n) if depth < 16 => {
                let got = self.load(*n)?;
                self.resolve(&got, depth + 1)
            }
            Obj::Ref(_) => Err("ссылки по кругу".into()),
            other => Ok(other.clone()),
        }
    }

    /// Длина данных потока: /Length числом или ссылкой.
    fn stream_len(&mut self, s: &Obj, start: usize, buf: &[u8]) -> Result<usize, String> {
        // Длина ссылкой на объект, которого ещё нет (таблица ссылок потоком
        // читается сама) или он битый, — как длины нет.
        let len = match s.get("Length") {
            Some(l) => self.resolve(&l.clone(), 0).ok().and_then(|o| o.num()).map(|n| n as usize),
            None => None,
        };
        // /Length 1e30 — не сложение с переполнением, а «длина врёт» (ревьюер 03.11).
        match len {
            Some(n) if start.checked_add(n).is_some_and(|e| e <= buf.len()) => Ok(n),
            // Длины нет или она врёт — до «endstream» (в прочитанном).
            _ => {
                let seen = self.avail(start).max(start).min(buf.len());
                let Some(end) = find(&buf[..seen], b"endstream", start) else {
                    if seen < buf.len() {
                        self.want.push((seen, seen + BLOCK));
                        return Err(MORE.into());
                    }
                    return Err("поток не закрыт".into());
                };
                let mut e = end;
                while e > start && matches!(buf[e - 1], b'\r' | b'\n') { e -= 1; }
                Ok(e - start)
            }
        }
    }

    /// Данные потока, распакованные (/FlateDecode, с предсказателем PNG).
    fn stream_bytes(&mut self, s: &Obj, buf: &[u8]) -> Result<Vec<u8>, String> {
        let Obj::Stream(_, start) = s else { return Err("не поток".into()) };
        let n = self.stream_len(s, *start, buf)?;
        if !self.ensure(*start, *start + n) {
            return Err(MORE.into());
        }
        let raw = &buf[*start..*start + n];
        let filters: Vec<String> = match s.get("Filter") {
            Some(Obj::Name(f)) => vec![f.clone()],
            Some(Obj::Arr(a)) => a.iter().filter_map(|x| x.name().map(str::to_string)).collect(),
            _ => vec![],
        };
        let mut out = raw.to_vec();
        for f in &filters {
            if f != "FlateDecode" {
                return Err(format!("сжатие {f} не поддерживается"));
            }
            let mut z = Vec::new();
            flate2::read::ZlibDecoder::new(&out[..]).take(INFLATE_LIMIT).read_to_end(&mut z).map_err(|e| format!("поток не распаковался: {e}"))?;
            out = z;
        }
        let parms = match s.get("DecodeParms") {
            Some(Obj::Arr(a)) => a.first().cloned(),
            other => other.cloned(),
        };
        if let Some(p) = parms {
            let predictor = p.get("Predictor").and_then(Obj::num).unwrap_or(1.0) as u32;
            if predictor >= 10 {
                let cols = p.get("Columns").and_then(Obj::num).unwrap_or(1.0) as usize;
                out = unpredict_png(&out, cols.max(1))?;
            }
        }
        Ok(out)
    }

    /// Обход дерева страниц; /Resources и /Rotate наследуются от родителей.
    fn walk(&mut self, node: &Obj, inherited: Option<Obj>, turn: Option<f64>, out: &mut Vec<Page>,
            seen: &mut HashSet<u32>, depth: usize) -> Result<(), String> {
        if depth > 64 {
            return Err("дерево страниц слишком глубокое".into());
        }
        if let Obj::Ref(n) = node {
            if !seen.insert(*n) {
                return Ok(());
            }
        }
        let d = self.resolve(node, 0)?;
        let res = d.get("Resources").cloned().or(inherited);
        let turn = match d.get("Rotate") {
            Some(r) => self.resolve(&r.clone(), 0).ok().and_then(|o| o.num()).or(turn),
            None => turn,
        };
        if let Some(Obj::Arr(kids)) = d.get("Kids").map(|k| self.resolve(k, 0)).transpose()? {
            for kid in kids {
                // Битая ссылка на одну страницу — лист с пометкой, а не весь
                // PDF нечитаемым (ревьюер 03.11.2026).
                if let Err(why) = self.walk(&kid, res.clone(), turn, out, seen, depth + 1) {
                    if depth + 1 > 64 { return Err(why); }
                    out.push(Page::Unsupported(format!("страница не читается: {why}")));
                }
            }
            return Ok(());
        }
        // /Rotate — кратно 90, может быть отрицательным: −90 — то же, что 270.
        // Не кратный 90 (так писать нельзя) округляется к нулю: 135 → 90.
        let rotate = (turn.unwrap_or(0.0) as i64 / 90 * 90).rem_euclid(360) as u16;
        out.push(self.page(res, rotate).unwrap_or_else(Page::Unsupported));
        Ok(())
    }

    /// Картинка страницы — если она одна и в JPEG. Рядом с ней может быть
    /// мелкий значок (штамп архива): картинка, у которой все остальные меньше
    /// двадцатой доли её площади, — та самая (п. 110, 03.11.2026).
    fn page(&mut self, res: Option<Obj>, rotate: u16) -> Result<Page, String> {
        let none = || "на странице нет картинки".to_string();
        let res = self.resolve(&res.ok_or_else(none)?, 0)?;
        let xobj = self.resolve(res.get("XObject").ok_or_else(none)?, 0)?;
        let Obj::Dict(items) = xobj else { return Err(none()) };
        let mut images = Vec::new();
        for (_, v) in items {
            let o = self.resolve(&v, 0)?;
            match o.get("Subtype").and_then(Obj::name) {
                Some("Image") => images.push(o),
                // Вложенный рисунок (Form) — не «одна цельная фотография».
                Some(_) => return Err("страница — не одна картинка".into()),
                None => {}
            }
        }
        if images.is_empty() {
            return Err(none());
        }
        let mut areas = Vec::new();
        for im in &images {
            let side = |doc: &mut Self, k: &str| im.get(k).cloned().and_then(|v| doc.resolve(&v, 0).ok()).and_then(|o| o.num()).unwrap_or(0.0);
            let area = side(self, "Width") * side(self, "Height");
            areas.push(area);
        }
        let main = (0..images.len()).max_by(|&a, &b| areas[a].total_cmp(&areas[b])).unwrap_or(0);
        let small = (0..images.len()).all(|k| k == main || areas[k] * 20.0 <= areas[main]);
        if images.len() > 1 && (!small || areas[main] <= 0.0) {
            return Err(format!("на странице {} картинки", images.len()));
        }
        let img = &images[main];
        let jpeg = match img.get("Filter") {
            Some(Obj::Name(f)) => f == "DCTDecode",
            Some(Obj::Arr(a)) => a.len() == 1 && a[0].name() == Some("DCTDecode"),
            _ => false,
        };
        if !jpeg {
            return Err("картинка страницы — не JPEG".into());
        }
        let Obj::Stream(_, start) = img else { return Err(none()) };
        let start = *start;
        let len = self.stream_len(img, start, self.data)?;
        if !self.ensure(start, start + 2) {
            return Err(MORE.into());
        }
        if self.data.get(start..start + 2) != Some(&[0xFF, 0xD8][..]) {
            return Err("картинка страницы — не JPEG".into());
        }
        Ok(Page::Jpeg { offset: start as u64, len: len as u64, rotate })
    }
}

/// Предсказатель PNG (/Predictor 10–15): у каждой строки — байт способа.
fn unpredict_png(data: &[u8], cols: usize) -> Result<Vec<u8>, String> {
    let row = cols;
    let mut out = Vec::with_capacity(data.len());
    let mut prev = vec![0u8; row];
    for chunk in data.chunks(row + 1) {
        if chunk.len() < row + 1 {
            break;
        }
        let (kind, src) = (chunk[0], &chunk[1..]);
        let mut cur = vec![0u8; row];
        for i in 0..row {
            let left = if i > 0 { cur[i - 1] } else { 0 };
            let up = prev[i];
            let ul = if i > 0 { prev[i - 1] } else { 0 };
            cur[i] = src[i].wrapping_add(match kind {
                0 => 0,
                1 => left,
                2 => up,
                3 => ((left as u16 + up as u16) / 2) as u8,
                4 => {
                    let p = left as i16 + up as i16 - ul as i16;
                    let (pa, pb, pc) = ((p - left as i16).abs(), (p - up as i16).abs(), (p - ul as i16).abs());
                    if pa <= pb && pa <= pc { left } else if pb <= pc { up } else { ul }
                }
                _ => return Err("неизвестный предсказатель строки".into()),
            });
        }
        out.extend_from_slice(&cur);
        prev = cur;
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Крошечный настоящий JPEG (1×1, серый) — данные страницы.
    pub const JPEG: &[u8] = &[
        0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06, 0x07, 0x06, 0x05, 0x08, 0x07, 0x07, 0x07, 0x09,
        0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D, 0x0C, 0x0B, 0x0B, 0x0C, 0x19, 0x12, 0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F,
        0x1E, 0x1D, 0x1A, 0x1C, 0x1C, 0x20, 0x24, 0x2E, 0x27, 0x20, 0x22, 0x2C, 0x23, 0x1C, 0x1C, 0x28, 0x37, 0x29,
        0x2C, 0x30, 0x31, 0x34, 0x34, 0x34, 0x1F, 0x27, 0x39, 0x3D, 0x38, 0x32, 0x3C, 0x2E, 0x33, 0x34, 0x32, 0xFF,
        0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00, 0x14, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x09,
        0xFF, 0xC4, 0x00, 0x14, 0x10, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0x2A, 0x9F, 0xFF, 0xD9,
    ];

    /// PDF из объектов «как пишет сканер»: обычная таблица ссылок.
    pub fn sample_pdf(objs: &[Vec<u8>], root: u32) -> Vec<u8> {
        let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offs = Vec::new();
        for (k, body) in objs.iter().enumerate() {
            offs.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", k + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
        for o in &offs {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root {root} 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
        out
    }

    fn image(len_ref: Option<u32>) -> Vec<u8> {
        let len = match len_ref {
            Some(r) => format!("{r} 0 R"),
            None => JPEG.len().to_string(),
        };
        let mut b = format!("<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /DCTDecode /Length {len} >>\nstream\r\n").into_bytes();
        b.extend_from_slice(JPEG);
        b.extend_from_slice(b"\r\nendstream");
        b
    }

    /// Три страницы: картинка, страница без картинки, картинка с /Length
    /// объектом и /Resources от родителя.
    pub fn three_pages() -> Vec<u8> {
        let objs: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Count 3 /Kids [3 0 R 5 0 R 6 0 R] /Resources << /XObject << /I0 8 0 R >> >> >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << /XObject << /I0 4 0 R >> >> >>".to_vec(),
            image(None),
            b"<< /Type /Page /Parent 2 0 R /Resources << /Font << >> >> >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
            JPEG.len().to_string().into_bytes(),
            image(Some(7)),
        ];
        sample_pdf(&objs, 1)
    }

    #[test]
    fn finds_jpeg_of_each_page() {
        let pdf = three_pages();
        let got = pages(&pdf).unwrap();
        assert_eq!(got.len(), 3);
        for k in [0, 2] {
            let Page::Jpeg { offset, len, .. } = got[k] else { panic!("страница {} — {:?}", k + 1, got[k]) };
            assert_eq!(&pdf[offset as usize..(offset + len) as usize], JPEG, "байты JPEG страницы {}", k + 1);
        }
        assert_eq!(got[1], Page::Unsupported("на странице нет картинки".into()));
    }

    /// Страница с картинками заданных размеров (ширина, высота) и /Rotate.
    fn page_with(sizes: &[(u32, u32)], rotate: Option<i32>, parent_rotate: Option<i32>) -> Vec<u8> {
        let mut objs: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            format!("<< /Type /Pages /Count 1 /Kids [3 0 R] {} >>",
                    parent_rotate.map(|r| format!("/Rotate {r}")).unwrap_or_default()).into_bytes(),
        ];
        let names: Vec<String> = (0..sizes.len()).map(|k| format!("/I{k} {} 0 R", 4 + k)).collect();
        objs.push(format!("<< /Type /Page /Parent 2 0 R {} /Resources << /XObject << {} >> >> >>",
                          rotate.map(|r| format!("/Rotate {r}")).unwrap_or_default(), names.join(" ")).into_bytes());
        for (w, h) in sizes {
            let mut b = format!("<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /Filter /DCTDecode /Length {} >>\nstream\r\n", JPEG.len()).into_bytes();
            b.extend_from_slice(JPEG);
            b.extend_from_slice(b"\r\nendstream");
            objs.push(b);
        }
        sample_pdf(&objs, 1)
    }

    #[test]
    fn rotated_page() {
        // п. 109: /Rotate у страницы и от родителя, −90 — то же, что 270.
        for (own, parent, want) in [(Some(90), None, 90), (None, Some(-90), 270), (Some(180), Some(90), 180), (None, None, 0)] {
            let got = pages(&page_with(&[(100, 100)], own, parent)).unwrap();
            assert!(matches!(got[0], Page::Jpeg { rotate, .. } if rotate == want), "{own:?}/{parent:?}: {:?}", got[0]);
        }
        // Пометка EXIF: 90 → 6, сразу после начала JPEG; без поворота — как было.
        let o = oriented(JPEG.to_vec(), 90);
        assert_eq!(&o[..4], &[0xFF, 0xD8, 0xFF, 0xE1]);
        assert_eq!(&o[6..12], b"Exif\0\0");
        // Значение — после SOI, маркера, длины, «Exif», заголовка TIFF, числа записей, тега, типа и счёта.
        assert_eq!(u16::from_be_bytes([o[30], o[31]]), 6);
        assert_eq!(&o[2 + 36..], &JPEG[2..]);
        assert_eq!(oriented(JPEG.to_vec(), 0), JPEG);
        // Своя пометка у JPEG, страница не повёрнута — «как есть» первой.
        let theirs = oriented(JPEG.to_vec(), 90);
        let kept = oriented(theirs.clone(), 0);
        assert_eq!(u16::from_be_bytes([kept[30], kept[31]]), 1);
        assert_eq!(&kept[38..], &theirs[2..]);
        assert_eq!(oriented(JPEG.to_vec(), 270)[31], 8);
    }

    #[test]
    fn main_image_with_small_stamp() {
        // п. 110: скан и маленький штамп архива — показывается скан.
        let pdf = page_with(&[(40, 30), (2000, 1500)], None, None);
        let got = pages(&pdf).unwrap();
        let Page::Jpeg { offset, .. } = got[0] else { panic!("{:?}", got[0]) };
        let big = find(&pdf, b"/Width 2000", 0).unwrap();
        assert!(offset as usize > big, "взята не большая картинка");
        // Две соизмеримые картинки — по-прежнему «показать не может».
        let got = pages(&page_with(&[(1000, 1500), (2000, 1500)], None, None)).unwrap();
        assert_eq!(got[0], Page::Unsupported("на странице 2 картинки".into()));
    }

    #[test]
    fn big_file_is_read_in_pieces() {
        // п. 111: файл из многих страниц с тяжёлыми картинками — разбор
        // читает таблицу и словари, а не картинки.
        let pad = vec![0x55u8; 3 << 20];
        let mut jpeg = JPEG[..JPEG.len() - 2].to_vec();
        jpeg.extend_from_slice(&pad);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        let n = 12;
        let mut objs: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            format!("<< /Type /Pages /Count {n} /Kids [{}] >>",
                    (0..n).map(|k| format!("{} 0 R", 3 + 2 * k)).collect::<Vec<_>>().join(" ")).into_bytes(),
        ];
        for k in 0..n {
            objs.push(format!("<< /Type /Page /Parent 2 0 R /Resources << /XObject << /I0 {} 0 R >> >> >>", 4 + 2 * k).into_bytes());
            let mut b = format!("<< /Subtype /Image /Width 9 /Height 9 /Filter /DCTDecode /Length {} >>\nstream\r\n", jpeg.len()).into_bytes();
            b.extend_from_slice(&jpeg);
            b.extend_from_slice(b"\r\nendstream");
            objs.push(b);
        }
        let pdf = sample_pdf(&objs, 1);
        let (got, read) = sparse(pdf.len(), |a, buf| { buf.copy_from_slice(&pdf[a..a + buf.len()]); Ok(()) })
            .unwrap().expect("кругов хватило");
        assert_eq!(got.unwrap(), pages(&pdf).unwrap(), "кусками — то же, что целиком");
        assert!(read * 4 < pdf.len(), "прочитано {read} из {}", pdf.len());
    }

    #[test]
    fn long_hex_string_at_piece_edge() {
        // Ревьюер 03.11.2026: строка <…> длиннее запаса, оборванная краем
        // прочитанного, давала «строка не закрыта» вместо дочитывания.
        let body = format!("<< /Type /Page /Y <{}> /Resources << >> >>", "0".repeat(300));
        let pdf = sample_pdf(&[b"<< /Type /Catalog >>".to_vec(), body.into_bytes()], 1);
        let obj = find(&pdf, b"2 0 obj", 0).unwrap();
        let cut = find(&pdf, b"/Y <", 0).unwrap() + 150;
        let have = [(0, cut)];
        let mut doc = Doc { data: &pdf, have: &have, want: Vec::new(), at: HashMap::new(),
                            streams: HashMap::new(), busy: HashSet::new() };
        doc.at.insert(2, Loc::At(obj));
        assert!(doc.load(2).is_err());
        assert!(!doc.want.is_empty(), "оборванная строка не попросила дочитать");
        // Всё прочитано — объект читается.
        let full = [(0, pdf.len())];
        let mut doc = Doc { data: &pdf, have: &full, want: Vec::new(), at: HashMap::new(),
                            streams: HashMap::new(), busy: HashSet::new() };
        doc.at.insert(2, Loc::At(obj));
        assert!(doc.load(2).is_ok());
    }

    #[test]
    fn rebuilds_broken_xref() {
        let mut pdf = three_pages();
        // Таблица ссылок указывает не туда.
        let sx = rfind(&pdf, b"startxref").unwrap();
        pdf.truncate(sx);
        pdf.extend_from_slice(b"startxref\n17\n%%EOF\n");
        assert_eq!(pages(&pdf).unwrap().len(), 3);
    }

    #[test]
    fn reads_xref_stream_and_object_streams() {
        // Каталог и дерево страниц — в сжатом потоке объектов, таблица
        // ссылок — потоком с предсказателем PNG (так пишут PDF 1.5).
        use std::io::Write;
        let zip = |b: &[u8]| {
            let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            z.write_all(b).unwrap();
            z.finish().unwrap()
        };
        let o1 = b"<< /Type /Catalog /Pages 2 0 R >>";
        let o2 = b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>";
        let head = format!("1 0 2 {} ", o1.len() + 1);
        let mut body = head.clone().into_bytes();
        body.extend_from_slice(o1);
        body.push(b' ');
        body.extend_from_slice(o2);
        let stm = zip(&body);
        let mut out = b"%PDF-1.5\n".to_vec();
        let mut offs = vec![0usize; 6];
        offs[3] = out.len();
        out.extend_from_slice(b"3 0 obj\n<< /Type /Page /Parent 2 0 R /Resources << /XObject << /Im 4 0 R >> >> >>\nendobj\n");
        offs[4] = out.len();
        out.extend_from_slice(b"4 0 obj\n");
        out.extend_from_slice(&image(None));
        out.extend_from_slice(b"\nendobj\n");
        offs[5] = out.len();
        out.extend_from_slice(format!("5 0 obj\n<< /Type /ObjStm /N 2 /First {} /Filter /FlateDecode /Length {} >>\nstream\n", head.len(), stm.len()).as_bytes());
        out.extend_from_slice(&stm);
        out.extend_from_slice(b"\nendstream\nendobj\n");
        // Строки таблицы: тип (1 байт), место (4), номер (1); объект 6 — она сама.
        let xref_at = out.len();
        let mut rows: Vec<[u8; 6]> = vec![[0, 0, 0, 0, 0, 0]];
        rows.push([2, 0, 0, 0, 5, 0]);
        rows.push([2, 0, 0, 0, 5, 1]);
        for k in [3usize, 4, 5] {
            let o = (offs[k] as u32).to_be_bytes();
            rows.push([1, o[0], o[1], o[2], o[3], 0]);
        }
        let o = (xref_at as u32).to_be_bytes();
        rows.push([1, o[0], o[1], o[2], o[3], 0]);
        // Предсказатель «вверх» (2): разница с предыдущей строкой.
        let mut pred = Vec::new();
        let mut prev = [0u8; 6];
        for r in &rows {
            pred.push(2);
            for i in 0..6 {
                pred.push(r[i].wrapping_sub(prev[i]));
            }
            prev = *r;
        }
        let xs = zip(&pred);
        out.extend_from_slice(format!("6 0 obj\n<< /Type /XRef /Size 7 /W [1 4 1] /Root 1 0 R /Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns 6 >> /Length {} >>\nstream\n", xs.len()).as_bytes());
        out.extend_from_slice(&xs);
        out.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{xref_at}\n%%EOF\n").as_bytes());
        let got = pages(&out).unwrap();
        assert_eq!(got.len(), 1);
        let Page::Jpeg { offset, len, .. } = got[0] else { panic!("{:?}", got[0]) };
        assert_eq!(&out[offset as usize..(offset + len) as usize], JPEG);
    }

    #[test]
    fn object_stream_length_inside_itself_is_an_error() {
        // /Length потока объектов — объект 2, который лежит в нём же.
        let mut out = b"%PDF-1.5\n".to_vec();
        let at3 = out.len();
        out.extend_from_slice(b"3 0 obj\n<< /Type /ObjStm /N 1 /First 4 /Length 2 0 R >>\nstream\n1 0 <<>>\nendstream\nendobj\n");
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 4\n0000000000 65535 f \n0000000000 00000 f \n0000000000 00000 f \n{at3:010} 00000 n \n").as_bytes());
        out.extend_from_slice(format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
        let mut doc = Doc { data: &out, have: &[(0, out.len())], want: Vec::new(), at: HashMap::new(), streams: HashMap::new(), busy: HashSet::new() };
        doc.at.insert(1, Loc::In(3, 0));
        doc.at.insert(2, Loc::In(3, 0));
        doc.at.insert(3, Loc::At(at3));
        let _ = doc.load(1);
        assert!(pages(&out).is_err());
    }

    #[test]
    fn huge_length_and_broken_kid() {
        let objs: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Count 2 /Kids [3 0 R 9 0 R] >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /Resources << /XObject << /I0 4 0 R >> >> >>".to_vec(),
            {
                let mut b = b"<< /Subtype /Image /Filter /DCTDecode /Length 1e30 >>\nstream\n".to_vec();
                b.extend_from_slice(JPEG);
                b.extend_from_slice(b"\nendstream");
                b
            },
        ];
        let pdf = sample_pdf(&objs, 1);
        let got = pages(&pdf).unwrap();
        assert_eq!(got.len(), 2, "{got:?}");
        let Page::Jpeg { offset, len, .. } = got[0] else { panic!("{:?}", got[0]) };
        assert_eq!(&pdf[offset as usize..(offset + len) as usize], JPEG, "длина — до endstream");
        assert!(matches!(got[1], Page::Unsupported(_)), "битая ссылка — одна страница с пометкой");
    }

    #[test]
    fn encrypted_pdf_is_named() {
        let mut pdf = three_pages();
        let t = rfind(&pdf, b"/Root 1 0 R").unwrap();
        pdf.splice(t..t, b"/Encrypt 9 0 R ".iter().copied());
        // Место startxref не сдвинулось: вставка — после таблицы.
        assert_eq!(pages(&pdf).unwrap_err(), ENCRYPTED);
    }

    #[test]
    fn rejects_garbage_without_panic() {
        assert!(pages(b"not a pdf").is_err());
        assert!(pages(b"%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 9 0 R >> endobj\ntrailer << /Root 1 0 R >>").is_err());
        // Обрывки настоящего файла — ошибка, а не паника.
        let pdf = three_pages();
        for cut in (0..pdf.len()).step_by(37) {
            let _ = pages(&pdf[..cut]);
        }
        let mut deep = b"%PDF-1.4\n1 0 obj\n".to_vec();
        deep.extend(std::iter::repeat(b'[').take(10_000));
        let _ = pages(&deep);
    }

    /// Файл Романа (`_reference/сканы_Романа_1410`): 43 страницы, у каждой —
    /// JPEG, первая — обложка 1706×2789. Только у разработчика:
    /// `GENMETRIC_SCANS_DIR=… cargo test -p genmetric-core scans_roman -- --ignored`.
    #[test]
    #[ignore]
    fn scans_roman_pdf() {
        let Ok(dir) = std::env::var("GENMETRIC_SCANS_DIR") else { return };
        let path = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path())
            .find(|p| p.extension().map_or(false, |x| x.eq_ignore_ascii_case("pdf"))).expect("в папке нет PDF");
        let t = std::time::Instant::now();
        let got = index_file(&path).unwrap();
        eprintln!("{} страниц за {:?}", got.len(), t.elapsed());
        assert_eq!(got.len(), 43);
        assert!(got.iter().all(|p| matches!(p, Page::Jpeg { .. })), "{got:?}");
        let Page::Jpeg { offset, len, .. } = got[0] else { unreachable!() };
        let jpeg = read_jpeg(&path, offset, len).unwrap();
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        assert_eq!(&jpeg[jpeg.len() - 2..], &[0xFF, 0xD9]);
        // Размер из заголовка SOF.
        let sof = jpeg.windows(2).position(|w| w[0] == 0xFF && (w[1] == 0xC0 || w[1] == 0xC2)).unwrap();
        let (h, w) = (u16::from_be_bytes([jpeg[sof + 5], jpeg[sof + 6]]), u16::from_be_bytes([jpeg[sof + 7], jpeg[sof + 8]]));
        assert_eq!((w, h), (1706, 2789));
    }
}
