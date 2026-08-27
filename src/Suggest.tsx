import { forwardRef, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { focusNextField } from "./focus";
import { report } from "./errors";

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
  const seq = useRef(0);
  // После подстановки поле меняется программно, и запрос подсказок не должен
  // открывать список заново — иначе он «залипает» открытым (баг 1 из отчёта).
  const justPicked = useRef(false);

  /**
   * Закрывает список и отменяет уже отправленные запросы.
   *
   * Увеличенный счётчик обязателен: запрос уходит на каждое нажатие, и на
   * момент выбора предыдущий ещё в пути. Без отмены он возвращается и открывает
   * список заново. В поле ИОФ это было видно заказчику, здесь ответы приходят
   * быстрее и поломка просто не успевала проявиться — но она та же.
   */
  function closeList() {
    seq.current += 1;
    setItems([]);
    setOpen(false);
  }

  useEffect(() => {
    const query = value.trim();
    if (query.length < 1) {
      closeList();
      return;
    }
    if (justPicked.current) {
      justPicked.current = false;
      closeList();
      return;
    }
    const mine = ++seq.current;
    invoke<Item[]>("suggest", { kind, prefix: query, limit: 8 })
      .then((rows) => {
        if (mine !== seq.current) return; // ответ на устаревший запрос
        setItems(rows);
        setActive(0);
        // Единственный вариант, совпадающий с набранным, показывать незачем.
        const exact = rows.length === 1 && rows[0].value.toLowerCase() === query.toLowerCase();
        setOpen(rows.length > 0 && !exact);
      })
      .catch((e) => {
        if (mine !== seq.current) return;
        setItems([]);
        setOpen(false);
        report(`Не удалось получить подсказки для поля «${label}»`, e);
      });
  }, [value, kind]);

  function pick(item: Item) {
    justPicked.current = true;
    closeList();
    onChange(item.value);
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
      closeList();
    }
  }

  return (
    <div className="field">
      <label>{label}</label>
      <div className="fieldbody">
      <input
        ref={ref}
        data-field
        value={value}
        placeholder={placeholder}
        autoComplete="off"
        spellCheck={false}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={onKeyDown}
        onBlur={() => closeList()}
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
    </div>
  );
});

export default Suggest;
