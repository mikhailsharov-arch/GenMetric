import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { report } from "./errors";

/**
 * Архив подсказок из Excel — память персон, мест, званий и причта, которую
 * присылает Михаил файлом. До 05.10.2026 блок стоял на экране «Дело»; Роман:
 * «не используется при повседневной индексации и перегружает интерфейс».
 * Теперь он на экране «ⓘ О программе»: новому приходу без импорта пригодится.
 */

type ImportReport = {
  persons_added: number;
  spouses_added: number;
  clergy_added: number;
  places_added: number;
  source: string;
};

export default function ArchiveBlock() {
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

  return (
    <>
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

    </>
  );
}
