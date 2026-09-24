import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Modal from "./Modal";
import { report } from "./errors";

/**
 * Окно сверки имени (или отчества) со справочником.
 *
 * Проект заказчика целиком, 23.09.2026: «должно открыться окно, в котором
 * будет написано „Какому имени из базы соответствует имя Пискарь“ и
 * выпадающий список с именами, где на первых местах будут имена, похожие
 * на Пискарь, например Кесарь. Пользователь выбирает имя и нажимает кнопку
 * „Запомнить“ в этом окне. Имя в поле „ИОФ“ меняется на Кесарь, а в
 * примечании автоматически появляется надпись: „Имя в документе: Пискарь“.
 * Такой же принцип и с отчеством».
 *
 * Пока поле поиска пусто — похожие (расстояние Дамерау — Левенштейна,
 * similar_names). Набор в поле — обычный поиск по началу слова, по полу.
 * Кнопки «Новое имя» нет намеренно — Роман 24.09.2026: «у пользователя не
 * должно быть возможности добавить в справочник новое имя, имена должны
 * быть универсализированы под каноничное написание». Esc — назад в поле,
 * поправить набор.
 */
type Props = {
  word: string;
  kind: "name" | "patr";
  gender?: "М" | "Ж";
  onPick: (value: string) => void;
  onCancel: () => void;
  /** «Это не отчество» — запомнить и больше не спрашивать. */
  onNotPatr?: () => void;
};

type Row = { value: string; gender: string | null };

export default function NameResolve({ word, kind, gender, onPick, onCancel, onNotPatr }: Props) {
  const [query, setQuery] = useState("");
  const [similar, setSimilar] = useState<Row[]>([]);
  const [found, setFound] = useState<Row[]>([]);
  const [active, setActive] = useState(0);
  const seq = useRef(0);
  const what = kind === "patr" ? "отчеству" : "имени";

  useEffect(() => {
    invoke<Row[]>("similar_names", { text: word, kind, gender: gender ?? null, limit: 12 })
      .then((rows) => { setSimilar(rows); setActive(0); })
      .catch((e) => report("Не удалось найти похожие имена", e));
  }, [word, kind, gender]);

  useEffect(() => {
    const q = query.trim();
    if (!q) { setFound([]); setActive(0); return; }
    const mine = ++seq.current;
    // Только словарь: обычная подсказка (suggest) мешает словарь с набранным
    // и с архивом Excel, а цель соответствия обязана быть словарным именем.
    invoke<Row[]>("dict_search", { prefix: q, kind, gender: gender ?? null, limit: 12 })
      .then((rows) => {
        if (mine !== seq.current) return;
        setFound(rows);
        setActive(0);
      })
      .catch((e) => report("Не удалось найти имена", e));
  }, [query, kind, gender]);

  const list = query.trim() ? found : similar;

  function pick() {
    if (list[active]) onPick(list[active].value);
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (list.length) setActive((i) => (i + 1) % list.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      if (list.length) setActive((i) => (i - 1 + list.length) % list.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      // Ctrl+Enter — не выбор: он подставлял первое похожее, а следующий
      // Ctrl+Enter сохранял запись с ним (Роман 24.09.2026).
      if (!e.ctrlKey && !e.metaKey) pick();
    }
  }

  return (
    <Modal title={`Какому ${what} из справочника соответствует «${word}»?`} kind={`resolve-${kind}`} onClose={onCancel}>
      <p className="hint">
        {kind === "patr"
          ? "Отчества с таким написанием в справочнике нет. Выберите, какое имелось в виду, — в записи останется пометка, как было в документе."
          : "Имени с таким написанием в справочнике нет. Выберите, какое имелось в виду, — в записи останется пометка «Имя в документе», и в следующий раз программа подставит его сама."}
      </p>
      <div className="field">
        <label>Поиск</label>
        <div className="fieldbody">
          <input
            data-field
            value={query}
            placeholder={list.length ? "начните набирать, чтобы искать по началу" : "похожих нет — наберите имя"}
            autoComplete="off"
            spellCheck={false}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKeyDown}
          />
        </div>
      </div>
      <ul className="suggest static resolve">
        {list.length === 0 && <li className="empty">ничего не найдено</li>}
        {list.map((r, i) => (
          <li
            key={r.value}
            ref={i === active ? (el) => el?.scrollIntoView({ block: "nearest" }) : undefined}
            className={i === active ? "active" : ""}
            onMouseDown={(e) => { e.preventDefault(); setActive(i); onPick(r.value); }}
          >
            <span className="val">{r.value}</span>
            {r.gender && <span className="tier">{r.gender}</span>}
            {!query.trim() && <span className="tier t4">похоже</span>}
          </li>
        ))}
      </ul>
      <div className="modalbar">
        <button type="button" className="primary" disabled={!list.length} onClick={pick}>
          Запомнить
        </button>
        {kind === "patr" && (
          <button type="button" className="toggle" onClick={onNotPatr ?? onCancel}>
            Это не отчество
          </button>
        )}
        <button type="button" className="toggle" onClick={onCancel}>Исправить набор (Esc)</button>
      </div>
    </Modal>
  );
}
