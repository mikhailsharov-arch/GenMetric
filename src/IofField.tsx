import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { focusNextField } from "./focus";
import type { Item } from "./Suggest";
import { report } from "./errors";

export type Parsed = {
  first_name: string | null;
  first_name_modern: string | null;
  patronymic: string | null;
  patronymic_modern: string | null;
  surname: string | null;
  gender: string | null;
  father_name: string | null;
  known_name: boolean;
};

export type PersonHint = {
  iof: string;
  place: string | null;
  rank: string | null;
  gender: string | null;
  uses: number;
};

/**
 * Поле ИОФ.
 *
 * Показывает два вида подсказок сразу, и порядок здесь принципиален.
 *
 * СВЕРХУ — персоны, уже занесённые в базу, вместе с населённым пунктом
 * и званием. Заказчик 17.08.2026: «я хочу чтобы во всех полях ИОФ индексатор
 * предугадывал уже занесённого в базу человека, а не отдельно имя, отчество
 * и фамилию». Выбор такой строки заполняет три поля разом, а для отца ещё
 * и данные жены. На его работе по одному приходу 36% вводимых строк ИОФ
 * уже встречались раньше.
 *
 * НИЖЕ — пословные подсказки по словарю: имя, потом отчество, потом фамилия.
 * Они нужны для людей, которых в базе ещё нет, а таких большинство при первом
 * проходе по приходу.
 *
 * Разбор набранного на части идёт всегда: и для новых, и для выбранных.
 */

type Props = {
  label: string;
  value: string;
  onChange: (text: string, parsed: Parsed | null) => void;
  onPickPerson?: (hint: PersonHint) => void;
  placeholder?: string;
  inputRef?: React.RefObject<HTMLInputElement>;
  /** Пол персоны, известный из её роли: отец всегда М, мать всегда Ж.
   *  Нужен, чтобы не предлагать мужчине женское отчество. Для ребёнка
   *  и восприемников роль пола не задаёт — тогда берём из разбора имени. */
  gender?: "М" | "Ж";
};

export default function IofField({
  label, value, onChange, onPickPerson, placeholder, inputRef, gender,
}: Props) {
  const [parsed, setParsed] = useState<Parsed | null>(null);
  const parsedRef = useRef<Parsed | null>(null);
  const [words, setWords] = useState<Item[]>([]);
  const [persons, setPersons] = useState<PersonHint[]>([]);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const seq = useRef(0);
  const justPicked = useRef(false);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  // onPickPerson приходит из формы новой функцией на каждую перерисовку.
  // Пока он стоял в зависимостях эффекта ниже, эффект срабатывал второй раз
  // уже после того, как флаг justPicked был израсходован, и список подсказок
  // открывался снова сразу после выбора. Заказчик 24.08.2026: «при его выборе
  // списки не прячутся, а должны». Держим в ссылке, а не в зависимостях.
  const onPickPersonRef = useRef(onPickPerson);
  onPickPersonRef.current = onPickPerson;
  const wantPersons = Boolean(onPickPerson);

  const tokens = value.split(/\s+/);
  const wordIndex = tokens.length - 1;
  const currentWord = tokens[wordIndex] ?? "";
  const kind = wordIndex === 0 ? "first_name" : wordIndex === 1 ? "patronymic" : "surname";
  const KIND_TITLE = ["имя", "отчество", "фамилия"][Math.min(wordIndex, 2)];

  // Разбор на имя, отчество и фамилию — на каждое изменение.
  useEffect(() => {
    invoke<Parsed>("parse_iof", { text: value })
      .then((result) => {
        parsedRef.current = result;
        setParsed(result);
        onChangeRef.current?.(value, result);
      })
      .catch((e) => {
        parsedRef.current = null;
        setParsed(null);
        report("Не удалось разобрать имя, отчество и фамилию", e);
      });
  }, [value]);

  // Подсказки: персоны по всей строке и слова по текущему слову.
  useEffect(() => {
    if (justPicked.current) {
      justPicked.current = false;
      setWords([]);
      setPersons([]);
      setOpen(false);
      return;
    }
    const query = value.trim();
    if (query.length < 1) {
      setWords([]);
      setPersons([]);
      setOpen(false);
      return;
    }
    const mine = ++seq.current;

    Promise.all([
      wantPersons
        ? invoke<PersonHint[]>("suggest_person", { prefix: query, limit: 6 })
        : Promise.resolve([] as PersonHint[]),
      currentWord.length > 0
        ? invoke<Item[]>("suggest", {
            kind, prefix: currentWord, limit: 6,
            // Пол роли важнее пола, угаданного по имени: «Никита» словарь
            // знает и как мужское, и как основу женских вариантов.
            gender: gender ?? parsedRef.current?.gender ?? null,
          })
        : Promise.resolve([] as Item[]),
    ])
      .then(([foundPersons, foundWords]) => {
        if (mine !== seq.current) return;
        setPersons(foundPersons);
        setWords(foundWords);
        setActive(0);
        setOpen(foundPersons.length + foundWords.length > 0);
      })
      .catch((e) => {
        setPersons([]);
        setWords([]);
        setOpen(false);
        report("Не удалось получить подсказки к ИОФ", e);
      });
    // Пола в зависимостях намеренно нет. Разбор имени приходит асинхронно,
    // и если бы его результат перезапускал этот эффект, список открывался бы
    // заново уже после того, как justPicked израсходован — ровно та поломка,
    // которую здесь и чиним. Поэтому пол читается по месту, из ссылки.
  }, [value, kind, currentWord, wantPersons, gender]);

  const total = persons.length + words.length;

  function pickPerson(hint: PersonHint) {
    justPicked.current = true;
    setOpen(false);
    setPersons([]);
    setWords([]);
    onChange(hint.iof, parsed);
    onPickPersonRef.current?.(hint);
  }

  function pickWord(item: Item) {
    justPicked.current = true;
    const head = tokens.slice(0, wordIndex);
    // Пробел сразу после подстановки: следующее слово набирается без пауз.
    onChange([...head, item.value].join(" ") + " ", parsed);
    setOpen(false);
    setPersons([]);
    setWords([]);
  }

  function pickActive() {
    if (active < persons.length) pickPerson(persons[active]);
    else pickWord(words[active - persons.length]);
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    const listOpen = open && total > 0;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (listOpen) setActive((i) => (i + 1) % total);
      else focusNextField(e.currentTarget);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      if (listOpen) setActive((i) => (i - 1 + total) % total);
      else focusNextField(e.currentTarget, -1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (listOpen) pickActive();
      else focusNextField(e.currentTarget);
    } else if (e.key === "Escape") {
      setOpen(false);
    }
  }

  const modern = [
    parsed?.first_name_modern !== parsed?.first_name ? parsed?.first_name_modern : null,
    parsed?.patronymic_modern !== parsed?.patronymic ? parsed?.patronymic_modern : null,
  ].filter(Boolean).join(" ");

  return (
    <div className="field">
      <label>{label}</label>
      <div className="fieldbody">
        <input
          ref={inputRef}
          data-field
          value={value}
          placeholder={placeholder ?? "имя, отчество, фамилия"}
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => onChange(e.target.value, parsed)}
          onKeyDown={onKeyDown}
          onBlur={() => setOpen(false)}
        />
        {/* Что программа поняла: современное написание и пол. Строка появляется
            только когда есть что сказать, чтобы не занимать высоту зря. */}
        {(modern || parsed?.gender) && (
          <div className="parsedline">
            {modern && <span className="modern">{modern}</span>}
            {parsed?.gender && <span className="tag">{parsed.gender}</span>}
            {parsed?.father_name && <span className="tag">отец: {parsed.father_name}</span>}
            {value.trim() && !parsed?.known_name && (
              <span className="tag warn">имени нет в словаре</span>
            )}
          </div>
        )}
        {open && total > 0 && (
          <ul className="suggest">
            {persons.map((p, i) => (
              <li
                key={`p${i}`}
                className={i === active ? "active person" : "person"}
                onMouseDown={(e) => {
                  e.preventDefault();
                  pickPerson(p);
                }}
              >
                <span className="val">
                  {p.iof}
                  {(p.place || p.rank) && (
                    <span className="sub">
                      {[p.rank, p.place].filter(Boolean).join(", ")}
                    </span>
                  )}
                </span>
                <span className="tier t1">персона</span>
              </li>
            ))}
            {words.map((w, i) => (
              <li
                key={`w${i}`}
                className={persons.length + i === active ? "active" : ""}
                onMouseDown={(e) => {
                  e.preventDefault();
                  pickWord(w);
                }}
              >
                <span className="val">{w.value}</span>
                <span className={`tier t${w.tier}`}>{KIND_TITLE}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
