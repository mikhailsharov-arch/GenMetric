import { useRef, useState } from "react";
import { focusNextField } from "./focus";

/**
 * Возраст жениха и невесты в годах — как в книге, с половиной года: «17,5»
 * (Роман 02.11.2026: «в метрических книгах регулярно встречаются дробные
 * значения (с половинами лет)»). Тот же вид и те же клавиши, что у числового
 * поля (`NumberField`): «−» и «+», Alt+← и Alt+→ меняют целые годы, дробь
 * остаётся. Значение — текст: годы и после запятой одна-две цифры; точка
 * становится запятой.
 */
type Props = {
  label: string;
  value: string;
  onChange: (v: string) => void;
};

// Запятая — только после лет: «,5» без лет не набирается (проверяющий 02.11.2026).
const SHAPE = /^(\d{1,3}(,\d{0,2})?)?$/;

export default function AgeYearsField({ label, value, onChange }: Props) {
  const field = useRef<HTMLInputElement>(null);
  const [rejected, setRejected] = useState(false);
  const rejectTimer = useRef<number | undefined>(undefined);
  function reject() {
    setRejected(true);
    window.clearTimeout(rejectTimer.current);
    rejectTimer.current = window.setTimeout(() => setRejected(false), 600);
  }

  function step(delta: number) {
    const [whole, frac] = value.split(",");
    const base = whole === "" || whole === undefined ? (delta > 0 ? 0 : 2) : Number(whole);
    const next = Math.min(100, Math.max(1, base + delta));
    onChange(frac !== undefined && frac !== "" ? `${next},${frac}` : String(next));
  }

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
      focusNextField(e.currentTarget, e.shiftKey && e.key === "Enter" ? -1 : 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      focusNextField(e.currentTarget, -1);
    }
  }

  return (
    <div className="field num">
      <label>{label}</label>
      <div className="fieldbody numrow">
        <button type="button" onClick={() => click(-1)} tabIndex={-1} aria-label="Меньше">
          −
        </button>
        <input
          ref={field}
          data-field
          data-age-years
          inputMode="decimal"
          className={rejected ? "rejected" : undefined}
          value={value}
          onChange={(e) => {
            const raw = e.target.value.replace(/\./g, ",").replace(/\s/g, "");
            const whole = raw.split(",")[0];
            // Лет от 1 до 100: «0» и «101» — опечатка.
            if (!SHAPE.test(raw) || (whole !== "" && (Number(whole) > 100 || whole.startsWith("0")))) {
              reject();
              return;
            }
            onChange(raw);
          }}
          onBlur={(e) => {
            // Потеря фокуса окном программы (щелчок в окне скана) — не уход из поля.
            if (e.relatedTarget === null && !document.hasFocus()) return;
            // Запятая без дроби («17,») — просто годы.
            if (value.endsWith(",")) onChange(value.slice(0, -1));
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
