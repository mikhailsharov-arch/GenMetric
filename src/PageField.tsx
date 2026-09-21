import { focusNextField } from "./focus";
import { stepPage } from "./page";

/**
 * Поле номера страницы: текст, а не число.
 *
 * Заказчик 21.09.2026: номера вида «938об-939» — разворот, оборот листа
 * и следующий лист. Кнопки «−»/«+» и Alt+стрелки шагают по всем числам
 * в строке сразу (src/page.ts). Внешне и по клавишам — как NumberField,
 * чтобы рука не замечала разницы.
 */
type Props = {
  label: string;
  value: string | null;
  onChange: (v: string | null) => void;
  width?: string;
};

export default function PageField({ label, value, onChange, width }: Props) {
  function step(delta: number) {
    onChange(stepPage(value, delta) || null);
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
    <div className="field num" style={width ? { width } : undefined}>
      <label>{label}</label>
      <div className="fieldbody numrow">
        <button type="button" onClick={() => step(-1)} tabIndex={-1} aria-label="Меньше">
          −
        </button>
        <input
          data-field
          value={value ?? ""}
          placeholder="938об-939"
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => onChange(e.target.value === "" ? null : e.target.value)}
          onBlur={(e) => onChange(e.target.value.trim() === "" ? null : e.target.value.trim())}
          onKeyDown={onKeyDown}
        />
        <button type="button" onClick={() => step(1)} tabIndex={-1} aria-label="Больше">
          +
        </button>
      </div>
    </div>
  );
}
