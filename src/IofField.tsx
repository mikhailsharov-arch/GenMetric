import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { focusNextField } from "./focus";
import type { Item } from "./Suggest";
import { report } from "./errors";

type Parsed = {
  first_name: string | null;
  first_name_modern: string | null;
  patronymic: string | null;
  patronymic_modern: string | null;
  surname: string | null;
  gender: string | null;
  father_name: string | null;
  known_name: boolean;
};

/**
 * Единое поле ИОФ вместо трёх отдельных.
 *
 * Подсказки даются пословно: пока набирается первое слово — по именам,
 * второе — по отчествам, третье — по фамилиям. Это принципиально: на
 * настоящих данных подсказка по строке целиком экономит 18% нажатий,
 * а пословная — 64%, потому что сочетание имени с отчеством почти всегда
 * уникально, а сами имена и отчества повторяются постоянно.
 *
 * Под полем показывается разбор: что программа считает именем, отчеством
 * и фамилией, и какой современный вариант она предлагает.
 */
type IofProps = {
  label: string;
  value?: string;
  onChange?: (text: string, parsed: Parsed | null) => void;
  placeholder?: string;
  compact?: boolean;
};

export default function IofField({ label, value, onChange, placeholder, compact }: IofProps) {
  const [own, setOwn] = useState("");
  const text = value ?? own;
  const setText = (next: string) => {
    setOwn(next);
    onChange?.(next, parsed);
  };
  const [parsed, setParsed] = useState<Parsed | null>(null);
  const [items, setItems] = useState<Item[]>([]);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const seq = useRef(0);
  const justPicked = useRef(false);
  // Ссылка, чтобы разбор не пересоздавал подписку на каждый набранный символ.
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  // какое слово сейчас набирается и по какому справочнику подсказывать
  const tokens = text.split(/\s+/);
  const wordIndex = tokens.length - 1;
  const currentWord = tokens[wordIndex] ?? "";
  const kind = wordIndex === 0 ? "first_name" : wordIndex === 1 ? "patronymic" : "surname";
  const KIND_TITLE = ["имя", "отчество", "фамилия"][Math.min(wordIndex, 2)];

  useEffect(() => {
    invoke<Parsed>("parse_iof", { text })
      .then((result) => {
        setParsed(result);
        onChangeRef.current?.(text, result);
      })
      .catch((e) => {
        setParsed(null);
        report("Не удалось разобрать имя, отчество и фамилию", e);
      });
  }, [text]);

  useEffect(() => {
    if (currentWord.length < 1) {
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
    invoke<Item[]>("suggest", { kind, prefix: currentWord, limit: 8 })
      .then((rows) => {
        if (mine !== seq.current) return;
        setItems(rows);
        setActive(0);
        setOpen(rows.length > 0);
      })
      .catch((e) => {
        setItems([]);
        setOpen(false);
        report(`Не удалось получить подсказки (${KIND_TITLE})`, e);
      });
  }, [text, kind, currentWord]);

  function pick(item: Item) {
    justPicked.current = true;
    const head = tokens.slice(0, wordIndex);
    // после подстановки сразу пробел: следующее слово можно набирать без пауз
    setText([...head, item.value].join(" ") + " ");
    setItems([]);
    setOpen(false);
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    const listOpen = open && items.length > 0;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (listOpen) setActive((i) => (i + 1) % items.length);
      else focusNextField(e.currentTarget);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      if (listOpen) setActive((i) => (i - 1 + items.length) % items.length);
      else focusNextField(e.currentTarget, -1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (listOpen) pick(items[active]);
      else focusNextField(e.currentTarget);
    } else if (e.key === "Escape") {
      setOpen(false);
    }
  }

  const hasParse =
    parsed && (parsed.first_name || parsed.patronymic || parsed.surname);

  return (
    <div className="field">
      <label>
        {label}
        <span className="ms">
          {currentWord.length > 0
            ? `подсказки: ${KIND_TITLE}`
            : wordIndex === 0
              ? "начните с имени"
              : `дальше ${KIND_TITLE}`}
        </span>
      </label>
      <input
        data-field
        value={text}
        placeholder={placeholder ?? "например, Иван Семенов Карсаков"}
        autoComplete="off"
        spellCheck={false}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={onKeyDown}
        onBlur={() => setOpen(false)}
      />
      {/* Слова, которые уже приняты. Видно, что программа ждёт следующее,
          даже когда пробел в конце строки не разглядеть. */}
      {wordIndex > 0 && (
        <div className="words">
          {tokens.slice(0, wordIndex).map((w, i) => (
            <span key={i} className="word">
              {["имя", "отчество", "фамилия"][Math.min(i, 2)]}: <b>{w}</b>
            </span>
          ))}
          <span className="word next">ждёт: {KIND_TITLE}</span>
        </div>
      )}
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
              <span className={`tier t${it.tier}`}>
                {it.tier === 4 ? "словарь" : "в базе"}
              </span>
            </li>
          ))}
        </ul>
      )}

      {hasParse && !compact && (
        <table className="parsed">
          <tbody>
            <tr>
              <td className="pk">Имя</td>
              <td>{parsed!.first_name}</td>
              <td className="modern">
                {parsed!.first_name_modern !== parsed!.first_name ? parsed!.first_name_modern : ""}
              </td>
            </tr>
            <tr>
              <td className="pk">Отчество</td>
              <td>{parsed!.patronymic ?? "—"}</td>
              <td className="modern">
                {parsed!.patronymic_modern !== parsed!.patronymic ? parsed!.patronymic_modern : ""}
              </td>
            </tr>
            <tr>
              <td className="pk">Фамилия</td>
              <td>{parsed!.surname ?? "—"}</td>
              <td className="modern" />
            </tr>
            <tr>
              <td className="pk">Пол</td>
              <td>{parsed!.gender ?? "—"}</td>
              <td className="modern">
                {parsed!.father_name ? `отец: ${parsed!.father_name}` : ""}
              </td>
            </tr>
          </tbody>
        </table>
      )}
    </div>
  );
}
