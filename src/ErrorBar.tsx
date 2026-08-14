import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { dismiss, useAppError } from "./errors";

/**
 * Полоса вверху окна: появляется только когда что-то пошло не так.
 *
 * Сверху — по-русски, что не получилось. Под кнопкой «Подробности» —
 * техническое сообщение, которое можно выделить и прислать. Отдельно журнал:
 * в нём остаются все ошибки за всё время, включая те, что человек уже закрыл.
 */
export default function ErrorBar() {
  const error = useAppError();
  const [details, setDetails] = useState(false);
  const [log, setLog] = useState<string | null>(null);

  if (!error) return null;

  async function showLog() {
    if (log !== null) {
      setLog(null);
      return;
    }
    try {
      setLog(await invoke<string>("read_log", { lines: 100 }));
    } catch (e) {
      setLog(`Не удалось прочитать журнал: ${String(e)}`);
    }
  }

  return (
    <div className="errorbar" role="alert">
      <div className="errorbar-head">
        <div>
          <b>{error.what}</b>
          <div className="errorbar-sub">
            Это не ваша ошибка. Нажмите «Подробности» и пришлите текст —
            по нему видно причину.
          </div>
        </div>
        <button className="errorbar-close" onClick={dismiss} title="Закрыть">
          ✕
        </button>
      </div>

      <div className="errorbar-buttons">
        <button onClick={() => setDetails((v) => !v)}>
          {details ? "Скрыть подробности" : "Подробности"}
        </button>
        <button onClick={showLog}>
          {log !== null ? "Скрыть журнал" : "Показать журнал"}
        </button>
      </div>

      {details && <pre className="errorbar-detail">{error.detail}</pre>}
      {log !== null && <pre className="errorbar-detail">{log}</pre>}
    </div>
  );
}
