import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Suggest from "./Suggest";
import { focusNextField } from "./focus";
import { report } from "./errors";

/**
 * Шапка дела: архив, фонд, опись, дело, приход, год, кто индексирует.
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

export default function CaseHeader({ onSaved }: { onSaved: (c: Case) => void }) {
  const [c, setC] = useState<Case>(EMPTY);
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

  useEffect(() => {
    invoke<Case | null>("case_load")
      .then((loaded) => {
        if (loaded) {
          setC(loaded);
          onSaved(loaded);
        }
      })
      .catch((e) => report("Не удалось прочитать дело", e));
  }, []);

  const set = (k: keyof Case) => (v: string) =>
    setC((prev) => ({ ...prev, [k]: v === "" ? null : v }));

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

  async function save() {
    setBusy(true);
    setOk(false);
    try {
      const id = await invoke<number>("case_save", { case: { ...c, id: c.id || 1 } });
      const next = { ...c, id };
      setC(next);
      onSaved(next);
      setOk(true);
    } catch (e) {
      report("Не удалось сохранить дело", e);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section>
      <h2>Дело</h2>
      <p className="hint">
        Заполняется один раз и держится, пока вы сами не измените. Церковь, село,
        уезд и губерния вместе задают приход — по нему подсказки переносятся
        между годами.
      </p>

      <Suggest label="Архив" kind="archive" browse value={c.archive ?? ""} onChange={set("archive")} />
      <div className="row">
        {text("Фонд", "fond")}
        {text("Опись", "opis")}
        {text("Дело", "delo")}
      </div>
      <Suggest label="Церковь" kind="church" value={c.church ?? ""} onChange={set("church")} />
      <Suggest label="Село" kind="place" value={c.village ?? ""} onChange={set("village")} />
      <Suggest label="Уезд" kind="uyezd" browse value={c.uyezd ?? ""} onChange={set("uyezd")} />
      <Suggest label="Губерния" kind="guberniya" browse value={c.guberniya ?? ""} onChange={set("guberniya")} />
      {/* Года здесь больше нет — он на форме рождений и меняется по ходу
          индексации. Заказчик 22.09.2026: «чтобы наличие двух годов не путало».
          В деле год остаётся в базе как год первой записи. */}
      {text("Кто индексирует", "indexer")}

      <button className="primary" onClick={save} disabled={busy}>
        {busy ? "Сохраняю…" : "Сохранить дело"}
      </button>
      {ok && <p className="hint">Сохранено. Можно переходить к записям.</p>}

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
    </section>
  );
}
