import { useRef, useState } from "react";
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
  /** Вне обхода клавишами перехода — Tab, Enter, стрелки: поле подставляется
   *  само (месяц крещения, месяц погребения), править его — мышью (Роман
   *  03.10 и 05.10.2026). */
  noTab?: boolean;
};

export default function NumberField({
  label, value, onChange, min = 0, max: limit, width, inputRef, noTab,
}: Props) {
  const max = limit ?? 9999;
  const own = useRef<HTMLInputElement>(null);
  const field = inputRef ?? own;
  // Набранное не принято — поле коротко мигает: молча отброшенная цифра
  // выглядела как залипшая клавиша (проверяющий 05.10.2026).
  const [rejected, setRejected] = useState(false);
  const rejectTimer = useRef<number | undefined>(undefined);
  function reject() {
    setRejected(true);
    window.clearTimeout(rejectTimer.current);
    rejectTimer.current = window.setTimeout(() => setRejected(false), 600);
  }

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
          data-skip={noTab ? "" : undefined}
          inputMode="numeric"
          className={rejected ? "rejected" : undefined}
          value={value ?? ""}
          onChange={(e) => {
            const raw = e.target.value.replace(/[^0-9]/g, "");
            // Больше предела не набирается: месяц 13 или день 32 — опечатка
            // (Роман 05.10.2026). Поле остаётся с прежним значением.
            // Только где предел задан явно (день, месяц, год): счёт не ограничен.
            if (raw !== "" && limit !== undefined && Number(raw) > limit) {
              // Цифра дописана в конец уже полного значения — после щелчка
              // мышью курсор стоит в конце, и «15» + «7» давало «157», то есть
              // ничего. Человек набирает новое число: начинаем его заново.
              const old = value === null ? "" : String(value);
              const tail = old !== "" && raw.startsWith(old) ? raw.slice(old.length) : "";
              // Хвост «0» числом не становится: «4» + «0» — это «40», а не новый «0».
              // И не меньше наименьшего: «1897» + «7» — это не год 7 (ревьюер
              // 06.10.2026); у года новое число так не начать — поле мигает.
              if (tail !== "" && Number(tail) >= Math.max(1, min) && Number(tail) <= limit) onChange(Number(tail));
              else reject();
              return;
            }
            onChange(raw === "" ? null : Number(raw));
          }}
          onBlur={() => {
            // Ноль — не день и не месяц. При наборе он нужен («05»), а
            // оставшийся при уходе из поля — стирается, и поле мигает.
            if (value === 0 && min >= 1) {
              onChange(null);
              reject();
            }
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
