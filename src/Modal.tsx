import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";

/**
 * Модальное окно поверх формы: сверка имени, карточка населённого пункта.
 *
 * Правила одни для всех окон. Esc закрывает. Фокус при открытии — на первое
 * поле внутри. Enter и стрелки ходят только по полям окна (data-focus-scope,
 * см. focus.ts), чтобы не уйти в форму под ним. Ctrl+Enter — сохранение
 * записи — внутри окна не действует: событие не пускается выше.
 */
/** Сколько после открытия окно не принимает набор, мс. Человек замечает
 *  окно и останавливается примерно за полсекунды. */
const GUARD_MS = 350;

type Props = {
  title: string;
  onClose: () => void;
  children: React.ReactNode;
  /** Для стенда и e2e: чем это окно является. */
  kind: string;
};

export default function Modal({ title, onClose, children, kind }: Props) {
  const box = useRef<HTMLDivElement | null>(null);
  const openedAt = useRef(performance.now());

  useEffect(() => {
    // Первым — список похожих, если он есть (Enter выбирает), иначе первое
    // поле. Кнопки внизу фокус при открытии не получают.
    // То же для ввода, пришедшего без нажатия клавиши (раскладка, вставка,
    // IME): beforeinput. Первые GUARD_MS окно текст не принимает.
    const el = box.current;
    const guard = (e: Event) => {
      if (performance.now() - openedAt.current < GUARD_MS) e.preventDefault();
    };
    el?.addEventListener("beforeinput", guard, true);
    // Скрытое поле выбора файла — не поле. Полей нет (окно «Приходы») —
    // фокус на само окно: иначе он остаётся под ним, и Esc до окна не доходит.
    const first = box.current?.querySelector<HTMLElement>(
      "[data-autofocus], [data-similar], input:not([type='file']):not([hidden])") ?? box.current;
    first?.focus();
    if (first instanceof HTMLInputElement) first.select();
    return () => el?.removeEventListener("beforeinput", guard, true);
  }, []);

  // Окно — прямо в body, а не внутри блока персоны: иначе его положение
  // и размер зависят от предков (container queries, будущие transform) —
  // на macOS/WKWebView не проверено (ревьюер 27.09.2026).
  return createPortal(
    <div
      className="overlay"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="modal"
        role="dialog"
        aria-label={title}
        data-focus-scope
        data-modal={kind}
        tabIndex={-1}
        ref={box}
        onKeyDownCapture={(e) => {
          // Первые мгновения после открытия буквы и Enter в окно не идут: это
          // быстрый набор, начатый до появления окна, — он предназначался
          // форме (техдолг: «набор после Enter уходит в поиск окна сверки»).
          if (((e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) || e.key === "Enter")
              && performance.now() - openedAt.current < GUARD_MS) {
            e.preventDefault();
            e.stopPropagation();
          }
        }}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            onClose();
          } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
            e.stopPropagation();
          } else if (e.key === "Tab") {
            // Tab не выходит из окна: с последней кнопки — на первое поле,
            // Shift+Tab с первого — на последнюю (ревьюер 23.09.2026).
            const items = Array.from(box.current?.querySelectorAll<HTMLElement>(
              "input:not([disabled]), button:not([disabled]), [tabindex='0']") ?? [])
              .filter((el) => el.offsetParent !== null);
            if (!items.length) return;
            const i = items.indexOf(document.activeElement as HTMLElement);
            if (!e.shiftKey && (i === items.length - 1 || i === -1)) { e.preventDefault(); items[0].focus(); }
            else if (e.shiftKey && i <= 0) { e.preventDefault(); items[items.length - 1].focus(); }
          }
        }}
      >
        <h2>{title}</h2>
        {children}
      </div>
    </div>,
    document.body,
  );
}
