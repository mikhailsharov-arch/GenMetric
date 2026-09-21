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
  /** Кнопка «▾» и Alt+↓: показать весь перечень, не набирая ни буквы.
   *  Заказчик 21.09.2026 про архив: «в индексаторе Excel же есть список
   *  архивов, надо сделать, чтобы можно было выбрать из выпадающего списка». */
  browse?: boolean;
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
  { label, kind, value, onChange, placeholder, hint, browse },
  ref,
) {
  const [items, setItems] = useState<Item[]>([]);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const seq = useRef(0);
  // После подстановки поле меняется программно, и запрос подсказок не должен
  // открывать список заново — иначе он «залипает» открытым (баг 1 из отчёта).
  const justPicked = useRef(false);
  // Список открывается только на набор с клавиатуры. Значение меняется и
  // программно — выбор персоны подставляет НП и звание, выбор отца — жену, —
  // и без этого у каждого подставленного поля открывался свой список.
  // Заказчик 15.09.2026: «открывается сразу несколько списков, которые
  // приходится протыкивать мышкой».
  const typed = useRef(false);

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

  /** Весь перечень целиком: своё и частое — сверху, остальное по алфавиту. */
  function browseAll() {
    const mine = ++seq.current;
    invoke<Item[]>("suggest", { kind, prefix: "", limit: 200 })
      .then((rows) => {
        if (mine !== seq.current) return;
        setItems(rows);
        setActive(Math.max(0, rows.findIndex((r) => r.value === value)));
        setOpen(rows.length > 0);
        inputRef.current?.focus();
      })
      .catch((e) => report(`Не удалось получить перечень для поля «${label}»`, e));
  }

  useEffect(() => {
    // Флаг снимается сразу, на любом пути: иначе после стирания поля до
    // пустого он оставался взведённым и следующая программная подстановка
    // открывала список (ревьюер 18.09.2026).
    const byKeyboard = typed.current;
    typed.current = false;
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
    // Не с клавиатуры — список не трогаем: ни открывать, ни закрывать.
    // Закрывать нельзя: у восприемника пол приходит после разбора имени
    // и перезапускает эффект — открытый по набору список пропадал бы.
    if (!byKeyboard) return;
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

  const inputRef = useRef<HTMLInputElement | null>(null);

  /**
   * Выбор строки — и подстановка, и переход к следующему полю одним нажатием.
   *
   * Заказчик 13.09.2026: «у восприемника после ввода НП почему-то не
   * перескакивает на следующее поле». На стенде переход работал — но вторым
   * Enter: первый выбирал, второй переходил. В Excel это одно действие,
   * и лишнее нажатие на каждом поле с подсказкой он ощущал как поломку.
   */
  function pick(item: Item) {
    justPicked.current = true;
    closeList();
    onChange(item.value);
    if (inputRef.current) focusNextField(inputRef.current);
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    const listOpen = open && items.length > 0;

    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (listOpen) setActive((i) => (i + 1) % items.length);
      else if (browse && e.altKey) browseAll();
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
      if (listOpen) {
        // Ctrl+Enter при открытом списке: только подставить, не сохранять.
        // Иначе сохранение (обработчик выше по дереву) уйдёт с недобранным
        // «Бух» вместо выбранного «Бухарино» — состояние ещё не применилось.
        // Проверяющий 13.09.2026 воспроизвёл это на стенде.
        if (e.ctrlKey || e.metaKey) e.stopPropagation();
        pick(items[active]);
      } else focusNextField(e.currentTarget, e.shiftKey ? -1 : 1); // Shift+Enter — назад
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
        ref={(el) => {
          inputRef.current = el;
          if (typeof ref === "function") ref(el);
          else if (ref) ref.current = el;
        }}
        data-field
        value={value}
        placeholder={placeholder}
        autoComplete="off"
        spellCheck={false}
        onChange={(e) => {
          typed.current = true;
          onChange(e.target.value);
        }}
        onKeyDown={onKeyDown}
        onBlur={() => closeList()}
      />
      {browse && (
        <button
          type="button"
          className="browse"
          tabIndex={-1}
          title="Показать весь перечень (Alt+↓)"
          aria-label="Показать весь перечень"
          onMouseDown={(e) => {
            e.preventDefault(); // не отдавать фокус кнопке — иначе onBlur закроет список
            if (open) closeList();
            else browseAll();
          }}
        >
          ▾
        </button>
      )}
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
