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
};

export default function NumberField({ label, value, onChange, min = 0, max = 9999, width }: Props) {
  function step(delta: number) {
    const base = value ?? (delta > 0 ? min - 1 : min + 1);
    const next = Math.min(max, Math.max(min, base + delta));
    onChange(next);
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
      focusNextField(e.currentTarget);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      focusNextField(e.currentTarget, -1);
    }
  }

  return (
    <div className="field num" style={width ? { width } : undefined}>
      <label>{label}</label>
      <div className="numrow">
        <button type="button" onClick={() => step(-1)} tabIndex={-1} aria-label="Меньше">
          −
        </button>
        <input
          data-field
          inputMode="numeric"
          value={value ?? ""}
          onChange={(e) => {
            const raw = e.target.value.replace(/[^0-9]/g, "");
            onChange(raw === "" ? null : Number(raw));
          }}
          onKeyDown={onKeyDown}
        />
        <button type="button" onClick={() => step(1)} tabIndex={-1} aria-label="Больше">
          +
        </button>
      </div>
    </div>
  );
}
