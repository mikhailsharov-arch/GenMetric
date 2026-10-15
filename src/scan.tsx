import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { report, warn } from "./errors";
import { panStep } from "./page";

/**
 * Сканы рядом с формой (спека 2026-10-10-skany-ryadom-s-formoj).
 *
 * Роман 03.10.2026: «встроенный просмотрщик сканов рядом с формой и ссылка на
 * скан у записи». 05.10.2026: «Формат файлов: JPG. Сканы одного дела лежат в
 * одной папке. Одно фото (один файл) — это всегда один разворот книги», имена
 * произвольные. Поэтому листает человек, а программа помнит: какой разворот
 * открыт и с какого набрана запись.
 *
 * Здесь — состояние (папка дела, файлы, открытый разворот) и сам блок скана.
 * Увеличение, перетаскивание, поворот и листание с сохранением места даёт
 * OpenSeadragon (BSD-3-Clause; решение Mike 10.10.2026 — «чтобы не писать
 * вьюер самому»). Наше — папка, раскладка, связь с записью и то, что мышь в
 * скане не отбирает курсор у формы.
 */

type ScanFiles = { dir: string | null; files: string[]; sep: string; missing: string | null; open: string | null };

type Scan = {
  /** Блок скана показан. */
  on: boolean;
  setOn: (on: boolean) => void;
  /** Год дела, чья папка открыта. */
  year: number | null;
  dir: string | null;
  files: string[];
  /** Папка выбрана, но не читается, — что сказать человеку. */
  missing: string | null;
  index: number;
  /** Имя открытого файла; null — показывать нечего. */
  file: string | null;
  /** Адрес открытого файла для окна. */
  url: (name: string) => string;
  /** Листание человеком. */
  step: (by: 1 | -1) => void;
  /** Переход к развороту по порядковому номеру с нуля (Роман 12.10.2026:
   *  «нажимать на стрелку 52 раза — не вариант»). */
  goTo: (index: number) => void;
  /**
   * Показать разворот записи по ссылке «скан». Место набора запоминается —
   * к нему возвращает кнопка «к набору». Год — год книги записи: папка у
   * каждого дела своя, и одноимённый файл чужого дела показывать нельзя.
   */
  show: (name: string, year: number | null) => "ok" | "нет файла" | "другое дело";
  /** Системное окно выбора папки. */
  pickDir: () => void;
  /** Файл для новой записи года `year`: открытый разворот, если блок включён и папка — этого дела. */
  current: (year: number | null) => string | null;
  /** Запись открыта на правку: показать её разворот, запомнив место набора. */
  peek: (name: string | null, year: number | null) => void;
  /** Вернуться к развороту, на котором шёл набор. */
  back: () => void;
  /** Показан не тот разворот, на котором шёл набор: запись на правке или ссылка «скан». */
  away: "edit" | "link" | null;
  /** Подписаться на перелистывание человеком на один файл (Alt+PageDown,
   *  Alt+PageUp, «←» и «→») — по нему форма сдвигает страницу (Роман
   *  14.10.2026). Переход по номеру, ссылка «скан», показ записи на правке не
   *  считаются. Возвращает отписку. */
  onTurn: (fn: (by: 1 | -1) => void) => () => void;
  /** При перелистывании — то же место листа (true) или верх листа (false). */
  keepPlace: boolean;
  setKeepPlace: (on: boolean) => void;
  /** Ширина колонки формы при включённом блоке, px. */
  formWidth: number;
  setFormWidth: (px: number, save: boolean) => void;
};

const EMPTY: ScanFiles = { dir: null, files: [], sep: "/", missing: null, open: null };
const ScanContext = createContext<Scan | null>(null);

export function useScan(): Scan {
  const scan = useContext(ScanContext);
  if (!scan) throw new Error("useScan вне ScanProvider");
  return scan;
}

export const FORM_WIDTH = 470;
const FORM_MIN = 380;

export function ScanProvider({ year, ready, children }: { year: number | null; ready: boolean; children: React.ReactNode }) {
  const [on, setOnState] = useState(false);
  const [state, setState] = useState<ScanFiles>(EMPTY);
  const [index, setIndex] = useState(0);
  const [formWidth, setFormWidthState] = useState(FORM_WIDTH);
  const [keepPlace, setKeepPlaceState] = useState(false);
  const turnListeners = useRef(new Set<(by: 1 | -1) => void>());
  /** Разворот, на котором шёл набор, пока показан другой: запись на правке
   *  или ссылка «скан». */
  const held = useRef<number | null>(null);
  const [away, setAway] = useState<"edit" | "link" | null>(null);
  const yearRef = useRef(year);
  yearRef.current = year;

  const apply = useCallback((next: ScanFiles) => {
    held.current = null;
    setAway(null);
    setState(next);
    setIndex(Math.max(0, next.open ? next.files.indexOf(next.open) : 0));
  }, []);

  // Вид окна — общий для приходов: включён ли блок и ширина колонки формы.
  useEffect(() => {
    if (!ready) return;
    invoke<string | null>("get_setting", { key: "ui_scan_on" })
      .then((v) => { if (v === "1") setOnState(true); })
      .catch((e) => report("Не удалось прочитать, показан ли блок скана", e));
    invoke<string | null>("get_setting", { key: "ui_scan_keep_place" })
      .then((v) => { if (v === "1") setKeepPlaceState(true); })
      .catch((e) => report("Не удалось прочитать, как вести скан при перелистывании", e));
    invoke<string | null>("get_setting", { key: "ui_scan_form_width" })
      .then((v) => { const n = Number(v); if (Number.isFinite(n) && n >= FORM_MIN) setFormWidthState(n); })
      .catch((e) => report("Не удалось прочитать ширину колонки формы", e));
  }, [ready]);

  // Папка — у дела: сменился год дела — читаем его папку.
  const loadSeq = useRef(0);
  useEffect(() => {
    if (!ready) return;
    const mine = ++loadSeq.current;
    invoke<ScanFiles>("scan_state", { year })
      .then((s) => { if (mine === loadSeq.current) apply(s); })
      .catch((e) => report("Не удалось прочитать папку сканов", e));
  }, [year, ready, apply]);

  // Окно: блок включён — узкое окно расширяется, выключен — возвращается.
  const windowSeq = useRef(false);
  useEffect(() => {
    if (!ready || (!on && !windowSeq.current)) return;
    windowSeq.current = true;
    invoke("scan_window", { on }).catch((e) => report("Не удалось изменить ширину окна", e));
  }, [on, ready]);

  const setOn = useCallback((value: boolean) => {
    setOnState(value);
    invoke("set_setting", { key: "ui_scan_on", value: value ? "1" : "0" })
      .catch((e) => report("Вид окна изменён, но не сохранён", e));
  }, []);

  const dirRef = useRef(state.dir);
  dirRef.current = state.dir;
  /** Папка выбрана: показать её и включить блок. */
  const chosen = useCallback((s: ScanFiles) => {
    apply(s);
    setOn(true);
    if (!s.missing && s.files.length === 0) warn("В папке нет сканов", "программа показывает файлы .jpg, .jpeg и .png; вложенные папки не читаются");
  }, [apply, setOn]);

  const pickDir = useCallback(() => {
    const mine = ++loadSeq.current;
    // Системное окно роботу стенда и сквозной проверки недоступно — им путь
    // даёт подмена `window.__pickScanDir`; дальше всё настоящее.
    const hook = (window as unknown as { __pickScanDir?: () => string | null }).__pickScanDir;
    const asked = hook
      ? Promise.resolve(hook()).then((path) => (path ? invoke<ScanFiles>("scan_set_dir", { year: yearRef.current, path }) : null))
      : invoke<ScanFiles | null>("scan_pick_dir", { year: yearRef.current });
    asked
      .then((s) => { if (s && mine === loadSeq.current) chosen(s); })
      .catch((e) => report("Не удалось открыть папку сканов", e));
  }, [chosen]);

  // Перетаскивание папки или файла-скана на окно программы. Событие даёт
  // само окно программы; в браузере стенда его нет — там и не подписываемся.
  // Чужой файл (не скан) Rust не принимает: на окно бросают и другое.
  const chosenRef = useRef(chosen);
  chosenRef.current = chosen;
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let off: (() => void) | undefined;
    let gone = false;
    const dropped = (path: string) => {
      const mine = ++loadSeq.current;
      invoke<ScanFiles>("scan_set_dir", { year: yearRef.current, path, dropped: true })
        .then((s) => {
          if (mine !== loadSeq.current) return;
          // Ничего не изменилось и открывать нечего — бросили не скан.
          if (s.dir === dirRef.current && !s.open && s.dir !== path) return;
          chosenRef.current(s);
        })
        .catch((e) => report("Не удалось открыть перетащенную папку сканов", e));
    };
    import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) => getCurrentWebview().onDragDropEvent((e) => {
        if (e.payload.type === "drop" && e.payload.paths[0]) dropped(e.payload.paths[0]);
      }))
      .then((unlisten) => { if (gone) unlisten(); else off = unlisten; })
      .catch((e) => report("Перетаскивание папки сканов на окно не работает — выберите папку кнопкой", e));
    return () => { gone = true; off?.(); };
  }, []);

  const count = state.files.length;
  const at = Math.min(index, Math.max(0, count - 1));
  const file = count > 0 && !state.missing ? state.files[at] : null;

  // Открытый разворот запоминается — кроме показа записи на правке.
  const remembered = useRef<string | null>(null);
  useEffect(() => {
    if (!file || held.current !== null || remembered.current === `${year}|${file}`) return;
    // Отметка «запомнено» — только когда запись ушла: отменённое ожидание
    // (пролистали дальше, открыли запись) не должно считаться сделанным.
    const timer = window.setTimeout(() => {
      remembered.current = `${year}|${file}`;
      invoke("scan_remember", { year, file }).catch((e) => report("Открытый скан не запомнен", e));
    }, 600);
    return () => window.clearTimeout(timer);
  }, [file, year]);

  const scan = useMemo<Scan>(() => {
    const go = (i: number) => setIndex(Math.min(Math.max(0, count - 1), Math.max(0, i)));
    const leave = (kind: "edit" | "link", i: number) => {
      if (held.current === null) held.current = at;
      setAway(kind);
      setIndex(i);
    };
    return {
      on, setOn, year, dir: state.dir, files: state.files, missing: state.missing, index: at, file, away,
      url: (name) => convertFileSrc(`${state.dir ?? ""}${state.sep}${name}`),
      step: (by) => {
        // Листают после ссылки «скан» — значит, работают уже здесь: место
        // набора теперь тут. При правке записи место набора ждёт её конца.
        const wasAway = away !== null;
        if (away === "link") { held.current = null; setAway(null); }
        const to = Math.min(Math.max(0, count - 1), Math.max(0, at + by));
        go(to);
        // Страница формы идёт за сканом, только если скан и правда сменился
        // и показан разворот набора, а не чужой записи.
        if (to !== at && !wasAway) turnListeners.current.forEach((fn) => fn(by));
      },
      onTurn: (fn) => {
        turnListeners.current.add(fn);
        return () => { turnListeners.current.delete(fn); };
      },
      keepPlace,
      setKeepPlace: (value) => {
        setKeepPlaceState(value);
        invoke("set_setting", { key: "ui_scan_keep_place", value: value ? "1" : "0" })
          .catch((e) => report("Как вести скан при перелистывании — изменено, но не сохранено", e));
      },
      goTo: (i) => {
        if (away === "link") { held.current = null; setAway(null); }
        go(i);
      },
      show: (name, entryYear) => {
        if (entryYear !== year) return "другое дело";
        const i = state.files.indexOf(name);
        if (i < 0) return "нет файла";
        if (i !== at) leave(away ?? "link", i);
        return "ok";
      },
      pickDir,
      // У дела ещё нет года (новое дело) — папка его же: первая запись года получает файл.
      current: (formYear) => (on && (year === null || formYear === year) ? file : null),
      peek: (name, entryYear) => {
        if (!name || entryYear !== year) return;
        const i = state.files.indexOf(name);
        if (i < 0) return;
        if (i === at && held.current === null) return;
        leave("edit", i);
      },
      back: () => {
        setAway(null);
        if (held.current === null) return;
        const to = held.current;
        held.current = null;
        setIndex(to);
      },
      formWidth,
      setFormWidth: (px, save) => {
        const width = Math.round(Math.max(FORM_MIN, px));
        setFormWidthState(width);
        if (save) invoke("set_setting", { key: "ui_scan_form_width", value: String(width) })
          .catch((e) => report("Ширина колонки формы не сохранена", e));
      },
    };
  }, [on, setOn, year, state, at, file, count, pickDir, formWidth, away, keepPlace]);

  return <ScanContext.Provider value={scan}>{children}</ScanContext.Provider>;
}

/**
 * Скан записи в форме: что записать при сохранении и что показать при правке.
 *
 * Новая запись получает открытый разворот — если папка в блоке от дела её
 * года. Запись на правке свой файл не меняет и чужой не получает, пока человек
 * не нажал «привязать»: он открыл её поправить букву, а в блоке — сегодняшний
 * разворот; молчаливая привязка тысяч прежних записей к чужим листам хуже
 * записи без скана (ревьюер и проверяющий 10.10.2026).
 */
export function useEntryScan(editing: boolean, year: number | null,
                             turn?: (by: 1 | -1) => void, root?: React.RefObject<HTMLElement>) {
  const scan = useScan();
  const [own, setOwn] = useState<string | null>(null);
  const [rebind, setRebind] = useState(false);
  const open = scan.current(year);
  // Страница сама при перелистывании (Роман 14.10.2026): только новая
  // запись, только видимая форма (три формы живут в окне одновременно),
  // только когда открыта папка дела года этой формы.
  const now = useRef({ editing, open, turn });
  now.current = { editing, open, turn };
  const subscribe = scan.onTurn;
  useEffect(() => subscribe((by) => {
    const { editing: busy, open: file, turn: fn } = now.current;
    if (busy || !file || !fn) return;
    if (root?.current && root.current.offsetParent === null) return;
    fn(by);
  }), [subscribe]);
  const bind = (
    <button type="button" className="linkish" data-scan-rebind onMouseDown={(e) => e.preventDefault()}
            onClick={() => setRebind(true)}>привязать к открытому</button>
  );
  return {
    /** Запись поднята в форму; `entryYear` — год её книги. */
    opened(file: string | null, entryYear: number | null) {
      setOwn(file);
      setRebind(false);
      scan.peek(file, entryYear);
    },
    /** Правка сохранена или отменена. */
    closed() {
      setOwn(null);
      setRebind(false);
      scan.back();
    },
    forSave(): string | null {
      if (!editing) return open;
      return rebind && open ? open : own;
    },
    /** Строка в полосе правки: у записи нет разворота или он не тот, что открыт. */
    bar: !editing || !scan.on || !open || own === open ? null
      : rebind ? <span className="scanbind" data-scan-bind="rebound"> · скан: запишется {open}</span>
      : own === null ? <span className="scanbind" data-scan-bind="none"> · у записи нет скана: {bind}</span>
      : <span className="scanbind" data-scan-bind="other"> · скан записи: {own} {bind}</span>,
  };
}

/** Ссылка в строке списка «Набрано» и в досье: показать разворот записи. */
export function ScanLink({ file, year }: { file: string | null | undefined; year: number | null }) {
  const scan = useScan();
  if (!file) return null;
  return (
    <button type="button" className="linkish scanlink" data-scan-link={file} title={`Показать скан: ${file}`}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => {
              const got = scan.show(file, year);
              if (got === "ok") { if (!scan.on) scan.setOn(true); return; }
              if (got === "другое дело") warn("Скан — в папке другого дела",
                `запись из дела ${year ?? "без года"} года, а открыта папка дела ${scan.year ?? "без года"} года; у записи — файл «${file}». Откройте дело её года на экране «Дело»`);
              else warn("Скана нет в папке", scan.dir ? `в папке «${scan.dir}» нет файла «${file}»` : `папка сканов этого дела не выбрана; у записи — файл «${file}»`);
            }}>
      скан
    </button>
  );
}

type Viewer = import("openseadragon").Viewer;
type Osd = typeof import("openseadragon");

/** Просмотрщик открытого блока — для клавиш, которые слушает всё окно. */
const live: { viewer: Viewer | null; osd: Osd | null } = { viewer: null, osd: null };

/** Сдвинуть вид на долю видимой части: `dx`, `dy` — в долях ширины и высоты
 *  блока. `now` — без анимации: повторы удержанной клавиши идут ровно. */
function panView(dx: number, dy: number, now = false) {
  const { viewer: v, osd } = live;
  if (!v || !osd) return;
  const size = v.viewport.getContainerSize();
  v.viewport.panBy(v.viewport.deltaPointsFromPixels(new osd.Point(size.x * dx, size.y * dy)), now);
  v.viewport.applyConstraints(now);
}

/** Лист виден по ширине целиком — двигать вбок нечего (долг п. 58): тогда
 *  Alt+← и Alt+→ остаются полю («±1» в числовом, переход по словам на Mac). */
function fitsWidth(): boolean {
  const { viewer: v, osd } = live;
  const item = v?.world.getItemAt(0);
  if (!v || !osd || !item) return true;
  const b = item.getBounds(true);
  const xs = [[b.x, b.y], [b.x + b.width, b.y], [b.x, b.y + b.height], [b.x + b.width, b.y + b.height]]
    .map(([x, y]) => v.viewport.viewportToViewerElementCoordinates(new osd.Point(x, y)).x);
  return Math.max(...xs) - Math.min(...xs) <= v.viewport.getContainerSize().x + 1;
}

/**
 * Вид — к верхнему краю листа, масштаб и место по горизонтали прежние (Роман
 * 12.10.2026: «дойдя до последней записи, я оказываюсь в самом низу… при
 * смене слайда масштаб должен сохраняться, но фокус просмотра должен
 * автоматически сбрасываться на самый верх новой страницы»). Считаем в
 * точках блока: так верно и для повёрнутого листа. Лист виден целиком по
 * высоте — двигать нечего.
 */
function toTop(v: Viewer, osd: Osd) {
  const item = v.world.getItemAt(0);
  if (!item) return;
  const b = item.getBounds(true);
  const ys = [[b.x, b.y], [b.x + b.width, b.y], [b.x, b.y + b.height], [b.x + b.width, b.y + b.height]]
    .map(([x, y]) => v.viewport.viewportToViewerElementCoordinates(new osd.Point(x, y)).y);
  const top = Math.min(...ys), bottom = Math.max(...ys);
  if (bottom - top <= v.viewport.getContainerSize().y) return;
  v.viewport.panBy(v.viewport.deltaPointsFromPixels(new osd.Point(0, top)), true);
}

/** Блок скана: справа от формы. */
export function ScanPane() {
  const scan = useScan();
  const box = useRef<HTMLDivElement>(null);
  const viewer = useRef<Viewer | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [loaded, setLoaded] = useState<string | null>(null);
  const filesKey = `${scan.dir}|${scan.files.join("|")}`;
  const want = useRef(scan.index);
  want.current = scan.index;
  const keepRef = useRef(scan.keepPlace);
  keepRef.current = scan.keepPlace;

  // Просмотрщик создаётся, когда есть что показывать, и пересоздаётся при
  // смене папки: набор разворотов у него задаётся целиком.
  useEffect(() => {
    if (!box.current || scan.files.length === 0 || scan.missing) return;
    let gone = false;
    const el = box.current;
    setProblem(null);
    import("openseadragon").then(({ default: OpenSeadragon }) => {
      if (gone) return;
      const v = OpenSeadragon({
        element: el,
        // Один разворот за раз; на следующем — то же место, масштаб и поворот.
        sequenceMode: true,
        preserveViewport: true,
        initialPage: want.current,
        tileSources: scan.files.map((name) => ({ type: "image", url: scan.url(name), buildPyramid: false })) as never,
        // Картинка идёт через протокол программы — с разрешением CORS, без
        // него режим WebGL её не возьмёт; canvas — запасной.
        crossOriginPolicy: "Anonymous",
        drawer: ["webgl", "canvas"],
        // Кнопки — свои, над сканом: встроенным нужны картинки из пакета.
        showNavigationControl: false,
        showSequenceControl: false,
        // Клавиатура остаётся форме.
        keyboardNavEnabled: false,
        tabIndex: -1,
        gestureSettingsMouse: { clickToZoom: false, dblClickToZoom: true, scrollToZoom: true },
        maxZoomPixelRatio: 4,
        visibilityRatio: 0.3,
        animationTime: 0.25,
        zoomPerScroll: 1.25,
      });
      viewer.current = v;
      live.viewer = v;
      live.osd = OpenSeadragon;
      let opened = 0;
      // Просмотрщик на щелчке забирает фокус себе (canvas.focus() в его
      // коде), а уход курсора из поля формы — это сверка недописанного имени
      // и окно поверх набора. Курсор остаётся форме: клавиатурой просмотрщик
      // у нас не управляется.
      v.canvas.focus = () => undefined;
      // Место и масштаб — в атрибуты блока: их читают сценарии проверки.
      v.addHandler("animation-finish", () => {
        const pane = el.closest<HTMLElement>(".scanpane");
        if (!pane) return;
        const c = v.viewport.getCenter(true);
        pane.dataset.scanView = `${v.viewport.getZoom(true).toFixed(3)} ${c.x.toFixed(3)} ${c.y.toFixed(3)} ${v.viewport.getRotation()}`;
      });
      v.addHandler("open", () => {
        setProblem(null);
        setLoaded(scan.files[v.currentPage()] ?? null);
        // Первый разворот открывается целиком; следующие — с верха листа.
        if (opened++ > 0 && !keepRef.current) toTop(v, OpenSeadragon);
      });
      v.addHandler("open-failed", () => {
        setLoaded(null);
        setProblem(`Файл «${scan.files[want.current] ?? ""}» не открылся: его нет на месте или это не картинка JPG или PNG.`);
      });
    }).catch((e) => { if (!gone) { setProblem("Просмотрщик не запустился."); report("Не удалось запустить просмотрщик сканов", e); } });
    return () => {
      gone = true;
      if (live.viewer === viewer.current) live.viewer = null;
      viewer.current?.destroy();
      viewer.current = null;
      setLoaded(null);
    };
  }, [filesKey, scan.missing]);

  // Листание.
  useEffect(() => {
    const v = viewer.current;
    if (v && v.currentPage() !== scan.index) v.goToPage(scan.index);
  }, [scan.index, filesKey]);

  const view = (act: (v: Viewer) => void) => () => { if (viewer.current) act(viewer.current); };
  // Кнопки блока курсор у формы не забирают.
  const keep = { onMouseDown: (e: React.MouseEvent) => e.preventDefault() };
  const count = scan.files.length;
  const empty = !scan.dir ? "Папка со сканами этого дела не выбрана."
    : scan.missing ? `Папка сканов не открылась: ${scan.missing}`
    : count === 0 ? "В папке нет файлов .jpg, .jpeg и .png." : null;

  // Номер разворота — поле: набрали, Enter — открыт. Курсор после этого
  // возвращается в поле формы, из которого пришли.
  const [typedNo, setTypedNo] = useState<string | null>(null);
  const cameFrom = useRef<HTMLElement | null>(null);
  function leaveNo(apply: boolean) {
    const n = Number(typedNo);
    if (apply && typedNo && Number.isInteger(n) && n >= 1) scan.goTo(Math.min(count, n) - 1);
    setTypedNo(null);
    const back = cameFrom.current;
    cameFrom.current = null;
    if (back && back.isConnected) back.focus();
    else (document.activeElement as HTMLElement | null)?.blur();
  }

  return (
    <aside className="scanpane" data-scan data-scan-file={scan.file ?? ""} data-scan-count={count}
           data-scan-loaded={loaded ?? ""} data-scan-problem={problem ?? ""}
           // Нажатие мыши в блоке не уводит курсор из поля формы: набор
           // продолжается в том же поле (спека, п. 2.3). Перетаскивание и
           // колесо у просмотрщика — на событиях указателя, им это не мешает.
           // Поле номера разворота — исключение: в него щёлкают, чтобы набрать.
           onMouseDownCapture={(e) => { if (!(e.target as HTMLElement).closest("[data-scan-no]")) e.preventDefault(); }}>
      <div className="scanbar">
        <button type="button" className="toggle small" data-scan-prev disabled={count === 0 || scan.index === 0} {...keep}
                onClick={() => scan.step(-1)} title="Предыдущий разворот (Alt+PageUp)">←</button>
        <button type="button" className="toggle small" data-scan-next disabled={count === 0 || scan.index >= count - 1} {...keep}
                onClick={() => scan.step(1)} title="Следующий разворот (Alt+PageDown)">→</button>
        <span className="scanname" title={scan.dir ?? ""}>
          {count > 0 && !scan.missing ? (
            <>
              <input className="scanno" data-scan-no inputMode="numeric" aria-label="Номер разворота" tabIndex={-1}
                     title="Номер разворота: наберите и нажмите Enter"
                     value={typedNo ?? String(scan.index + 1)}
                     onFocus={(e) => { cameFrom.current = e.relatedTarget as HTMLElement | null; setTypedNo(""); }}
                     onChange={(e) => setTypedNo(e.target.value.replace(/[^0-9]/g, "").slice(0, 5))}
                     onBlur={(e) => {
                       // Из поля номера ушли не Enter и не Esc, а щелчком в
                       // другое поле: поле, из которого пришли, своего ухода
                       // ещё не прожило (сверка имени, пробел — долг п. 61).
                       // Проводим курсор через него: правила сработают, как
                       // при обычном переходе из него в новое поле. Кроме
                       // выпадающего списка: открытый щелчком, он бы закрылся
                       // (ревьюер 15.10.2026) — сохранение правила повторит.
                       const back = cameFrom.current, to = e.relatedTarget as HTMLElement | null;
                       if (back && back.isConnected && to && to !== back && to.matches?.("input, textarea")) {
                         setTimeout(() => { back.focus(); to.focus(); }, 0);
                       }
                       if (typedNo !== null) { setTypedNo(null); cameFrom.current = null; }
                     }}
                     onKeyDown={(e) => {
                       if (e.key === "Enter") { e.preventDefault(); e.stopPropagation(); leaveNo(true); }
                       else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); leaveNo(false); }
                     }} />
              <b> из {count}</b> · {scan.file}
            </>
          ) : "скан"}
        </span>
        {scan.away === "link" && (
          <button type="button" className="toggle small" data-scan-back {...keep} onClick={scan.back}
                  title="Вернуться к развороту, на котором шёл набор">↩ к набору</button>
        )}
        <span className="scantools">
          <button type="button" className="toggle small" data-scan-width disabled={!loaded} {...keep}
                  onClick={view((v) => v.viewport.fitHorizontally())} title="Разворот по ширине блока"><span className="long">по ширине</span><span className="short" aria-hidden>↔</span></button>
          <button type="button" className="toggle small" data-scan-whole disabled={!loaded} {...keep}
                  onClick={view((v) => v.viewport.goHome())} title="Разворот целиком"><span className="long">целиком</span><span className="short" aria-hidden>▢</span></button>
          <button type="button" className="toggle small" data-scan-rotate disabled={!loaded} {...keep}
                  onClick={view((v) => v.viewport.setRotation((v.viewport.getRotation() + 90) % 360))} title="Повернуть на 90° по часовой стрелке"><span className="long">повернуть</span><span className="short" aria-hidden>⟳</span></button>
          <button type="button" className={scan.keepPlace ? "toggle small on" : "toggle small"} data-scan-keep
                  aria-pressed={scan.keepPlace} {...keep} onClick={() => scan.setKeepPlace(!scan.keepPlace)}
                  title={scan.keepPlace
                    ? "Следующий разворот — на том же месте листа. Нажмите, чтобы открывать с верха листа"
                    : "Следующий разворот — с верха листа. Нажмите, чтобы оставлять то же место листа"}>
            <span className="long">{scan.keepPlace ? "то же место" : "к верху"}</span>
            <span className="short" aria-hidden>{scan.keepPlace ? "⇔" : "⤒"}</span>
          </button>
          <button type="button" className="toggle small" data-scan-dir {...keep} onClick={scan.pickDir}
                  title={scan.dir ? `Папка: ${scan.dir}` : "Выбрать папку со сканами дела"}><span className="long">папка…</span><span className="short" aria-hidden>📁</span></button>
          <button type="button" className="toggle small" data-scan-hide {...keep} onClick={() => scan.setOn(false)}
                  title="Убрать скан; вернуть — кнопкой на экране «Дело»"><span className="long">скрыть</span><span className="short" aria-hidden>✕</span></button>
        </span>
      </div>
      {empty ? (
        <div className="scanempty" data-scan-empty>
          <p>{empty}</p>
          <button type="button" className="toggle" {...keep} onClick={scan.pickDir}>Выбрать папку…</button>
          <p className="hint">
            Можно и перетащить папку (или любой её файл) мышью на окно программы. Сканы — файлы JPG или PNG, один
            файл — один разворот или одна страница; они идут по порядку имён. У каждого дела папка своя.
          </p>
        </div>
      ) : (
        <>
          {problem && <p className="scanproblem" data-scan-error>{problem}</p>}
          <div className="scanview" ref={box} />
        </>
      )}
    </aside>
  );
}

/** Граница между формой и сканом: тянется мышью, ширина формы запоминается. */
export function ScanSplitter() {
  const scan = useScan();
  return (
    <div className="scansplit" data-scan-split title="Потяните, чтобы изменить ширину формы"
         // Курсор остаётся в поле формы и при перетаскивании границы.
         onMouseDown={(e) => e.preventDefault()}
         onPointerDown={(e) => {
           e.preventDefault();
           const el = e.currentTarget;
           el.setPointerCapture(e.pointerId);
           const startX = e.clientX, startWidth = scan.formWidth;
           const limit = () => Math.max(FORM_MIN, window.innerWidth - 260);
           const width = (ev: PointerEvent) => Math.min(limit(), startWidth + ev.clientX - startX);
           const move = (ev: PointerEvent) => scan.setFormWidth(width(ev), false);
           const up = (ev: PointerEvent) => {
             el.removeEventListener("pointermove", move);
             el.removeEventListener("pointerup", up);
             el.removeEventListener("pointercancel", up);
             scan.setFormWidth(width(ev), true);
           };
           el.addEventListener("pointermove", move);
           el.addEventListener("pointerup", up);
           el.addEventListener("pointercancel", up);
         }} />
  );
}

/**
 * Окно из двух частей: слева колонка формы, справа скан. Здесь же листание с
 * клавиатуры из любого поля: Alt+PageDown и Alt+PageUp листают, Alt+стрелки
 * двигают лист — набранное не трогают.
 */
export function ScanShell({ children }: { children: React.ReactNode }) {
  const scan = useScan();
  const stepRef = useRef(scan.step);
  stepRef.current = scan.step;
  const onRef = useRef(scan.on);
  onRef.current = scan.on;
  useEffect(() => {
    // Удержание стрелки: с какого момента и какой клавишей — для разгона.
    const held = { key: "", since: 0 };
    const on = (e: KeyboardEvent) => {
      // Только пока есть что двигать и листать: блок включён, а папки нет —
      // клавиши остаются полям.
      if (!e.altKey || e.ctrlKey || e.metaKey || !onRef.current || !live.viewer) return;
      // Alt+стрелки двигают увеличенный лист (Роман 12.10.2026: «не снимать
      // руки с клавиатуры при чтении документа»). Стрелка — куда идёт
      // взгляд: Alt+↓ показывает то, что ниже. Числовое поле при видимом
      // скане своё Alt+← и Alt+→ («число ±1») уступает.
      const pan = ({ ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] } as Record<string, number[]>)[e.key];
      // Alt+↓ в поле с перечнем («Причина», «Родство», архив) открывает
      // перечень — это остаётся полю (ревьюер 13.10.2026).
      if (pan && e.key === "ArrowDown" && (e.target as HTMLElement | null)?.matches?.("[data-browse], select")) return;
      if (pan && pan[0] !== 0 && fitsWidth()) return;
      if (pan) {
        e.preventDefault();
        e.stopPropagation();
        // Короткое нажатие — заметный шаг, удержание — ползёт и разгоняется
        // (Роман 15.10.2026; panStep в page.ts).
        const repeat = e.repeat && held.key === e.key;
        if (!repeat) { held.key = e.key; held.since = performance.now(); }
        const step = panStep(repeat, performance.now() - held.since);
        panView(pan[0] * step, pan[1] * step, repeat);
        return;
      }
      if (e.key !== "PageDown" && e.key !== "PageUp") return;
      e.preventDefault();
      e.stopPropagation();
      stepRef.current(e.key === "PageDown" ? 1 : -1);
    };
    document.addEventListener("keydown", on, true);
    return () => document.removeEventListener("keydown", on, true);
  }, []);
  // Окно с вопросом (сверка имени, карточка пункта) при включённом скане
  // занимает только колонку формы: скан рядом остаётся виден и доступен мыши
  // — сверять имя по скану и есть главный случай (проверяющий 10.10.2026).
  // Окна живут в body, поэтому ширина колонки передаётся ему.
  useEffect(() => {
    document.body.classList.toggle("withscan", scan.on);
    if (scan.on) document.body.style.setProperty("--formw", `${scan.formWidth}px`);
    else document.body.style.removeProperty("--formw");
    return () => { document.body.classList.remove("withscan"); document.body.style.removeProperty("--formw"); };
  }, [scan.on, scan.formWidth]);
  return (
    <div className={scan.on ? "shell withscan" : "shell"}
         style={scan.on ? ({ "--formw": `${scan.formWidth}px` } as React.CSSProperties) : undefined}>
      {/* Колонка программы — контейнер для правил раскладки по ширине. */}
      <div className="appcol">{children}</div>
      {scan.on && <><ScanSplitter /><ScanPane /></>}
    </div>
  );
}

/** Блок на экране «Дело»: включить скан и выбрать папку этого дела. */
export function ScanCaseBlock() {
  const scan = useScan();
  return (
    <div className="findblock scanblock">
      <button type="button" className="toggle" data-scan-toggle onClick={() => scan.setOn(!scan.on)}>
        {scan.on ? "Скрыть скан" : "Показать скан рядом с формой"}
      </button>{" "}
      <button type="button" className="toggle" data-scan-pick onClick={scan.pickDir}>
        {scan.dir ? "Сменить папку сканов…" : "Папка сканов дела…"}
      </button>
      <p className="hint">
        {scan.dir
          ? <>Сканы этого дела: <span className="mono">{scan.dir}</span>{scan.missing ? " — папка сейчас не открывается" : `, файлов: ${scan.files.length}`}. </>
          : "Сканы дела — файлы JPG или PNG в одной папке. Папку можно и перетащить мышью на окно программы. "}
        Листать, не уходя из поля: Alt+PageDown и Alt+PageUp; двигать лист — Alt+стрелки. Запись запоминает
        разворот, с которого набрана.
      </p>
    </div>
  );
}
