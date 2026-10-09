import { FIELDS } from "./focus";
import { foldKey, useParishFlag } from "./flags";

/**
 * Сворачиваемый блок формы (Роман 03.10.2026, задача 4: «сворачиваемые блоки
 * для всех групп полей с памятью»).
 *
 * Не все набирают всех: кто-то не индексирует восприемников, у кого-то мать
 * всегда заполняется сама по отцу. Свёрнутый блок — одна строка, его полей
 * нет на экране и в обходе клавишами; высота — самый дефицитный ресурс формы.
 *
 * Свёрнутый блок показывает, что в нём стоит: мать подставляется по отцу,
 * запись на правку приходит с данными — молча спрятанные данные хуже лишней
 * строки. Данные свёрнутого блока сохраняются как обычно.
 *
 * Блок при этом не убирается из окна, а прячется (`hidden`): уберёшь — и
 * он забудет, что звание стёрли руками, и подставит его снова при
 * разворачивании (ревьюер 09.10.2026). Обход клавишами скрытые поля
 * пропускает сам (focus.ts смотрит на `offsetParent`).
 *
 * Состояние помнится в приходе (ключ `fold_<блок>`), изначально развёрнут.
 */
export function useFold(block: string, title: string): [boolean, (folded: boolean) => void] {
  return useParishFlag(foldKey(block), `свернуть блок «${title}»`, false);
}

/**
 * Щелчок по «свернуть» и «развернуть» не забирает фокус у поля (Роман
 * 30.09.2026: «после клика мышью невозможно сразу продолжить навигацию по
 * форме с помощью клавиатуры»). Если поле с курсором ушло со сворачиваемым
 * блоком — курсор встаёт в первое поле после него.
 */
function keepFocus(e: React.MouseEvent) {
  e.preventDefault();
}
function refocus(block: string) {
  setTimeout(() => {
    const active = document.activeElement as HTMLElement | null;
    if (active && active !== document.body && active.offsetParent !== null) return;
    const line = document.querySelector<HTMLElement>(`[data-fold="${block}"]`);
    const root = line?.closest(".formroot");
    if (!line || !root || line.offsetParent === null) return;
    const next = Array.from(root.querySelectorAll<HTMLInputElement>(FIELDS))
      .filter((el) => !el.disabled && el.offsetParent !== null && !el.hasAttribute("data-skip"))
      .find((el) => line.compareDocumentPosition(el) & Node.DOCUMENT_POSITION_FOLLOWING);
    (next ?? root.querySelector<HTMLButtonElement>(".savebar button"))?.focus();
  }, 0);
}

/** Строка свёрнутого блока. */
export function FoldLine({ block, title, summary, onOpen }: {
  block: string; title: string; summary: string; onOpen: () => void;
}) {
  return (
    <section className="person folded" data-fold={block}>
      <div className="foldline">
        <h2>{title}</h2>
        <span className={summary ? "foldsummary has" : "foldsummary"} title={summary || undefined}>
          {summary || "свёрнуто"}
        </span>
        <button type="button" className="linkish" tabIndex={-1} data-fold-open={block}
                onMouseDown={keepFocus} onClick={onOpen}>
          развернуть
        </button>
      </div>
    </section>
  );
}

/** Ссылка «свернуть» в заголовке развёрнутого блока; вне обхода клавишами. */
export function FoldLink({ block, onFold }: { block: string; onFold: () => void }) {
  return (
    <button type="button" className="linkish" tabIndex={-1} data-fold-close={block}
            onMouseDown={keepFocus} onClick={() => { onFold(); refocus(block); }}>
      свернуть
    </button>
  );
}

/** Что стоит в свёрнутом блоке: ИОФ и НП каждой персоны. НП без ИОФ не
 *  показывается: мать получает НП отца и без имени, а без имени она не
 *  сохранится — строка «Малово» выглядела бы заполненной матерью. */
export function foldSummary(people: { iof: string; place?: string }[]): string {
  return people
    .filter((p) => p.iof.trim() !== "")
    .map((p) => [p.iof.trim(), (p.place ?? "").trim()].filter(Boolean).join(", "))
    .join("; ");
}
