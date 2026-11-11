import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Modal from "./Modal";
import { report } from "./errors";

/**
 * «Удалить запись» — у записи, открытой на правку (Роман 11.11.2026:
 * «критически нужна… внизу рядом с кнопками сохранения/отмены», «В общем
 * списке „Набрано“ делать её не обязательно»; подтверждение «обязательно»).
 * Курсор в окне — на «Отмена»: привычный Enter не удаляет. Перед удалением
 * программа пишет запись целиком в свой журнал.
 */
export default function DeleteEntry({ id, what, onDeleted }: {
  id: number;
  /** Что за запись — для вопроса: «рождение, № м. 3, Пётр». */
  what: string;
  onDeleted: () => void;
}) {
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);
  async function remove() {
    setBusy(true);
    try {
      await invoke<number>("entry_delete", { id });
      setAsking(false);
      onDeleted();
    } catch (e) {
      report("Не удалось удалить запись", e);
    } finally {
      setBusy(false);
    }
  }
  return (
    <>
      <button type="button" className="danger" data-entry-delete tabIndex={-1} onClick={() => setAsking(true)}
              title="Удалить эту запись из прихода — с подтверждением">
        Удалить запись
      </button>
      {asking && (
        <Modal title="Удалить запись?" kind="entry-delete" onClose={busy ? () => {} : () => setAsking(false)}>
          <p>
            Вы уверены, что хотите <b>безвозвратно</b> удалить эту запись ({what})? Её люди уйдут и из подсказок.
          </p>
          <p className="hint">Отменить удаление кнопкой нельзя. Запись целиком остаётся только в журнале программы.</p>
          <div className="modalbar">
            <button type="button" className="danger" data-entry-delete-yes disabled={busy} onClick={() => void remove()}>
              {busy ? "Удаляю…" : "Да, удалить"}
            </button>
            <button type="button" className="toggle" data-autofocus disabled={busy} onClick={() => setAsking(false)}>Отмена</button>
          </div>
        </Modal>
      )}
    </>
  );
}
