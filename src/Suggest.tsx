import { forwardRef, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { focusNextField } from "./focus";

export type Item = { value: string; tier: number; count: number };

type Props = {
  label: string;
  kind: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  hint?: string;
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
 * Управление целиком с клавиатуры. Пока список открыт: стрелки выбирают
 * вариант, Enter подставляет, Escape закрывает. Когда список закрыт, Enter
 * и стрелка вниз переводят фокус на следующее поле — как в Excel-Индексаторе,
 * где переход шёл клавишей «вниз».
 */
const Suggest = forwardRef<HTMLInputElement, Props>(function Suggest(
  { label, kind, value, onChange, placeholder, hint },
  ref,
) {
  const [items, setItems] = useState<Item[]>([]);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [ms, setMs] = useState<number | null>(null);
  const seq = useRef(0);
  // После подстановки поле меняется программно, и запрос подсказок не должен
  // открывать список заново — иначе он «залипает» открытым (баг 1 из отчёта).
  const justPicked = useRef(false);

  useEffect(() => {
    const query = value.trim();
    if (query.length < 1) {
      setItems([]);
      setOpen(false);
      return;
    }
    if (justPicked.current) {
      justPicked.current = false;
      setItems([]);
      setOpen(false);
      return;
    }
    const mine = ++seq.current;
    const started = performance.now();
    invoke<Item[]>("suggest", { kind, prefix: query, limit: 8 })
      .then((rows) => {
        if (mine !== seq.current) return; // ответ на устаревший запрос
        setMs(Math.round(performance.now() - started));
        setItems(rows);
        setActive(0);
        // Единственный вариант, совпадающий с набранным, показывать незачем.
        const exact = rows.length === 1 && rows[0].value.toLowerCase() === query.toLowerCase();
        setOpen(rows.length > 0 && !exact);
      })
      .catch(() => setItems([]));
  }, [value, kind]);

  function pick(item: Item) {
    justPicked.current = true;
    onChange(item.value);
    setItems([]);
    setOpen(false);
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    const listOpen = open && items.length > 0;

    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (listOpen) setActive((i) => (i + 1) % items.length);
      else focusNextField(e.currentTarget);
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      if (listOpen) setActive((i) => (i - 1 + items.length) % items.length);
      else focusNextField(e.currentTarget, -1);
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      if (listOpen) pick(items[active]);
      else focusNextField(e.currentTarget);
      return;
    }
    if (e.key === "Escape") {
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
        data-field
        value={value}
        placeholder={placeholder}
        autoComplete="off"
        spellCheck={false}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={onKeyDown}
        onBlur={() => setOpen(false)}
      />
      {hint && <div className="fieldhint">{hint}</div>}
      {open && (
        <ul className="suggest">
          {items.map((it, i) => (
            <li
              key={it.value}
              className={i === active ? "active" : ""}
              onMouseDown={(e) => {
                e.preventDefault();
                pick(it);
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
