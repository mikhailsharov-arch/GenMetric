import { useEffect, useRef } from "react";

/**
 * Модальное окно поверх формы: сверка имени, карточка населённого пункта.
 *
 * Правила одни для всех окон. Esc закрывает. Фокус при открытии — на первое
 * поле внутри. Enter и стрелки ходят только по полям окна (data-focus-scope,
 * см. focus.ts), чтобы не уйти в форму под ним. Ctrl+Enter — сохранение
 * записи — внутри окна не действует: событие не пускается выше.
 */
type Props = {
  title: string;
  onClose: () => void;
  children: React.ReactNode;
  /** Для стенда и e2e: чем это окно является. */
  kind: string;
};

export default function Modal({ title, onClose, children, kind }: Props) {
  const box = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    // Первым — список похожих, если он есть (Enter выбирает), иначе первое
    // поле. Кнопки внизу фокус при открытии не получают.
    const first = box.current?.querySelector<HTMLElement>("[data-similar], input");
    first?.focus();
    if (first instanceof HTMLInputElement) first.select();
  }, []);

  return (
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
        ref={box}
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
    </div>
  );
}
