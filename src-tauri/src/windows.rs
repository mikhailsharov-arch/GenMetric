//! Окна программы: память положения и размера (бэклог З3.2, решение Mike
//! 20.10.2026) и окно скана (спека 2026-10-27-skan-oknom-i-dela).
//!
//! Роман 19.10.2026: «полная свобода» — форма и скан на любом экране в любом
//! порядке, каждое окно помнит своё место. Место пишется в файл папки данных,
//! а не в базу: окна создаются до открытия базы, и место нужно и тогда,
//! когда база не открылась.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Monitor, Position, Size, WebviewUrl, WebviewWindow, Window, WindowEvent};

pub const MAIN: &str = "main";
pub const SCAN: &str = "scan";

/// Место окна: левый верхний угол рамки и размер содержимого. Единицы — те,
/// в которых координаты у системы общие для всех мониторов (`units`): на
/// Windows — физические точки, на macOS — логические. Иначе при разном
/// масштабе мониторов окно вставало не туда и уменьшалось при каждом запуске
/// (ревьюер 28.10.2026).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Geo {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    /// Развёрнуто на весь экран: открывается развёрнутым, а место и размер —
    /// те, что были до разворота.
    #[serde(default)]
    pub max: bool,
}

pub struct Windows {
    path: PathBuf,
    log: PathBuf,
    /// Файл пишут и события окон, и команда окна скана, и выход — по одному.
    io: Mutex<()>,
    /// Открыть и закрыть окно скана — по одному: два быстрых нажатия не
    /// заводят окно дважды, «закрыть» не обгоняет «открыть» (ревьюер 28.10.2026).
    detach: Mutex<()>,
    geo: Mutex<BTreeMap<String, Geo>>,
    written: Mutex<Option<Instant>>,
    /// Программа закрывается: окно скана уходит вместе с ней и при следующем
    /// запуске открывается само; закрытое крестиком — прячет скан.
    exiting: AtomicBool,
}

impl Windows {
    pub fn load(data_dir: &Path) -> Windows {
        let path = data_dir.join("genmetric-окна.json");
        // Битый или чужой файл — как будто его нет: окна встанут как раньше.
        let geo = std::fs::read_to_string(&path).ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Windows { path, log: data_dir.join("genmetric-журнал.txt"), io: Mutex::new(()), detach: Mutex::new(()), geo: Mutex::new(geo),
                  written: Mutex::new(None), exiting: AtomicBool::new(false) }
    }

    pub fn get(&self, label: &str) -> Option<Geo> {
        self.geo.lock().unwrap_or_else(|p| p.into_inner()).get(label).copied()
    }

    /// Запомнить место окна. Файл пишется не чаще раза в полсекунды (окно
    /// тянут мышью — событий десятки в секунду) и всегда при `force`.
    fn remember(&self, label: &str, mut geo: Geo, force: bool) {
        {
            let mut all = self.geo.lock().unwrap_or_else(|p| p.into_inner());
            // Развёрнутое: место и размер — прежние, до разворота.
            if geo.max {
                if let Some(old) = all.get(label) {
                    geo = Geo { max: true, ..*old };
                }
            }
            if all.get(label) == Some(&geo) && !force {
                return;
            }
            all.insert(label.to_string(), geo);
        }
        let mut written = self.written.lock().unwrap_or_else(|p| p.into_inner());
        if force || written.map_or(true, |t| t.elapsed() >= Duration::from_millis(500)) {
            *written = Some(Instant::now());
            self.flush();
        }
    }

    /// Записать файл: во временный рядом и переименованием — оборванная
    /// запись не оставит битого файла. Ошибка записи — не повод мешать
    /// работе: место не запомнится, а в журнале останется строка.
    pub fn flush(&self) {
        let _io = self.io.lock().unwrap_or_else(|p| p.into_inner());
        let all = self.geo.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let tmp = self.path.with_extension("json.tmp");
        let result = serde_json::to_string_pretty(&all).map_err(|e| e.to_string())
            .and_then(|text| std::fs::write(&tmp, text).map_err(|e| e.to_string()))
            .and_then(|()| std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string()));
        if let Err(e) = result {
            crate::write_log(&self.log, &format!("Место окон не запомнилось ({}): {e}", self.path.display()));
        }
    }

    /// Программа закрывается (Cmd+Q, выход): окно скана закрывается вместе с
    /// ней и при следующем запуске откроется само.
    pub fn set_exiting(&self) {
        self.exiting.store(true, Ordering::SeqCst);
    }
}

/// Единицы, общие для всех мониторов: на macOS координаты окон — в логических
/// точках (физические у каждого монитора свои), на Windows — в физических.
#[cfg(target_os = "macos")]
fn units(scale: f64) -> f64 {
    scale
}
#[cfg(not(target_os = "macos"))]
fn units(_scale: f64) -> f64 {
    1.0
}

/// Рабочая область монитора в единицах `Geo`: x, y, ширина, высота.
fn area(m: &Monitor) -> (f64, f64, f64, f64) {
    let k = units(m.scale_factor());
    let a = m.work_area();
    (a.position.x as f64 / k, a.position.y as f64 / k, a.size.width as f64 / k, a.size.height as f64 / k)
}

/// Место окна сейчас; свёрнутое и во весь экран (macOS) — не запоминаем.
fn capture(w: &Window) -> Option<Geo> {
    if w.is_minimized().unwrap_or(false) || w.is_fullscreen().unwrap_or(false) {
        return None;
    }
    let k = units(w.scale_factor().ok()?);
    let pos = w.outer_position().ok()?;
    let size = w.inner_size().ok()?;
    Some(Geo { x: (pos.x as f64 / k).round() as i32, y: (pos.y as f64 / k).round() as i32,
               w: (size.width as f64 / k).round() as u32, h: (size.height as f64 / k).round() as u32,
               max: w.is_maximized().unwrap_or(false) })
}

/// Место видно на одном из подключённых мониторов (по середине верхней
/// кромки — за неё окно таскают). Второй монитор отключили — нет.
fn screen_of(app: &AppHandle, g: &Geo) -> Option<(f64, f64, f64, f64)> {
    let monitors = app.available_monitors().ok()?;
    let (cx, cy) = (g.x as f64 + g.w as f64 / 2.0, g.y as f64 + 16.0);
    monitors.iter().map(area).find(|&(x, y, w, h)| cx >= x && cx < x + w && cy >= y && cy < y + h)
}

/// Место, ужатое в рабочую область монитора: окно, сохранённое выше или
/// шире экрана (другое разрешение), не уходит краем за экран (проверяющий
/// 28.10.2026). Рамка заголовка — с запасом.
fn fit(g: Geo, (ax, ay, aw, ah): (f64, f64, f64, f64)) -> Geo {
    const FRAME: f64 = 40.0;
    let w = (g.w as f64).min(aw).max(320.0);
    let h = (g.h as f64).min(ah - FRAME).max(320.0);
    let x = (g.x as f64).min(ax + aw - w).max(ax);
    let y = (g.y as f64).min(ay + ah - h - FRAME).max(ay);
    Geo { x: x.round() as i32, y: y.round() as i32, w: w.round() as u32, h: h.round() as u32, max: g.max }
}

/// Поставить место в единицах `Geo`. Сначала место, потом размер: окно,
/// переехавшее на монитор другого масштаба, Windows пересчитывает, и размер,
/// заданный раньше, уменьшился бы (ревьюер 28.10.2026).
fn place(win: &WebviewWindow, g: &Geo) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        win.set_position(Position::Logical(tauri::LogicalPosition::new(g.x as f64, g.y as f64)))?;
        win.set_size(Size::Logical(tauri::LogicalSize::new(g.w as f64, g.h as f64)))?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        win.set_position(Position::Physical(tauri::PhysicalPosition::new(g.x, g.y)))?;
        win.set_size(Size::Physical(tauri::PhysicalSize::new(g.w, g.h)))?;
    }
    Ok(())
}

/// Поставить окно на запомненное место. false — места нет или оно вне
/// мониторов: окно остаётся, где его поставили по умолчанию.
pub fn restore(win: &WebviewWindow, label: &str) -> tauri::Result<bool> {
    let app = win.app_handle();
    let Some(g) = app.state::<Windows>().get(label) else { return Ok(false) };
    let Some(screen) = screen_of(app, &g) else { return Ok(false) };
    place(win, &fit(g, screen))?;
    if g.max {
        win.maximize()?;
    }
    Ok(true)
}

/// События окон: место — в память; закрыли окно программы — закрывается и
/// окно скана; закрыли окно скана крестиком — скан прячется.
pub fn on_event(window: &Window, event: &WindowEvent) {
    let app = window.app_handle();
    let Some(state) = app.try_state::<Windows>() else { return };
    let label = window.label();
    match event {
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            if let Some(g) = capture(window) {
                state.remember(label, g, false);
            }
        }
        WindowEvent::CloseRequested { .. } => {
            if let Some(g) = capture(window) {
                state.remember(label, g, true);
            }
            if label == MAIN {
                state.set_exiting();
                if let Some(scan) = app.get_webview_window(SCAN) {
                    if let Some(g) = capture(&scan.as_ref().window()) {
                        state.remember(SCAN, g, true);
                    }
                    let _ = scan.destroy();
                }
            } else if label == SCAN && !state.exiting.load(Ordering::SeqCst) {
                let _ = app.emit_to(MAIN, "scan-closed", ());
            }
        }
        _ => {}
    }
}

/// Выход из программы (Cmd+Q на Mac): окна закрываются без «закрыть окно» —
/// окно скана при следующем запуске откроется само.
pub fn on_exit(app: &AppHandle) {
    if let Some(state) = app.try_state::<Windows>() {
        state.set_exiting();
        state.flush();
    }
}

/// Выход запрошен (Cmd+Q): до того, как окна начнут закрываться, — иначе
/// окно скана сочло бы своё закрытие «спрятать скан» (ревьюер 28.10.2026).
pub fn on_exit_requested(app: &AppHandle) {
    if let Some(state) = app.try_state::<Windows>() {
        state.set_exiting();
    }
}

/// Аргументы WebView2 для сквозной проверки на Windows: порт отладки. У
/// всех окон одного окружения WebView2 они обязаны совпадать — иначе второе
/// окно не создаётся.
#[cfg(windows)]
pub fn e2e_browser_args() -> Option<String> {
    let port = std::env::var("GENMETRIC_E2E_DEBUG_PORT").ok()?;
    // Первая часть — то, что wry передаёт по умолчанию; при своих
    // аргументах её нужно повторить (см. документацию метода).
    Some(format!("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --remote-debugging-port={port}"))
}

/// Открыть или закрыть окно скана. Окно — та же страница программы; режим
/// «только скан» она узнаёт по метке окна. Курсор остаётся в поле формы:
/// окно скана встаёт без фокуса, главное окно его не отдаёт.
/// Асинхронная: на Windows окно, созданное из синхронной команды, вешает
/// программу (документация Tauri).
#[tauri::command]
pub async fn scan_detach(app: AppHandle, open: bool) -> Result<(), String> {
    let e = |e: tauri::Error| e.to_string();
    let state = app.state::<Windows>();
    let _one = state.detach.lock().unwrap_or_else(|p| p.into_inner());
    let existing = app.get_webview_window(SCAN);
    if !open {
        if let Some(win) = existing {
            if let Some(g) = capture(&win.as_ref().window()) {
                state.remember(SCAN, g, true);
            }
            win.destroy().map_err(e)?;
        }
        return Ok(());
    }
    if existing.is_some() {
        return Ok(());
    }
    #[allow(unused_mut)]
    let mut builder = tauri::WebviewWindowBuilder::new(&app, SCAN, WebviewUrl::App("index.html".into()))
        .title("GenMetric — скан")
        .inner_size(900.0, 1000.0)
        .min_inner_size(320.0, 320.0)
        .focused(false);
    #[cfg(windows)]
    if let Some(args) = e2e_browser_args() {
        builder = builder.additional_browser_args(&args);
    }
    let win = builder.build().map_err(e)?;
    if !restore(&win, SCAN).map_err(e)? {
        win.center().map_err(e)?;
    }
    if let Some(main) = app.get_webview_window(MAIN) {
        let _ = main.set_focus();
    }
    Ok(())
}

/// Буква, набранная в окне скана, идёт в поле формы (Роман 19.10.2026,
/// ответ 1.4): главное окно встаёт наверх, курсор — в том же поле.
#[tauri::command]
pub fn scan_focus_main(app: AppHandle) -> Result<(), String> {
    match app.get_webview_window(MAIN) {
        Some(main) => main.set_focus().map_err(|e| e.to_string()),
        None => Ok(()),
    }
}
