import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Suggest from "./Suggest";
import { focusNextField } from "./focus";
import { dismissWarn, report, warn } from "./errors";
import { dirtyForms, setClearer, setDirty } from "./dirty";
import ExportPanel from "./ExportPanel";
import ParishDialog from "./ParishDialog";
import { takeReviewMark } from "./reviewmark";
import ReviewDialog from "./ReviewDialog";
import NumberField from "./NumberField";
import Modal from "./Modal";
import PlaceCard, { type PlaceInfo, type Similar } from "./PlaceCard";
import { PLACE_RENAMED, usePlaceRenamed } from "./PersonBlock";
import { placeCase } from "./names";
import { placeTypes } from "./placetypes";

/**
 * Шапка дела: архив, фонд, опись, дело, приход, год, кто индексирует.
 *
 * ДЕЛО — НА ГОД КНИГИ (02.10.2026). В Excel Романа 1889 год — «Ф.56 Оп.31
 * Д.11», 1890 — «Д.12»: у каждого года своё архивное дело. Экран остался
 * одной формой (Роман: «Текущий подход … вполне удобен. Менять эту логику не
 * нужно»): он показывает дело того года, с которым сейчас работают. Новому
 * году дело заводит программа при сохранении записи — копией прошлого.
 * Список годов в заголовке — чтобы поправить реквизиты прошлого года.
 *
 * Требование 3.4: вводится один раз и держится неизменной, пока человек сам
 * её не поменяет. В Excel это была та же идея, и Роман назвал её среди того,
 * что нужно обязательно сохранить.
 *
 * Церковь, село, уезд и губерния вместе образуют ключ прихода. По нему
 * переносится накопленная статистика подсказок между годами: индексируют
 * приходами, год за годом, поэтому на второй год подсказки почти всегда
 * попадают с первой буквы.
 */

export type Case = {
  id: number;
  archive: string | null;
  fond: string | null;
  opis: string | null;
  delo: string | null;
  church: string | null;
  village: string | null;
  uyezd: string | null;
  guberniya: string | null;
  year: number | null;
  indexer: string | null;
};

const EMPTY: Case = {
  id: 0, archive: null, fond: null, opis: null, delo: null, church: null,
  village: null, uyezd: null, guberniya: null, year: null, indexer: null,
};


/** Строка — дело, а не год: у года после импорта бывает два дела (п. 94). */
type CaseYear = { year: number; entries: number; id: number; delo: string | null };

type CaseSaved = { id: number; status: "saved" | "created" | "exists"; existing: string | null };

export default function CaseHeader({ onSaved, reload, showYear, parishName, onWorkYear, onOpenEntry, viewBlock }: {
  onSaved: (c: Case) => void; reload: number; parishName: string;
  /** Год сменили на форме — экран показывает дело этого года, а не
   *  «текущее» по последней записи (ревьюер 19.10.2026). */
  showYear?: { year: number; n: number } | null;
  /** Дело сохранено с этим годом — формы встают на него (спека 2026-10-03, п. 1.2).
   *  `fresh` — в году ещё нет записей: формы начинают с чистого листа (Роман 16.10.2026). */
  onWorkYear: (year: number, fresh?: boolean) => void;
  /** «Открыть запись» из списка на сверку. */
  onOpenEntry: (section: number, id: number) => void;
  /** Блок «Вид формы» — между кнопкой сохранения и выгрузкой. */
  viewBlock?: React.ReactNode;
}) {
  // Год открытого дела — отдельно от года в поле: поле можно поменять и
  // сохранить, и тогда новому году заводится своё дело (Роман 03.10.2026:
  // «сначала изменить архивный шифр …, а уже затем выбрать или изменить Год»).
  const [loadedYear, setLoadedYear] = useState<number | null>(null);
  const [ask, setAsk] = useState<{ year: number; existing: string } | null>(null);
  const [okText, setOkText] = useState("");
  const [review, setReview] = useState(false);
  // null — ещё не прочитано: кнопка «Список на сверку…» не мигает до ответа.
  const [toReview, setToReview] = useState<number | null>(null);
  const [c, setC] = useState<Case>(EMPTY);
  const [parishes, setParishes] = useState(false);
  // Реквизиты поправлены, но не сохранены: смена года или прихода их не
  // должна терять молча (проверяющий 02.10.2026).
  const [edited, setEdited] = useState(false);
  // Пункт переименовали или объединили в карточке НП формы, а в «Селе»
  // стоит прежнее название: иначе «Сохранить дело» записало бы его обратно
  // (ревьюер 03.11.2026). Дело в базе объединение уже поправило — поле
  // меняется без пометки «изменено».
  usePlaceRenamed((oldName, newName) =>
    setC((prev) => ((prev.village ?? "").trim() === oldName ? { ...prev, village: newName } : prev)));
  const editedRef = useRef(false);
  // Несохранённым считается и изменённый год: перечитывание его не выбрасывает.
  editedRef.current = edited || c.year !== loadedYear;
  const yearChanged = c.year !== loadedYear;
  useEffect(() => {
    setDirty("Дело", edited || yearChanged);
    return () => setDirty("Дело", false);
  }, [edited, yearChanged]);
  // «Очистить» в окне «Приходы» (ревьюер 03.11.2026): поправленные
  // реквизиты — как были в базе, дело перечитывается.
  // Дела в приходе ещё нет (новый приход) — перечитывать нечего: поля
  // пустеют (стенд 06.11.2026: фонд «999» оставался, и смена прихода после
  // «Очистить» снова отказывала).
  useEffect(() => {
    setClearer("Дело", () => {
      if (c.id) {
        load(loadedYear, c.id);
      } else {
        setC(EMPTY);
        setLoadedYear(null);
        setEdited(false);
      }
    });
    return () => setClearer("Дело", null);
  });
  const [years, setYears] = useState<CaseYear[]>([]);
  const [busy, setBusy] = useState(false);
  const [ok, setOk] = useState(false);

  /** Первое чтение дела после запуска (или смены прихода). */
  const startup = useRef(true);
  /** year = null — дело, с которым сейчас работают (год последней записи);
   *  id — дело, выбранное в списке (у года их бывает два, п. 94). */
  function load(year: number | null, id: number | null = null) {
    // Годы читаются один раз; ошибка — одним сообщением.
    const yearsRead = invoke<CaseYear[]>("case_years")
      .catch((e) => { report("Не удалось прочитать годы дел", e); return null; });
    invoke<Case | null>("case_load", { year, id })
      .then(async (loaded) => {
        if (!loaded) return;
        setC(loaded);
        setLoadedYear(loaded.year);
        setEdited(false);
        setOk(false);
        // Формам — только текущее дело: открытое ради правки реквизитов
        // прошлого года место работы не меняет.
        if (year === null) {
          onSaved(loaded);
          // Текущее дело — год без записей (новый год, заведённый до
          // перезапуска, `case_current`): формы встают на него с чистого
          // листа, а не на год последней записи (долг п. 67).
          const all = await yearsRead;
          if (startup.current && loaded.year !== null && all?.find((y) => y.year === loaded.year)?.entries === 0
              && all.some((y) => y.entries > 0)) {
            onWorkYear(loaded.year, true);
          }
          startup.current = false;
        }
      })
      .catch((e) => report("Не удалось прочитать дело", e));
    void yearsRead.then((v) => { if (v) setYears(v); });
    invoke<number>("review_count")
      .then((n) => {
        setToReview(n);
        // Сразу после импорта список открывается сам (Роман 20.10.2026):
        // окно импорта ставит отметку и перезагружает программу.
        // Отметка снимается всегда (проверяющий 20.10.2026: после импорта без
        // строк на сверку она ждала и открывала список позже, невпопад).
        if (takeReviewMark() && n > 0) setReview(true);
      })
      .catch((e) => report("Не удалось прочитать список на сверку", e));
  }
  // Запись другого года увела работу в другое дело — перечитать; но не поверх
  // несохранённой правки реквизитов.
  useEffect(() => { if (!editedRef.current) load(null); }, [reload]);
  useEffect(() => { if (showYear && !editedRef.current) load(showYear.year); }, [showYear?.n]);

  const emptyYear = loadedYear !== null && years.length > 1
    && years.find((y) => y.id === c.id)?.entries === 0;
  /** Год, у которого несколько дел: в списке рядом с годом — номер дела. */
  const twoCases = (year: number) => years.filter((y) => y.year === year).length > 1;

  /** Убрать дело года без записей: опечатка года или год, из которого ушла
   *  последняя запись. */
  async function dropYear() {
    if (loadedYear === null) return;
    try {
      const gone = await invoke<boolean>("case_delete", { year: loadedYear });
      if (!gone) warn("Дело не убрано", "в этом году есть записи, или это единственное дело прихода");
      load(null);
      // Формы стояли на убранном годе — следующая запись завела бы его дело
      // заново. Возвращаем их на год текущего дела (ревьюер 03.10.2026).
      const current = await invoke<Case | null>("case_load", { year: null });
      if (gone && current?.year != null) onWorkYear(current.year);
    } catch (e) {
      report("Не удалось убрать дело", e);
    }
  }

  function pickYear(id: number) {
    const year = years.find((y) => y.id === id)?.year ?? null;
    if (edited || (yearChanged && c.year !== year)) {
      warn("Сначала сохраните дело", "реквизиты этого года поправлены, но не сохранены — при смене года они пропали бы");
      return;
    }
    load(year, id);
  }

  const set = (k: keyof Case) => (v: string) => {
    setC((prev) => {
      const next = v === "" ? null : v;
      // Suggest сообщает значение и без правки (выбор того же) — «поправлено»
      // только когда оно действительно другое.
      if ((prev[k] ?? null) !== next) setEdited(true);
      return { ...prev, [k]: next };
    });
  };

  /** Enter и стрелки ведут по форме, как в остальных полях программы.
   *  Шапка заполняется редко, но спотыкаться на ней всё равно незачем. */
  function plainKeys(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter" || e.key === "ArrowDown") {
      e.preventDefault();
      // Shift+Enter — назад: заказчик 15.09.2026 возвращался в пропущенное
      // поле мышью. Стрелка вверх делала это и раньше, но её не нашли.
      focusNextField(e.currentTarget, e.shiftKey && e.key === "Enter" ? -1 : 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      focusNextField(e.currentTarget, -1);
    }
  }

  // Год — число, а не строка. Текстовое поле отдавало сюда строку, и
  // сохранение дела падало на разборе: программа ждёт число. Ошибка нашлась
  // на скриншоте до того, как её увидел тестировщик.
  const text = (label: string, k: "archive" | "fond" | "opis" | "delo" | "church"
                | "village" | "uyezd" | "guberniya" | "indexer", hint?: string) => (
    <div className="field">
      <label>{label}</label>
      <input
        data-field
        value={(c[k] as string | null) ?? ""}
        onChange={(e) => set(k)(e.target.value)}
        onKeyDown={plainKeys}
        autoComplete="off"
        spellCheck={false}
      />
      {hint && <div className="fieldhint">{hint}</div>}
    </div>
  );

  /** Год набран, реквизиты не тронуты, дело этого года есть — открыть его
   *  (то же, что выбор в списке годов). */
  function yearLeft() {
    if (!edited && c.year !== null && c.year !== loadedYear && years.some((y) => y.year === c.year))
      load(c.year);
  }

  // --- Карточка села прихода (Роман 06.10.2026) ---
  // «При создании прихода программа не предлагает заполнить карточку этого
  // села… карточка остаётся пустой»; его решение — «предлагать заполнить
  // сразу при создании прихода». После первого сохранения дела нового прихода
  // карточка открывается сама, если села нет в справочнике или у него нет
  // подробностей. Позже — ссылкой «карточка» в поле «Село».
  const [villageCard, setVillageCard] = useState<{ name: string; similar: Similar[]; existing?: PlaceInfo;
                                                   defaults: { guberniya: string; uyezd: string } } | null>(null);
  async function openVillageCard(from: Case, auto: boolean) {
    const text = (from.village ?? "").trim();
    if (!text || document.querySelector(".modal")) return;
    const defaults = { guberniya: from.guberniya ?? "", uyezd: from.uyezd ?? "" };
    try {
      const existing = await invoke<PlaceInfo | null>("place_get", { name: text });
      if (existing) {
        const filled = [existing.np_type, existing.guberniya, existing.uyezd, existing.volost]
          .some((v) => (v ?? "").trim() !== "");
        if (auto && filled) return;
        setVillageCard({ name: existing.name, similar: [], existing, defaults });
        return;
      }
      const r = await invoke<{ known: boolean; similar: Similar[] }>("place_check", { name: text });
      // Сама открывшаяся карточка похожих не показывает: село только что
      // набрано в деле, и вопрос «не это ли вы имели в виду» тут лишний.
      setVillageCard({ name: text, similar: auto ? [] : r.similar, defaults });
    } catch (e) {
      report(`Не удалось открыть карточку села «${text}»`, e);
    }
  }
  function villageSaved(saved: string) {
    const was = villageCard;
    setVillageCard(null);
    // В деле — название, как оно стоит в справочнике: по нему выгрузка
    // находит карточку села (чистое название она выводит сама). Поправили
    // название или комментарий — дело изменено и ждёт «Сохранить дело».
    if (was && (c.village ?? "").trim() !== saved) set("village")(saved);
    // Уезд и губерния карточки — в поля дела (Роман 03.11.2026: «указанные в
    // ней „Уезд“ и „Губерния“ должны автоматически подтянуться»). Карточку
    // только что подтвердили — её значения главнее.
    void fillFromVillage(saved, true).then(villageCardClosed);
  }

  /** Уезд и губерния села из справочника — в поля дела: `card` — из только
   *  что сохранённой карточки (заменяют), иначе — только в пустые поля. */
  async function fillFromVillage(name: string, card: boolean) {
    try {
      const p = await invoke<PlaceInfo | null>("place_get", { name });
      if (!p) return;
      // Пустота — по текущему делу, а не по замыканию: пока шёл запрос,
      // в «Уезд» могли начать набирать (ревьюер 03.11.2026).
      setC((prev) => {
        let next = prev;
        for (const k of ["uyezd", "guberniya"] as const) {
          const v = (p[k] ?? "").trim();
          if (v && (card || !(prev[k] ?? "").trim()) && (prev[k] ?? "") !== v) next = { ...next, [k]: v };
        }
        if (next !== prev) setEdited(true);
        return next;
      });
    } catch (e) {
      report(`Не удалось прочитать карточку села «${name}»`, e);
    }
  }

  /**
   * Уход из поля «Село» (Роман 03.11.2026: «Если введенного в поле „Село“
   * населенного пункта еще нет в справочнике, программа должна
   * автоматически открывать модальное окно создания нового НП (как это
   * работает при вводе неизвестного НП в записях…)»). Известное село — его
   * уезд и губерния в пустые поля дела. Только набранное в этот заход: проход
   * клавишами по готовому делу ничего не открывает.
   */
  async function leaveVillage(value: string, related: EventTarget | null, typed: boolean) {
    const text = value.trim();
    if (!text || !typed || (related === null && !document.hasFocus()) || document.querySelector(".modal")) return;
    try {
      const r = await invoke<{ known: boolean; canonical?: string | null; similar: Similar[] }>("place_check", { name: text });
      // Пока ждали ответ, поле могло измениться или открыться другое окно.
      if (villageNow.current.trim() !== text || document.querySelector(".modal")) return;
      if (r.known) {
        const name = r.canonical || text;
        if (name !== text) set("village")(name);
        await fillFromVillage(name, false);
        return;
      }
      // Ушли из «Село» щелчком по «Сохранить дело»: щелчок достаётся
      // карточке — дело сохраняется само, когда её закроют (п. 120).
      // По нажатию мыши на кнопке, не по relatedTarget: на macOS щелчок не
      // переводит на неё фокус. Щелчок успел дойти до кнопки (дело уже
      // сохраняется) — отметку снял onClick, второй раз не сохраняем.
      saveAfterCard.current = performance.now() - savePressed.current < 1000;
      setVillageCard({ name: text, similar: r.similar,
                       defaults: { guberniya: c.guberniya ?? "", uyezd: c.uyezd ?? "" } });
    } catch (e) {
      report(`Не удалось проверить село «${text}»`, e);
    }
  }
  const villageNow = useRef("");
  villageNow.current = c.village ?? "";
  /** Карточку села открыл щелчок по «Сохранить дело» (п. 120, 04.11.2026):
   *  после неё дело сохраняется само — уже с уездом и губернией карточки. */
  const saveAfterCard = useRef(false);
  const savePressed = useRef(-Infinity);
  const [saveNow, setSaveNow] = useState(0);
  function villageCardClosed() {
    if (!saveAfterCard.current) return;
    saveAfterCard.current = false;
    // После перерисовки: сохранение должно видеть село, уезд и губернию из
    // карточки, а не значения до неё.
    setSaveNow((n) => n + 1);
  }
  useEffect(() => {
    if (saveNow > 0) void save();
  }, [saveNow]);

  async function save(overwrite = false) {
    // Год — четыре цифры в разумных пределах: «189» завело бы дело 189 года.
    if (c.year !== null && (c.year < 1500 || c.year > 2100)) {
      warn("Год книги набран не полностью", `«${c.year}» — не год; поправьте поле «Год книги»`);
      return;
    }
    setBusy(true);
    setOk(false);
    setAsk(null);
    try {
      // Стёртый год год дела не стирает (так и в базе) — возвращаем его в поле.
      const toSave = c.year === null && loadedYear !== null ? { ...c, year: loadedYear } : c;
      const yearMoved = toSave.year !== loadedYear;
      const r = await invoke<CaseSaved>("case_save", { case: toSave, overwrite });
      if (r.status === "exists") {
        // У набранного года уже есть своё дело — молча его не переписываем.
        if (c.year !== null) setAsk({ year: c.year, existing: r.existing ?? "" });
        return;
      }
      // Прежняя жёлтая полоса («Новый год книги…», «Сначала сохраните дело»)
      // после сохранения устарела.
      dismissWarn();
      const firstSave = !(c.id > 0);
      const next = { ...toSave, id: r.id };
      setC(next);
      setLoadedYear(next.year);
      setEdited(false);
      // Церковь, село, уезд, губерния и индексатор разошлись на все годы —
      // формам нужно дело с ними, а не то, что открыто здесь. Дело —
      // сохранённого года: «текущее» (год последней записи) было бы прежним,
      // и блок скана записал бы папку нового года в прежний, затерев её
      // (Роман 16.10.2026, воспроизведено на стенде).
      // Новый год (год сменили или дело заведено, записей в нём нет) — окно
      // встаёт на дело этого года; правка реквизитов года с записями
      // оставляет окно на текущем деле, как раньше (ревьюер 16.10.2026).
      const all = await invoke<CaseYear[]>("case_years").catch((e) => { report("Не удалось прочитать годы дел", e); return null; });
      if (all) setYears(all);
      const entries = all?.find((y) => y.id === r.id)?.entries;
      // Окно — на год без записей (новый или пересохранённый пустой); форма с
      // чистого листа — только когда год сменили или завели дело.
      const empty = next.year !== null && entries === 0;
      const fresh = empty && (r.status === "created" || yearMoved);
      const current = await invoke<Case | null>("case_load", { year: empty ? next.year : null });
      onSaved(current ?? next);
      if (next.year !== null) onWorkYear(next.year, fresh);
      const scanDir = next.year !== null
        ? await invoke<string | null>("get_setting", { key: `scan_dir_${next.year}` })
            .catch((e) => { report("Не удалось узнать папку сканов нового года", e); return null; })
        : null;
      setOkText(r.status === "created"
        ? `Году ${next.year} заведено своё дело. Реквизиты прежнего года не изменились. Формы встали на ${next.year} год`
          + (fresh ? " с чистого листа: страница пуста, счёт 1, январь." : ".")
          + (scanDir ? "" : " Папку сканов этого года выберите в блоке скана («папка…»).")
        : "Сохранено. Можно переходить к записям.");
      setOk(true);
      // Новый приход: дело сохранено впервые — карточка его села.
      if (firstSave) void openVillageCard(next, true);
    } catch (e) {
      report("Не удалось сохранить дело", e);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section>
      {/* Приход — файл; открыт один (спека 2026-10-02, п. 1.4). */}
      <div className="parishrow main">
        <span>Приход: <b>{parishName || c.village || "без названия"}</b></span>
        {/* Кнопкой, не ссылкой: Роман 03.10.2026 — «не всегда интуитивно
            понятно, что это кликабельные элементы». */}
        <button type="button" className="toggle" onClick={() => setParishes(true)}>Сменить…</button>
        {/* Список на сверку — всегда под рукой, и когда всё сверено (Роман
            20.10 и 06.11.2026, п. 125: «кнопка на сверку была доступна всегда,
            а не только после импорта»): в нём и сделанное, его можно вернуть. */}
        {toReview === 0 && (
          <button type="button" className="toggle" data-review-open onClick={() => setReview(true)}>Список на сверку…</button>
        )}
      </div>
      {parishes && <ParishDialog onClose={() => setParishes(false)} />}
      {/* Заметно и всегда под рукой, пока список не пуст (Роман 20.10.2026: второй
          тестировщик не заметил прежнюю строку, и ошибки импорта осели в базе). */}
      {toReview !== null && toReview > 0 && (
        <div className="reviewbar" data-review-bar>
          <span>На сверку после импорта: <b>{toReview}</b> — проверьте, пока ошибки импорта не разошлись по базе.</span>
          <button type="button" className="primary" onClick={() => setReview(true)}>Открыть список на сверку</button>
        </div>
      )}
      {review && (
        <ReviewDialog
          onClose={() => { setReview(false); invoke<number>("review_count").then(setToReview)
            .catch((e) => report("Не удалось прочитать список на сверку", e)); }}
          onOpenEntry={(section, id) => {
            // Форма занята — запись в неё не откроется (формы сами берегут
            // набранное). Окно списка при этом не закрываем: раньше оно
            // закрывалось, и человек терял место в списке (проверяющий 03.10.2026).
            const form = ["", "Рождения", "Браки", "Смерти"][section] ?? "";
            if (dirtyForms().includes(form)) return form;
            setReview(false);
            onOpenEntry(section, id);
            return null;
          }}
        />
      )}
      {ask && (
        <Modal title={`У ${ask.year} года уже есть дело`} kind="case-exists" onClose={() => setAsk(null)}>
          <p>
            Дело {ask.year} года: {ask.existing || "реквизиты не заполнены"}. Заменить его
            реквизиты набранными{c.fond || c.opis || c.delo
              ? ` (Ф.${c.fond ?? ""} Оп.${c.opis ?? ""} Д.${c.delo ?? ""})` : ""}?
          </p>
          <p className="hint">Записи {ask.year} года останутся при своём деле — изменятся только фонд, опись и дело.</p>
          <div className="modalbar">
            <button type="button" className="primary" onClick={() => void save(true)}>Заменить</button>
            <button type="button" className="toggle" onClick={() => setAsk(null)}>Отмена (Esc)</button>
          </div>
        </Modal>
      )}
      <h2 className="caseyear">
        {loadedYear !== null ? `Дело за ${loadedYear} год` : "Дело"}
        {years.length > 1 && (
          <select value={c.id} title="Реквизиты дела другого года"
                  onChange={(e) => pickYear(Number(e.target.value))}>
            {years.map((y) => (
              <option key={y.id} value={y.id}>
                {y.year}{twoCases(y.year) ? ` · Д.${y.delo ?? "—"}` : ""} — записей: {y.entries}
              </option>
            ))}
          </select>
        )}
      </h2>
      <p className="hint">
        Заполняется один раз и держится, пока вы сами не измените. Фонд, опись и
        дело — у каждого года книги свои: новому году программа заводит дело сама
        и напоминает их проверить. Церковь, село, уезд и губерния — общие для всех
        годов прихода.
      </p>

      <Suggest label="Архив" kind="archive" browse value={c.archive ?? ""} onChange={set("archive")} />
      <div className="row fod">
        {text("Фонд", "fond")}
        {text("Опись", "opis")}
        {text("Дело", "delo")}
      </div>
      {/* Год книги — полем: поправили фонд, опись, дело, поставили год,
          сохранили — и набираете (спека 2026-10-03, п. 1). */}
      <div className="row tight caseyearrow" onBlur={yearLeft}>
        <NumberField label="Год книги" value={c.year} min={1700} max={1930}
                     onChange={(year) => setC((prev) => ({ ...prev, year }))} />
        <div className="fieldhint">
          {yearChanged && c.year !== null
            ? years.some((y) => y.year === c.year)
              ? edited
                ? `у ${c.year} года своё дело — «Сохранить» спросит, заменять ли его реквизиты`
                : `у ${c.year} года своё дело — оно откроется, когда вы выйдете из поля`
              : `«Сохранить» заведёт дело ${c.year} года; дело ${loadedYear ?? "прежнего"} года не изменится`
            : "новый год книги: поправьте фонд, опись, дело, поставьте год и сохраните"}
        </div>
      </div>
      <Suggest label="Церковь" kind="church" value={c.church ?? ""} onChange={set("church")} />
      {/* Заглавная буква — как в поле НП формы; «карточка» — тип, волость и
          ссылка Familio села прихода. */}
      <Suggest label="Село" kind="place" value={c.village ?? ""} onChange={set("village")}
               fix={(text, final) => placeCase(text, placeTypes(), final)}
               onLeave={(v, related, typed) => void leaveVillage(v, related, typed)}
               action={{ label: "карточка", onClick: () => void openVillageCard(c, false) }} />
      {villageCard && (
        <PlaceCard name={villageCard.name} similar={villageCard.similar} existing={villageCard.existing}
                   defaults={villageCard.defaults} defaultType="с." fillBare
                   onPick={(name) => { setVillageCard(null); set("village")(name); void fillFromVillage(name, false).then(villageCardClosed); }}
                   onSaved={villageSaved}
                   onMerged={(kept, dropped) => {
                     setVillageCard(null);
                     // Село дел в базе объединение уже поправило — здесь
                     // только поле, без «дело изменено».
                     setC((prev) => ((prev.village ?? "").trim() === dropped ? { ...prev, village: kept } : prev));
                     window.dispatchEvent(new CustomEvent(PLACE_RENAMED, { detail: { oldName: dropped, newName: kept } }));
                     // Карточку открыл щелчок «Сохранить дело» — после объединения
                     // дело сохраняется так же, как после карточки (ревьюер 04.11.2026).
                     void fillFromVillage(kept, false).then(villageCardClosed);
                   }}
                   onCancel={() => { setVillageCard(null); villageCardClosed(); }} />
      )}
      <Suggest label="Уезд" kind="uyezd" browse value={c.uyezd ?? ""} onChange={set("uyezd")} />
      <Suggest label="Губерния" kind="guberniya" browse value={c.guberniya ?? ""} onChange={set("guberniya")} />
      {/* 22.09.2026 год с этого экрана убирали («чтобы наличие двух годов не
          путало»); 03.10.2026 Роман попросил свободный порядок — «Год книги»
          вернулся выше, рядом с фондом, описью и делом. */}
      {text("Кто индексирует", "indexer")}

      <button className="primary" data-case-save onMouseDown={() => { savePressed.current = performance.now(); }}
              onClick={() => { savePressed.current = -Infinity; saveAfterCard.current = false; void save(); }} disabled={busy}>
        {busy ? "Сохраняю…" : "Сохранить дело"}
      </button>
      {ok && <p className="hint">{okText}</p>}
      {emptyYear && (
        <p className="hint">
          В {loadedYear} году нет ни одной записи.{" "}
          <button type="button" className="linkish" onClick={() => void dropYear()}>Убрать дело этого года</button>
        </p>
      )}

      {/* «Всегда в столбик» — между «Сохранить дело» и «Выгрузкой» (Роман
          07.10.2026). */}
      {viewBlock}

      {c.id > 0 && <ExportPanel mkCase={c} />}
    </section>
  );
}
