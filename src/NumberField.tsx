import { useRef } from "react";
import { focusNextField } from "./focus";

/**
 * Числовое поле с кнопками шага.
 *
 * Единственное, что Роман добавил от себя в опроснике: «чтобы у всех числовых
 * значений были кнопки слева минус, а справа плюс, нажимая на которые
 * изменялось число, чтобы не перенабивать его вручную». Номер страницы,
 * счёт записей, день и месяц меняются почти каждую запись на единицу.
 *
 * С клавиатуры то же самое делают стрелки влево и вправо — рука не отрывается.
 */
type Props = {
  label: string;
  value: number | null;
  onChange: (v: number | null) => void;
  min?: number;
  max?: number;
  width?: string;
  inputRef?: React.RefObject<HTMLInputElement>;
  /** Вне обхода клавишей Tab: поле подставляется само (месяц крещения,
   *  месяц погребения), править его — мышью (Роман 03.10.2026). */
  noTab?: boolean;
};

export default function NumberField({
  label, value, onChange, min = 0, max = 9999, width, inputRef, noTab,
}: Props) {
  const own = useRef<HTMLInputElement>(null);
  const field = inputRef ?? own;

  function step(delta: number) {
    const base = value ?? (delta > 0 ? min - 1 : min + 1);
    const next = Math.min(max, Math.max(min, base + delta));
    onChange(next);
  }

  /** Щелчок по «+» / «−» оставляет фокус в поле: Роман 30.09.2026 — «после
   *  клика мышью невозможно сразу продолжить навигацию по форме с помощью
   *  клавиатуры». С клавиатуры кнопки по-прежнему не в обходе (tabIndex −1). */
  function click(delta: number) {
    step(delta);
    field.current?.focus();
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "ArrowLeft" && e.altKey) {
      e.preventDefault();
      step(-1);
    } else if (e.key === "ArrowRight" && e.altKey) {
      e.preventDefault();
      step(1);
    } else if (e.key === "Enter" || e.key === "ArrowDown") {
      e.preventDefault();
      // Shift+Enter — назад: заказчик 15.09.2026 возвращался в пропущенное
      // поле мышью. Стрелка вверх делала это и раньше, но её не нашли.
      focusNextField(e.currentTarget, e.shiftKey && e.key === "Enter" ? -1 : 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      focusNextField(e.currentTarget, -1);
    }
  }

  return (
    <div className="field num" style={width ? { width } : undefined}>
      <label>{label}</label>
      <div className="fieldbody numrow">
        <button type="button" onClick={() => click(-1)} tabIndex={-1} aria-label="Меньше">
          −
        </button>
        <input
          ref={field}
          data-field
          tabIndex={noTab ? -1 : undefined}
          inputMode="numeric"
          value={value ?? ""}
          onChange={(e) => {
            const raw = e.target.value.replace(/[^0-9]/g, "");
            onChange(raw === "" ? null : Number(raw));
          }}
          onKeyDown={onKeyDown}
        />
        <button type="button" onClick={() => click(1)} tabIndex={-1} aria-label="Больше">
          +
        </button>
      </div>
    </div>
  );
}
