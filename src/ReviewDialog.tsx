import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Modal from "./Modal";
import { report } from "./errors";
import { dirtyForms } from "./dirty";

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
  id: number; kind: "name" | "patronymic" | "note" | "skipped" | "duplicate" | "merge_edit" | "merge_deleted" | "merge_twin"; text: string;
  sheet: string | null; row: number | null; entry_id: number | null; done: boolean;
  section: number | null; year: number | null; page: string | null; no: number | null;
};

const KIND: Record<Item["kind"], string> = {
  skipped: "Строки, которые не перенесены",
  duplicate: "Одна запись в двух файлах",
  // Слияние с файлом помощника (спека 2026-10-17-sliyanie-prihodov).
  merge_edit: "Помощник поправил слитую запись",
  merge_deleted: "У вас удалено, у помощника есть",
  merge_twin: "Одно место в книге — две записи (ваша и помощника)",
  note: "Перенесено с оговоркой",
  name: "Имена, которых нет в словаре",
  patronymic: "Отчества, которых нет в словаре",
};
const ORDER: Item["kind"][] = ["skipped", "merge_deleted", "merge_edit", "merge_twin", "duplicate", "note", "name", "patronymic"];

/** Откуда строка: импорт Familio пишет лист как «РОЖДЕНИЕ | файл.xlsx». */
function source(sheet: string, row: number | null): string {
  const [name, file] = sheet.split(" | ");
  return file ? `файл «${file}», лист «${name}», строка ${row}` : `Excel: лист «${sheet}», строка ${row}`;
}
const SECTION = ["", "рождение", "брак", "смерть"];

export default function ReviewDialog({ onClose, onOpenEntry }: {
  onClose: () => void;
  /** Открыть запись в форме. Возвращает название формы, если она занята
   *  (в ней набрана или открыта другая запись) — тогда окно остаётся. */
  onOpenEntry: (section: number, id: number) => string | null;
}) {
  const [items, setItems] = useState<Item[]>([]);
  const [busyForm, setBusyForm] = useState<string | null>(null);
  const [showDone, setShowDone] = useState(false);

  function load(done: boolean) {
    invoke<Item[]>("review_list", { done })
      .then((rows) => {
        // Всё сверено — сразу показать сделанное: окно открыли кнопкой
        // «Список на сверку…», чтобы к нему вернуться (п. 125, 06.11.2026).
        if (!done && rows.length === 0 && !askedDone.current) {
          askedDone.current = true;
          setShowDone(true);
          return;
        }
        setItems(rows);
      })
      .catch((e) => report("Не удалось прочитать список на сверку", e));
  }
  const askedDone = useRef(false);
  useEffect(() => { load(showDone); }, [showDone]);

  /** «Взять правку» / «Вернуть запись»: запись помощника, ждавшая решения,
   *  становится записью прихода; строка уходит в сделанные. */
  const [taking, setTaking] = useState<number | null>(null);
  const [took, setTook] = useState<string | null>(null);
  /** Записи прихода менялись — при закрытии окно перечитывается: списки
   *  «Набрано» в формах иначе показывали бы прежнее. Формы при этом пусты
   *  (`take` иначе отказывает). */
  const tookAny = useRef(false);
  const close = () => {
    if (tookAny.current && dirtyForms().length === 0) window.location.reload();
    else onClose();
  };
  function take(item: Item) {
    if (taking !== null) return;
    // Запись, открытая в форме на правку, перезаписала бы взятую версию
    // помощника прежней (как у «Фамилий жён»).
    const forms = dirtyForms();
    if (forms.length) {
      setTook(`Сначала сохраните или очистите набранное: ${forms.join(", ")} — и возьмите запись помощника снова.`);
      return;
    }
    setTaking(item.id);
    invoke<{ entry_id: number; section: number; restored: boolean }>("merge_take", { reviewId: item.id })
      .then((t) => {
        setTook(t.restored ? "Запись помощника возвращена в приход." : "Запись заменена версией помощника.");
        tookAny.current = true;
        load(showDone);
      })
      .catch((e) => report("Не удалось взять запись помощника", e))
      .finally(() => setTaking(null));
  }

  function mark(item: Item, done: boolean) {
    invoke("review_done", { id: item.id, done })
      .then(() => load(showDone))
      .catch((e) => report("Не удалось отметить строку списка", e));
  }

  const open = items.filter((i) => !i.done).length;

  return (
    <Modal title={`На сверку после импорта: ${open}`} kind="review" onClose={close}>
      <p className="hint">
        Всё, что импорт или слияние с файлом помощника перенесли не дословно или не смогли сверить. «Открыть запись» —
        запись в форме (форма должна быть пустой); «Готово» — убрать строку из списка.
      </p>
      {took && <p className="hint" data-review-took>{took}</p>}
      {busyForm && (
        <p className="reviewbusy" data-review-busy>
          Форма «{busyForm}» занята: в ней набрана или открыта на правку другая запись. Закройте
          это окно, сохраните или очистите её — и откройте запись отсюда снова. Список никуда не денется.
        </p>
      )}
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
                        {i.sheet ? `${i.section ? " · " : ""}${source(i.sheet, i.row)}` : ""}
                      </div>
                    </td>
                    <td>
                      {i.entry_id !== null && i.section !== null && (
                        <button type="button" className="toggle small"
                                onClick={() => setBusyForm(onOpenEntry(i.section!, i.entry_id!))}>
                          Открыть запись
                        </button>
                      )}
                      {!i.done && (i.kind === "merge_edit" || i.kind === "merge_deleted") && (
                        <button type="button" className="toggle small" data-review-take disabled={taking !== null}
                                title={i.kind === "merge_edit"
                                  ? "Заменить вашу запись версией помощника — целиком"
                                  : "Завести запись помощника в приходе заново"}
                                onClick={() => take(i)}>
                          {i.kind === "merge_edit" ? "Взять правку" : "Вернуть запись"}
                        </button>
                      )}
                      <button type="button" className="toggle small" onClick={() => mark(i, !i.done)}
                              title={i.done ? undefined : i.kind === "merge_edit" ? "Оставить вашу запись как есть"
                                : i.kind === "merge_deleted" ? "Не возвращать запись" : undefined}>
                        {i.done ? "Вернуть в список" : i.kind === "merge_edit" ? "Оставить мою" : i.kind === "merge_deleted" ? "Не возвращать" : "Готово"}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        );
      })}
      {items.length === 0 && <p data-review-empty>Список пуст: на сверке ничего нет.</p>}
      <div className="modalbar">
        <button type="button" className="toggle" aria-pressed={showDone} onClick={() => setShowDone((v) => !v)}>
          {showDone ? "Скрыть сделанные" : "Показать сделанные"}
        </button>
        <button type="button" className="toggle" onClick={close}>Закрыть (Esc)</button>
      </div>
    </Modal>
  );
}
