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
  /** «Помощник поправил запись» по полям (merge::edit_view, пп. 156–157). */
  view: EditView | null;
};
type Piece = { key: string; who: string[] };
type EditView = { head: string; pieces: Piece[]; other: string[] };
type ManyDone = { closed: number; left: number; failed: string[]; copy: string | null };
/** Строк в одном вызове «во всех»: между частями окно показывает ход (п. 152). */
const CHUNK = 25;
const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

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

  /** Одна правка помощника во многих записях — разом (Роман 12.11.2026:
   *  1761 строка «с.Горелое» → «Горелое»; 16.11.2026: и в записях, где
   *  помощник поправил ещё что-то, — берётся только эта правка). */
  const [openGroups, setOpenGroups] = useState<Set<string>>(new Set());
  /** Группа, для которой нажали «Взять правку во всех» один раз: второе
   *  нажатие — «Да, взять в N записях» (как у замены в записях). */
  const [sureGroup, setSureGroup] = useState<string | null>(null);
  /** Ход действия «во всех» (п. 152): кнопки группы заменены полосой. */
  const [progress, setProgress] = useState<{ key: string; done: number; total: number; take: boolean } | null>(null);
  /** Итог (п. 153): плашка наверху окна, до его закрытия. */
  const [result, setResult] = useState<{ text: string; copy: string | null } | null>(null);
  async function runMany(key: string, rows: Item[], take: boolean) {
    if (taking !== null) return;
    if (take && sureGroup !== key) { setSureGroup(key); return; }
    setSureGroup(null);
    const forms = dirtyForms();
    if (forms.length) {
      setTook(`Сначала сохраните или очистите набранное: ${forms.join(", ")} — и решите правки помощника снова.`);
      return;
    }
    setTaking(-1);
    setResult(null);
    const ids = rows.map((r) => r.id);
    const sum: ManyDone = { closed: 0, left: 0, failed: [], copy: null };
    // Ход — только если действие идёт дольше полсекунды (Роман: «Оставить
    // мою» мгновенное, индикатор ему не нужен).
    const slow = window.setTimeout(() => setProgress((p) => p ?? { key, done: 0, total: ids.length, take }), 500);
    try {
      for (let i = 0; i < ids.length; i += CHUNK) {
        const r = await invoke<ManyDone>("merge_piece_many", { reviewIds: ids.slice(i, i + CHUNK), key, take, backup: i === 0 });
        sum.closed += r.closed; sum.left += r.left; sum.failed.push(...r.failed); sum.copy = sum.copy ?? r.copy;
        if (take) tookAny.current = tookAny.current || r.closed + r.left > 0;
        const done = Math.min(i + CHUNK, ids.length);
        setProgress((p) => p && { ...p, done });
      }
    } catch (e) {
      report(take ? "Не удалось взять правки помощника" : "Не удалось оставить ваши записи", e);
    } finally {
      window.clearTimeout(slow);
      setProgress(null);
      setTaking(null);
    }
    const n = sum.closed + sum.left;
    const failed = sum.failed.length
      ? ` Не вышло в ${sum.failed.length}: ${[...new Set(sum.failed)].slice(0, 2).join("; ")} — эти строки решите по одной.` : "";
    setResult({
      text: (take ? `✓ Правка помощника взята в ${n} записях.` : `✓ Оставлена ваша версия в ${n} записях.`) +
            (sum.left ? ` В ${sum.left} из них помощник изменил и другие поля — они остались ниже, только с этими отличиями.` : "") + failed,
      copy: sum.copy,
    });
    load(showDone);
    refocus();
  }
  /** Кнопки группы исчезают вместе с ней — курсор возвращается в окно, иначе
   *  Esc уходил в никуда и окно не закрывалось (стенд 13.11.2026). */
  const refocus = () => document.querySelector<HTMLElement>('.modal[data-modal="review"]')?.focus();

  function mark(item: Item, done: boolean) {
    invoke("review_done", { id: item.id, done })
      .then(() => load(showDone))
      .catch((e) => report("Не удалось отметить строку списка", e));
  }

  const open = items.filter((i) => !i.done).length;

  const toggleGroup = (key: string) => setOpenGroups((o) => {
    const n = new Set(o);
    if (n.has(key)) n.delete(key); else n.add(key);
    return n;
  });
  const openEntry = (i: Item) => i.entry_id !== null && i.section !== null && (
    <button type="button" className="toggle small" onClick={() => setBusyForm(onOpenEntry(i.section!, i.entry_id!))}>
      Открыть запись
    </button>
  );

  return (
    <Modal title={`На сверку после импорта: ${open}`} kind="review" onClose={close}>
      <div className="reviewtop">
        <p className="hint">
          Всё, что импорт или слияние с файлом помощника перенесли не дословно или не смогли сверить. «Открыть запись» —
          запись в форме (форма должна быть пустой); «Готово» — убрать строку из списка.
        </p>
        {result && (
          <p className="reviewresult" data-review-result>
            {result.text}
            {result.copy && (
              <>
                {" "}Копия прихода до этого: «{fileName(result.copy)}»{" "}
                <button type="button" className="linkish" onClick={() => void invoke("reveal_path", { path: result.copy })
                  .catch((e) => report("Не удалось открыть папку", e))}>Показать в папке</button>
              </>
            )}
          </p>
        )}
        {took && <p className="hint" data-review-took>{took}</p>}
        {busyForm && (
          <p className="reviewbusy" data-review-busy>
            Форма «{busyForm}» занята: в ней набрана или открыта на правку другая запись. Закройте
            это окно, сохраните или очистите её — и откройте запись отсюда снова. Список никуда не денется.
          </p>
        )}
      </div>
      <div className="reviewscroll">
      {ORDER.map((kind) => {
        const rows = items.filter((i) => i.kind === kind);
        if (!rows.length) return null;
        // Одна правка в двух записях и больше — блоком сверху (п. 157: и в
        // записях с другими отличиями).
        const groups = new Map<string, Item[]>();
        for (const i of rows) if (!i.done) for (const p of i.view?.pieces ?? []) groups.set(p.key, [...(groups.get(p.key) ?? []), i]);
        const big = [...groups.entries()].filter(([, g]) => g.length > 1).sort((a, b) => b[1].length - a[1].length);
        const inBig = new Set(big.map(([k]) => k));
        // Строка, все правки которой — в группах, отдельно не показывается.
        const shown = rows.filter((i) => !i.view || i.done || i.view.other.length > 0 || i.view.pieces.some((p) => !inBig.has(p.key)));
        return (
          <div key={kind}>
            <h3>{KIND[kind]}: {rows.filter((i) => !i.done).length}</h3>
            {kind === "merge_edit" && (
              <p className="hint">
                «Взять правку» в строке заменяет вашу запись версией помощника целиком. «Взять правку во всех» в
                группе меняет только поля своей правки — остальное в записях остаётся как было.
              </p>
            )}
            {big.map(([key, g]) => {
              const more = g.filter((i) => i.view!.pieces.length > 1 || i.view!.other.length > 0).length;
              const run = progress?.key === key ? progress : null;
              return (
                <div key={key} className="reviewgroup" data-review-group={g.length}>
                  <p>
                    <b>Одна и та же правка в {g.length} записях:</b> {key}
                    {more > 0 && <> (в {more} из них есть и другие отличия)</>}
                  </p>
                  {run ? (
                    <div className="reviewprogress" data-review-progress>
                      {run.take ? `Беру правку помощника: ${run.done} из ${run.total}` : `Оставляю вашу версию: ${run.done} из ${run.total}`}
                      <progress max={run.total} value={run.done} />
                    </div>
                  ) : (
                    <div className="row">
                      <button type="button" className={sureGroup === key ? "danger small" : "toggle small"} data-review-take-all
                              disabled={taking !== null}
                              title="Взять у помощника только эту правку во всех этих записях; перед этим программа делает копию прихода"
                              onClick={() => void runMany(key, g, true)}>
                        {sureGroup === key ? `Да, взять в ${g.length} записях` : `Взять правку во всех (${g.length})`}
                      </button>
                      {sureGroup === key && (
                        <button type="button" className="toggle small" onClick={() => setSureGroup(null)}>Отмена</button>
                      )}
                      <button type="button" className="toggle small" data-review-keep-all disabled={taking !== null}
                              title="Оставить вашу версию этого поля во всех этих записях" onClick={() => void runMany(key, g, false)}>
                        Оставить мою во всех ({g.length})
                      </button>
                      <button type="button" className="linkish" data-review-show-group aria-expanded={openGroups.has(key)}
                              onClick={() => toggleGroup(key)}>
                        {openGroups.has(key) ? "Скрыть записи ▴" : `Показать записи (${g.length}) ▾`}
                      </button>
                    </div>
                  )}
                  {openGroups.has(key) && (
                    <ul className="reviewgroupitems" data-review-group-items>
                      {g.map((i) => (
                        <li key={i.id}>
                          <span>{i.view!.head}</span>
                          {openEntry(i)}
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              );
            })}
            <table className="facts review">
              <tbody>
                {shown.map((i) => (
                  <tr key={i.id} className={i.done ? "done" : ""}>
                    <td>
                      {i.view ? (
                        <div data-review-edit>
                          <b>{i.view.head}</b>
                          {i.view.pieces.map((p) => (
                            <div key={p.key}>{p.key}{p.who.length ? `: ${p.who.join(", ")}` : ""}</div>
                          ))}
                          {i.view.other.map((o) => <div key={o}>{o}</div>)}
                        </div>
                      ) : (
                        <>
                          {i.text}
                          <div className="fieldhint">
                            {i.section ? `${SECTION[i.section]} ${i.year ?? "без года"}` : ""}
                            {i.no !== null ? `, № ${i.no}` : ""}{i.page ? `, стр. ${i.page}` : ""}
                            {i.sheet ? `${i.section ? " · " : ""}${source(i.sheet, i.row)}` : ""}
                          </div>
                        </>
                      )}
                      <div className="row reviewbuttons">
                        {openEntry(i)}
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
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        );
      })}
      {items.length === 0 && <p data-review-empty>Список пуст: на сверке ничего нет.</p>}
      </div>
      <div className="modalbar">
        <button type="button" className="toggle" aria-pressed={showDone} onClick={() => setShowDone((v) => !v)}>
          {showDone ? "Скрыть сделанные" : "Показать сделанные"}
        </button>
        <button type="button" className="toggle" onClick={close}>Закрыть (Esc)</button>
      </div>
    </Modal>
  );
}
