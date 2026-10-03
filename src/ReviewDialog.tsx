import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Modal from "./Modal";
import { report } from "./errors";

/**
 * Список на сверку после импорта из Excel (спека 2026-10-03, п. 5).
 *
 * Роман 03.10.2026: «Нужен отдельный список на сверку, не только имен, а всех
 * нестандартных моментов, ты перечислил их достаточно при импорте моего
 * файла». Импорт сохраняет в приходе всё, что назвал в итоге: имена и
 * отчества вне словаря, строки, перенесённые с оговоркой, и пропущенные.
 * Отсюда запись открывается в форме; «Готово» убирает строку из списка.
 */

type Item = {
  id: number; kind: "name" | "patronymic" | "note" | "skipped"; text: string;
  sheet: string | null; row: number | null; entry_id: number | null; done: boolean;
  section: number | null; year: number | null; page: string | null; no: number | null;
};

const KIND: Record<Item["kind"], string> = {
  skipped: "Строки, которые не перенесены",
  note: "Перенесено с оговоркой",
  name: "Имена, которых нет в словаре",
  patronymic: "Отчества, которых нет в словаре",
};
const ORDER: Item["kind"][] = ["skipped", "note", "name", "patronymic"];
const SECTION = ["", "рождение", "брак", "смерть"];

export default function ReviewDialog({ onClose, onOpenEntry }: {
  onClose: () => void; onOpenEntry: (section: number, id: number) => void;
}) {
  const [items, setItems] = useState<Item[]>([]);
  const [showDone, setShowDone] = useState(false);

  function load(done: boolean) {
    invoke<Item[]>("review_list", { done })
      .then(setItems)
      .catch((e) => report("Не удалось прочитать список на сверку", e));
  }
  useEffect(() => { load(showDone); }, [showDone]);

  function mark(item: Item, done: boolean) {
    invoke("review_done", { id: item.id, done })
      .then(() => load(showDone))
      .catch((e) => report("Не удалось отметить строку списка", e));
  }

  const open = items.filter((i) => !i.done).length;

  return (
    <Modal title={`На сверку после импорта: ${open}`} kind="review" onClose={onClose}>
      <p className="hint">
        Всё, что импорт перенёс не дословно или не смог сверить. «Открыть запись» —
        запись в форме (форма должна быть пустой); «Готово» — убрать строку из списка.
      </p>
      {ORDER.map((kind) => {
        const rows = items.filter((i) => i.kind === kind);
        if (!rows.length) return null;
        return (
          <div key={kind}>
            <h3>{KIND[kind]}: {rows.filter((i) => !i.done).length}</h3>
            <table className="facts review">
              <tbody>
                {rows.map((i) => (
                  <tr key={i.id} className={i.done ? "done" : ""}>
                    <td>
                      {i.text}
                      <div className="fieldhint">
                        {i.section ? `${SECTION[i.section]} ${i.year ?? "без года"}` : ""}
                        {i.no !== null ? `, № ${i.no}` : ""}{i.page ? `, стр. ${i.page}` : ""}
                        {i.sheet ? `${i.section ? " · " : ""}Excel: лист «${i.sheet}», строка ${i.row}` : ""}
                      </div>
                    </td>
                    <td>
                      {i.entry_id !== null && i.section !== null && (
                        <button type="button" className="toggle small"
                                onClick={() => onOpenEntry(i.section!, i.entry_id!)}>
                          Открыть запись
                        </button>
                      )}
                      <button type="button" className="toggle small" onClick={() => mark(i, !i.done)}>
                        {i.done ? "Вернуть" : "Готово"}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        );
      })}
      {items.length === 0 && <p>Список пуст.</p>}
      <div className="modalbar">
        <button type="button" className="toggle" aria-pressed={showDone} onClick={() => setShowDone((v) => !v)}>
          {showDone ? "Скрыть сделанные" : "Показать сделанные"}
        </button>
        <button type="button" className="toggle" onClick={onClose}>Закрыть (Esc)</button>
      </div>
    </Modal>
  );
}
