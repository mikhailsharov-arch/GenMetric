import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { focusNextField, focusNextEmptyField } from "./focus";
import type { Item } from "./Suggest";
import { report } from "./errors";
import NameResolve from "./NameResolve";

export type Parsed = {
  first_name: string | null;
  first_name_modern: string | null;
  patronymic: string | null;
  patronymic_modern: string | null;
  surname: string | null;
  gender: string | null;
  father_name: string | null;
  known_name: boolean;
  /** Имя опознано по соответствию человека («Пискарь» → «Кесарь»). */
  name_alias: string | null;
  patr_alias: string | null;
  /** Второе слово похоже на отчество, но словарю неизвестно. */
  patr_unknown: string | null;
};

/** Пометка в примечание после сверки: как было написано в документе. */
export function docNote(kind: "name" | "patr", word: string): string {
  return `${kind === "patr" ? "Отчество" : "Имя"} в документе: ${word}`;
}

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
  /**
   * Сверка со справочником решена: в поле — имя из словаря, в примечание —
   * «Имя в документе: …». Родитель дописывает примечание персоне (или записи
   * у ребёнка). Без обработчика поле просто меняет текст.
   */
  onResolved?: (iof: string, note: string) => void;
};

export default function IofField({
  label, value, onChange, onPickPerson, placeholder, inputRef, gender, onResolved,
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
  //
  // Ответ на прежнее значение обязан пропадать. Он несёт с собой то самое
  // прежнее значение и отдаёт его родителю через onChange — при быстром
  // наборе «Мария» ответ на «Мари» приходил после последней буквы и
  // стирал её. Поймано сквозной проверкой на Windows 18.09.2026 (сборка #27):
  // на снимке в поле осталось «Мари». Тот же приём, что у подсказок: счётчик.
  const parseSeq = useRef(0);
  useEffect(() => {
    const mine = ++parseSeq.current;
    // Флаг набора читается здесь, до эффекта подсказок, который его снимает.
    const byKeyboard = typed.current;
    invoke<Parsed>("parse_iof", { text: value })
      .then((result) => {
        if (mine !== parseSeq.current) return; // поле уже изменилось
        parsedRef.current = result;
        setParsed(result);
        onChangeRef.current?.(value, result);
        // Заполнено не с клавиатуры — жена от мужа, персона из архива,
        // запись на правку: из поля никто не выйдет, и сверка при уходе не
        // сработает (Роман 24.09.2026: «у матери не отрабатывает с
        // отчеством», «у восприемников не отрабатывает имя»). Сверяем сразу.
        if (!byKeyboard && value.trim()) decide(result, value.trim());
      })
      .catch((e) => {
        parsedRef.current = null;
        setParsed(null);
        report("Не удалось разобрать имя, отчество и фамилию", e);
      });
  }, [value]);

  /**
   * Закрывает список и отменяет всё, что уже улетело за подсказками.
   *
   * Отмена здесь — главное. Запрос подсказок уходит на каждое нажатие, и когда
   * человек выбирает строку, предыдущий запрос ещё в пути. Вернувшись, он
   * открывал список заново — тот самый «при его выборе списки не прячутся»,
   * который заказчик написал дважды, 24 и 27 августа. В первый раз я починил
   * зависимости эффекта, а не это, и поломка осталась.
   *
   * Увеличенный счётчик делает ответ на прежний запрос неактуальным: сравнение
   * mine !== seq.current в обработчике ответа отбросит его.
   */
  function closeSuggestions() {
    seq.current += 1;
    setOpen(false);
    setPersons([]);
    setWords([]);
  }

  // Только набор с клавиатуры открывает список: программная подстановка
  // (жена по мужу, причт из списка) — нет. Заказчик 15.09.2026, см. Suggest.tsx.
  const typed = useRef(false);

  // Подсказки: персоны по всей строке и слова по текущему слову.
  useEffect(() => {
    // Флаг снимается сразу, на любом пути — см. Suggest.tsx (ревьюер 18.09.2026).
    const byKeyboard = typed.current;
    typed.current = false;
    if (justPicked.current) {
      justPicked.current = false;
      closeSuggestions();
      return;
    }
    const query = value.trim();
    if (query.length < 1) {
      closeSuggestions();
      return;
    }
    // Не с клавиатуры — список не трогаем: ни открывать, ни закрывать.
    // Закрывать нельзя: у восприемника пол приходит после разбора имени
    // и перезапускает эффект — открытый по набору список пропадал бы.
    if (!byKeyboard) return;
    const mine = ++seq.current;

    Promise.all([
      wantPersons
        ? invoke<PersonHint[]>("suggest_person", {
            prefix: query, limit: 6,
            gender: gender ?? parsedRef.current?.gender ?? null,
          })
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
        if (mine !== seq.current) return;
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

  const inputEl = useRef<HTMLInputElement | null>(null);

  // --- Сверка со справочником при уходе из поля (заказчик 23.09.2026) ---
  //
  // Не при наборе: пока слово не дописано, оно почти всегда «неизвестно».
  // И не при сохранении: Ctrl+Enter упёрся бы в окно посреди потока. Уход
  // из поля — Enter, Tab, стрелка, клик мимо — момент, когда слово готово.
  const [resolve, setResolve] = useState<{ word: string; kind: "name" | "patr" } | null>(null);
  const valueRef = useRef(value);
  valueRef.current = value;
  // После «Исправить набор» (Esc) окно не открывается снова, пока текст
  // не изменится: иначе из поля не выйти.
  const skipCheck = useRef(false);
  useEffect(() => { skipCheck.current = false; }, [value]);
  const onResolvedRef = useRef(onResolved);
  onResolvedRef.current = onResolved;

  /** Одно окно на всё приложение: второе поверх первого залипает без
   *  клавиатуры (проверяющий 23.09.2026 — окно имени забирало фокус, поле
   *  НП под ним получало blur и открывало карточку). */
  function modalOpen(): boolean {
    return document.querySelector(".modal") !== null;
  }

  /** Подставить слова из словаря и дописать пометки в примечание.
   *  Замены — списком: имя и отчество могут прийти в один уход из поля. */
  function applyWords(changes: { index: number; word: string; kind: "name" | "patr" }[]) {
    const toks = valueRef.current.trim().split(/\s+/);
    const notes: string[] = [];
    for (const c of changes) {
      notes.push(docNote(c.kind, toks[c.index]));
      toks[c.index] = c.word;
    }
    const text = toks.join(" ");
    justPicked.current = true;
    if (onResolvedRef.current) onResolvedRef.current(text, notes.join("; "));
    else onChangeRef.current(text, null);
  }

  async function checkOnLeave(related: EventTarget | null = document.body) {
    // Потеря фокуса окном программы (клик в просмотрщик скана) — не уход
    // из поля: человек вернётся и допишет слово (проверяющий 23.09.2026).
    // Признак — фокус ушёл «в никуда» и окно не активно; по одному
    // hasFocus() нельзя: на раннере e2e окно может быть не в фокусе,
    // а Tab всё равно ведёт в следующее поле (ревьюер 23.09.2026).
    if (related === null && !document.hasFocus()) return;
    if (skipCheck.current || modalOpen()) return;
    const text = valueRef.current.trim();
    if (!text) return;
    let p: Parsed;
    try {
      p = await invoke<Parsed>("parse_iof", { text });
    } catch {
      return; // ошибка разбора уже показана эффектом выше
    }
    if (valueRef.current.trim() !== text) return; // пока ждали, набрали другое
    decide(p, text);
  }

  /** Что делать с разобранным: алиас — подставить, неизвестное — окно. */
  function decide(p: Parsed, text: string) {
    if (modalOpen() || valueRef.current.trim() !== text) return;
    const toks = text.split(/\s+/);
    if (!p.known_name) {
      setResolve({ word: toks[0], kind: "name" });
      return;
    }
    const changes: { index: number; word: string; kind: "name" | "patr" }[] = [];
    if (p.name_alias) changes.push({ index: 0, word: p.name_alias, kind: "name" });
    if (p.patr_alias) changes.push({ index: 1, word: p.patr_alias, kind: "patr" });
    if (changes.length) applyWords(changes);
    if (p.patr_unknown) {
      // Имя уже подставлено (если было чем), отчество — следующим окном:
      // «Такой же принцип и с отчеством» (Роман 23.09.2026).
      setResolve({ word: toks[1], kind: "patr" });
    }
  }

  function afterResolve() {
    setResolve(null);
    const el = inputEl.current;
    // Фокус — в следующее поле, и ещё одна сверка того же значения: после
    // имени могло остаться несверенное отчество. Оба — после перерисовки.
    if (el) setTimeout(() => { focusNextField(el); void checkOnLeave(); }, 0);
  }

  async function resolvePick(target: string) {
    if (!resolve) return;
    try {
      await invoke("alias_save", { kind: resolve.kind, form: resolve.word, target, gender: null });
    } catch (e) {
      report(`Не удалось запомнить соответствие «${resolve.word}» → «${target}»`, e);
      return;
    }
    applyWords([{ index: resolve.kind === "patr" ? 1 : 0, word: target, kind: resolve.kind }]);
    afterResolve();
  }

  /** Разобрать то же значение заново — после записи соответствия текст не
   *  менялся, и эффект разбора сам не сработает. */
  function reparseAfterAlias() {
    const mine = ++parseSeq.current;
    invoke<Parsed>("parse_iof", { text: valueRef.current })
      .then((result) => {
        if (mine !== parseSeq.current) return;
        parsedRef.current = result;
        setParsed(result);
        onChangeRef.current?.(valueRef.current, result);
      })
      .catch((e) => report("Не удалось разобрать имя, отчество и фамилию", e));
  }

  /** «Это не отчество»: запомнить, поле не меняется, фокус дальше. */
  async function resolveNotPatr() {
    if (!resolve) return;
    try {
      await invoke("alias_save", { kind: "patr", form: resolve.word, target: null, gender: null });
    } catch (e) {
      report(`Не удалось запомнить «${resolve.word}» как не отчество`, e);
      return;
    }
    reparseAfterAlias();
    setResolve(null);
    const el = inputEl.current;
    if (el) setTimeout(() => focusNextField(el), 0);
  }

  function resolveCancel() {
    skipCheck.current = true;
    setResolve(null);
    const el = inputEl.current;
    // Курсор в конец, не выделение: первая же буква иначе стирала бы всё.
    if (el) setTimeout(() => { el.focus(); el.setSelectionRange(el.value.length, el.value.length); }, 0);
  }

  /**
   * Выбор целой персоны заполняет ИОФ, НП и звание разом — значит и фокус
   * должен уйти дальше сразу, без второго Enter (заказчик 13.09.2026).
   * Пословная подсказка (pickWord) фокус не трогает: после имени набирается
   * отчество в том же поле.
   */
  function pickPerson(hint: PersonHint) {
    justPicked.current = true;
    closeSuggestions();
    onChange(hint.iof, parsed);
    onPickPersonRef.current?.(hint);
    // Поля заполнятся после того, как React применит состояние, — поэтому
    // к первому пустому идём следующим тиком, а не сразу.
    const el = inputEl.current;
    if (el) setTimeout(() => focusNextEmptyField(el), 0);
  }

  function pickWord(item: Item) {
    justPicked.current = true;
    closeSuggestions();
    const head = tokens.slice(0, wordIndex);
    // Пробел сразу после подстановки: следующее слово набирается без пауз.
    onChange([...head, item.value].join(" ") + " ", parsed);
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
      if (listOpen) {
        // Ctrl+Enter при открытом списке — только подставить, см. Suggest.tsx.
        if (e.ctrlKey || e.metaKey) e.stopPropagation();
        pickActive();
      } else focusNextField(e.currentTarget, e.shiftKey ? -1 : 1); // Shift+Enter — назад
    } else if (e.key === "Escape") {
      closeSuggestions();
    }
  }

  // Современное написание — целиком, «Василий Васильевич Промтов», а не
  // одно изменившееся отчество. Заказчик 22.09.2026. Показывается, только
  // если хоть что-то отличается от набранного.
  const differs = parsed && (
    (parsed.first_name_modern && parsed.first_name_modern !== parsed.first_name) ||
    (parsed.patronymic_modern && parsed.patronymic_modern !== parsed.patronymic));
  const modern = differs
    ? [parsed.first_name_modern ?? parsed.first_name,
       parsed.patronymic_modern ?? parsed.patronymic,
       parsed.surname].filter(Boolean).join(" ")
    : "";

  return (
    <div className="field">
      <label>{label}</label>
      <div className="fieldbody">
        <input
          ref={(el) => {
            inputEl.current = el;
            if (inputRef) (inputRef as React.MutableRefObject<HTMLInputElement | null>).current = el;
          }}
          data-field
          value={value}
          placeholder={placeholder ?? "имя, отчество, фамилия"}
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => {
            typed.current = true;
            onChange(e.target.value, parsed);
          }}
          onKeyDown={onKeyDown}
          onBlur={(e) => {
            closeSuggestions();
            void checkOnLeave(e.relatedTarget);
          }}
        />
        {resolve && (
          <NameResolve
            word={resolve.word}
            kind={resolve.kind}
            gender={gender}
            onPick={(v) => void resolvePick(v)}
            onCancel={resolveCancel}
            onNotPatr={() => void resolveNotPatr()}
          />
        )}
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
