import { forwardRef, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Item = { value: string; tier: number; count: number };

type Props = {
  label: string;
  kind: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
};

const TIER_TITLE: Record<number, string> = {
  1: "в этом деле",
  2: "в приходе",
  3: "в базе",
  4: "словарь",
};

/**
 * Поле с автоподстановкой.
 *
 * Управление целиком с клавиатуры: стрелки вверх и вниз выбирают вариант,
 * Enter подставляет, Escape закрывает список. Мышь не требуется.
 */
const Suggest = forwardRef<HTMLInputElement, Props>(function Suggest(
  { label, kind, value, onChange, placeholder },
  ref,
) {
  const [items, setItems] = useState<Item[]>([]);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [ms, setMs] = useState<number | null>(null);
  const seq = useRef(0);

  useEffect(() => {
    const query = value.trim();
    if (query.length < 1) {
      setItems([]);
      setOpen(false);
      return;
    }
    const mine = ++seq.current;
    const started = performance.now();
    invoke<Item[]>("suggest", { kind, prefix: query, limit: 8 })
      .then((rows) => {
        if (mine !== seq.current) return; // пришёл ответ на устаревший запрос
        setMs(Math.round(performance.now() - started));
        setItems(rows);
        setActive(0);
        setOpen(rows.length > 0);
      })
      .catch(() => setItems([]));
  }, [value, kind]);

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (!open || items.length === 0) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((i) => (i + 1) % items.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((i) => (i - 1 + items.length) % items.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      onChange(items[active].value);
      setOpen(false);
    } else if (e.key === "Escape") {
      setOpen(false);
    }
  }

  return (
    <div className="field">
      <label>
        {label}
        {ms !== null && <span className="ms">{ms} мс</span>}
      </label>
      <input
        ref={ref}
        value={value}
        placeholder={placeholder}
        autoComplete="off"
        spellCheck={false}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={onKeyDown}
      />
      {open && (
        <ul className="suggest">
          {items.map((it, i) => (
            <li
              key={it.value}
              className={i === active ? "active" : ""}
              onMouseDown={(e) => {
                e.preventDefault();
                onChange(it.value);
                setOpen(false);
              }}
            >
              <span className="val">{it.value}</span>
              <span className={`tier t${it.tier}`}>{TIER_TITLE[it.tier]}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
});

export default Suggest;
