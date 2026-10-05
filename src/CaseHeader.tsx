import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Suggest from "./Suggest";
import { focusNextField } from "./focus";
import { dismissWarn, report, warn } from "./errors";
import { setDirty } from "./dirty";
import ExportPanel from "./ExportPanel";
import ParishDialog from "./ParishDialog";
import ReviewDialog from "./ReviewDialog";
import NumberField from "./NumberField";
import Modal from "./Modal";

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

type ImportReport = {
  persons_added: number;
  spouses_added: number;
  clergy_added: number;
  places_added: number;
  source: string;
};

type CaseYear = { year: number; entries: number };

type CaseSaved = { id: number; status: "saved" | "created" | "exists"; existing: string | null };

export default function CaseHeader({ onSaved, reload, parishName, onWorkYear, onOpenEntry }: {
  onSaved: (c: Case) => void; reload: number; parishName: string;
  /** Дело сохранено с этим годом — формы встают на него (спека 2026-10-03, п. 1.2). */
  onWorkYear: (year: number) => void;
  /** «Открыть запись» из списка на сверку. */
  onOpenEntry: (section: number, id: number) => void;
}) {
  // Год открытого дела — отдельно от года в поле: поле можно поменять и
  // сохранить, и тогда новому году заводится своё дело (Роман 03.10.2026:
  // «сначала изменить архивный шифр …, а уже затем выбрать или изменить Год»).
  const [loadedYear, setLoadedYear] = useState<number | null>(null);
  const [ask, setAsk] = useState<{ year: number; existing: string } | null>(null);
  const [okText, setOkText] = useState("");
  const [review, setReview] = useState(false);
  const [toReview, setToReview] = useState(0);
  const [c, setC] = useState<Case>(EMPTY);
  const [parishes, setParishes] = useState(false);
  // Реквизиты поправлены, но не сохранены: смена года или прихода их не
  // должна терять молча (проверяющий 02.10.2026).
  const [edited, setEdited] = useState(false);
  const editedRef = useRef(false);
  // Несохранённым считается и изменённый год: перечитывание его не выбрасывает.
  editedRef.current = edited || c.year !== loadedYear;
  const yearChanged = c.year !== loadedYear;
  useEffect(() => {
    setDirty("Дело", edited || yearChanged);
    return () => setDirty("Дело", false);
  }, [edited, yearChanged]);
  const [years, setYears] = useState<CaseYear[]>([]);
  const [busy, setBusy] = useState(false);
  const [ok, setOk] = useState(false);

  // Архив подсказок из Excel: загружается файлом, который присылает Михаил.
  const [archiveLoaded, setArchiveLoaded] = useState<string | null>(null);
  const [archiveReport, setArchiveReport] = useState<ImportReport | null>(null);
  const [archiveBusy, setArchiveBusy] = useState(false);
  const filePick = useRef<HTMLInputElement>(null);

  useEffect(() => {
    invoke<string | null>("get_setting", { key: "archive_loaded" })
      .then(setArchiveLoaded)
      .catch((e) => report("Не удалось узнать, загружен ли архив", e));
  }, [archiveReport]);

  /**
   * Файл читается прямо в окне и уходит в программу байтами — так не нужен
   * ни плагин диалогов, ни доступ к путям на диске. Архив весит около
   * полутора мегабайт, это мгновенно.
   */
  async function importArchive(file: File) {
    setArchiveBusy(true);
    try {
      // Байты — обычным аргументом, не сырым телом: сырое тело на Windows
      // не дошло (13.09.2026, инцидент). Tauri сам превращает Uint8Array
      // в массив чисел, Rust принимает его как Vec<u8>.
      const bytes = new Uint8Array(await file.arrayBuffer());
      const r = await invoke<ImportReport>("import_archive", { bytes });
      setArchiveReport(r);
    } catch (e) {
      report("Архив не загрузился", e);
    } finally {
      setArchiveBusy(false);
      if (filePick.current) filePick.current.value = "";
    }
  }

  /** year = null — дело, с которым сейчас работают (год последней записи). */
  function load(year: number | null) {
    invoke<Case | null>("case_load", { year })
      .then((loaded) => {
        if (!loaded) return;
        setC(loaded);
        setLoadedYear(loaded.year);
        setEdited(false);
        setOk(false);
        // Формам — только текущее дело: открытое ради правки реквизитов
        // прошлого года место работы не меняет.
        if (year === null) onSaved(loaded);
      })
      .catch((e) => report("Не удалось прочитать дело", e));
    invoke<CaseYear[]>("case_years")
      .then(setYears)
      .catch((e) => report("Не удалось прочитать годы дел", e));
    invoke<number>("review_count")
      .then(setToReview)
      .catch((e) => report("Не удалось прочитать список на сверку", e));
  }
  // Запись другого года увела работу в другое дело — перечитать; но не поверх
  // несохранённой правки реквизитов.
  useEffect(() => { if (!editedRef.current) load(null); }, [reload]);

  const emptyYear = loadedYear !== null && years.length > 1
    && years.find((y) => y.year === loadedYear)?.entries === 0;

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

  function pickYear(year: number) {
    if (edited || (yearChanged && c.year !== year)) {
      warn("Сначала сохраните дело", "реквизиты этого года поправлены, но не сохранены — при смене года они пропали бы");
      return;
    }
    load(year);
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
      const r = await invoke<CaseSaved>("case_save", { case: toSave, overwrite });
      if (r.status === "exists") {
        // У набранного года уже есть своё дело — молча его не переписываем.
        if (c.year !== null) setAsk({ year: c.year, existing: r.existing ?? "" });
        return;
      }
      // Прежняя жёлтая полоса («Новый год книги…», «Сначала сохраните дело»)
      // после сохранения устарела.
      dismissWarn();
      const next = { ...toSave, id: r.id };
      setC(next);
      setLoadedYear(next.year);
      setEdited(false);
      // Церковь, село, уезд, губерния и индексатор разошлись на все годы —
      // формам нужно текущее дело с ними, а не то, что открыто здесь.
      const current = await invoke<Case | null>("case_load", { year: null });
      onSaved(current ?? next);
      invoke<CaseYear[]>("case_years").then(setYears)
        .catch((e) => report("Не удалось прочитать годы дел", e));
      if (next.year !== null) onWorkYear(next.year);
      setOkText(r.status === "created"
        ? `Году ${next.year} заведено своё дело. Реквизиты прежнего года не изменились. Формы встали на ${next.year} год.`
        : "Сохранено. Можно переходить к записям.");
      setOk(true);
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
      </div>
      {parishes && <ParishDialog onClose={() => setParishes(false)} />}
      {toReview > 0 && (
        <div className="parishrow">
          <span>На сверку после импорта: <b>{toReview}</b></span>
          <button type="button" className="toggle small" onClick={() => setReview(true)}>Показать…</button>
        </div>
      )}
      {review && (
        <ReviewDialog
          onClose={() => { setReview(false); invoke<number>("review_count").then(setToReview)
            .catch((e) => report("Не удалось прочитать список на сверку", e)); }}
          onOpenEntry={(section, id) => { setReview(false); onOpenEntry(section, id); }}
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
          <select value={loadedYear ?? ""} title="Реквизиты дела другого года"
                  onChange={(e) => pickYear(Number(e.target.value))}>
            {years.map((y) => (
              <option key={y.year} value={y.year}>{y.year} — записей: {y.entries}</option>
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
      <Suggest label="Село" kind="place" value={c.village ?? ""} onChange={set("village")} />
      <Suggest label="Уезд" kind="uyezd" browse value={c.uyezd ?? ""} onChange={set("uyezd")} />
      <Suggest label="Губерния" kind="guberniya" browse value={c.guberniya ?? ""} onChange={set("guberniya")} />
      {/* 22.09.2026 год с этого экрана убирали («чтобы наличие двух годов не
          путало»); 03.10.2026 Роман попросил свободный порядок — «Год книги»
          вернулся выше, рядом с фондом, описью и делом. */}
      {text("Кто индексирует", "indexer")}

      <button className="primary" onClick={() => void save()} disabled={busy}>
        {busy ? "Сохраняю…" : "Сохранить дело"}
      </button>
      {ok && <p className="hint">{okText}</p>}
      {emptyYear && (
        <p className="hint">
          В {loadedYear} году нет ни одной записи.{" "}
          <button type="button" className="linkish" onClick={() => void dropYear()}>Убрать дело этого года</button>
        </p>
      )}

      {/* Память подсказок из Excel. Без неё программа помнит только набранное
          в ней самой, и подсказка людьми на первых страницах пуста. */}
      <div className="archive">
        <h2 className="sub-h2">Архив подсказок из Excel</h2>
        <p className="hint">
          {archiveLoaded
            ? `Загружен: ${archiveLoaded}. Люди, места, звания и причт из Excel уже подсказываются.`
            : "Пока не загружен. Файл архива присылает Михаил; после загрузки программа " +
              "будет подсказывать людей, места и звания из вашего Excel."}
        </p>
        <input
          ref={filePick}
          type="file"
          accept=".sqlite,application/octet-stream"
          hidden
          onChange={(e) => {
            const f = e.target.files?.[0];
            if (f) void importArchive(f);
          }}
        />
        <button
          type="button"
          className="toggle"
          disabled={archiveBusy}
          onClick={() => filePick.current?.click()}
        >
          {archiveBusy ? "Загружаю…" : "Загрузить архив из файла"}
        </button>
        {archiveReport && (
          <p className="hint">
            Добавлено: персон {archiveReport.persons_added}, пар муж — жена {archiveReport.spouses_added},
            причта {archiveReport.clergy_added}, населённых пунктов {archiveReport.places_added}.
          </p>
        )}
      </div>

      {c.id > 0 && <ExportPanel mkCase={c} />}
    </section>
  );
}
