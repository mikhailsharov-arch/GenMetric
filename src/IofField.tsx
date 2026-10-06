import { useEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { showNoName, titleCase } from "./names";
import { focusNextField, focusNextEmptyField, scrollInList } from "./focus";
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
  /** Имя в книге не указано — первым словом стоит «***». */
  name_missing?: boolean;
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
  /** Ребёнок из записи о рождении (подсказка умершего): его родитель, место
   *  родителя и дата рождения — чтобы различать тёзок (Роман 30.09.2026). */
  infant?: { kin: string | null; parent: string | null; place: string | null;
             rank: string | null; born: string | null };
};

type InfantHint = {
  iof: string; gender: string | null; kin: string | null; parent: string | null;
  place: string | null; rank: string | null; born: string | null;
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
  /** Умерший: в подсказке первыми младенцы из записей о рождении
   *  (Роман 28.09.2026). */
  preferInfant?: boolean;
  /** Умершему предлагать детей из записей о рождении прихода отдельными
   *  строками, каждого со своим родителем (Роман 30.09.2026). По всему
   *  приходу, а не по делу года: умерший в январе родился в прошлом году. */
  infantRows?: boolean;
  /** Год на форме смертей: дети, родившиеся не позже него (и не раньше чем
   *  за 7 лет) — после импорта в приходе рождения за много лет. */
  infantYear?: number | null;
  /** НП умершего, если уже набран: только дети этой деревни (Роман 03.10.2026:
   *  19 «Евдокий» на весь приход). */
  infantPlace?: string | null;
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
  label, value, onChange, onPickPerson, placeholder, inputRef, gender, onResolved, preferInfant,
  infantRows,
  infantYear,
  infantPlace,
}: Props) {
  const [parsed, setParsed] = useState<Parsed | null>(null);
  const parsedRef = useRef<Parsed | null>(null);
  const [words, setWords] = useState<Item[]>([]);
  const [persons, setPersons] = useState<PersonHint[]>([]);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  /** Сколько в списке детей-тёзок первой строки (0 — тёзок нет). */
  const [namesakes, setNamesakes] = useState(0);
  const [namesFirstShown, setNamesFirstShown] = useState(false);
  /** НП умершего набран, детей с таким именем в нём нет, а в приходе есть:
   *  сколько их. Пустой список без объяснения читался как «не работает». */
  const [elsewhere, setElsewhere] = useState(0);
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
        // Только видимое поле: причт общий для трёх форм (27.09.2026), и
        // набор в одной форме приходит в скрытые не с клавиатуры — без этой
        // проверки они открывали окна сверки на каждую букву (стенд #36).
        if (!byKeyboard && value.trim() && inputEl.current?.offsetParent != null)
          decide(result, value.trim());
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
    setElsewhere(0);
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
    const askedGender = gender ?? parsedRef.current?.gender ?? null;

    Promise.all([
      wantPersons
        ? invoke<PersonHint[]>("suggest_person", {
            prefix: query, limit: 6,
            gender: gender ?? parsedRef.current?.gender ?? null,
            // Дети идут своими строками (infantRows) — тогда в общем списке
            // их поднимать незачем.
            preferInfant: (preferInfant ?? false) && !infantRows,
          })
        : Promise.resolve([] as PersonHint[]),
      wantPersons && infantRows
        ? invoke<InfantHint[]>("suggest_infant", {
            // 30 — чтобы при десятках тёзок за год нужный не выпал из списка
            // (список прокручивается); порядок — от недавно родившихся.
            prefix: query, limit: 30, year: infantYear ?? null,
            place: infantPlace?.trim() || null,
            gender: gender ?? parsedRef.current?.gender ?? null,
          })
        : Promise.resolve([] as InfantHint[]),
      currentWord.length > 0
        ? invoke<Item[]>("suggest", {
            kind, prefix: currentWord, limit: 6,
            // Пол роли важнее пола, угаданного по имени: «Никита» словарь
            // знает и как мужское, и как основу женских вариантов.
            gender: gender ?? parsedRef.current?.gender ?? null,
          })
        : Promise.resolve([] as Item[]),
    ])
      .then(([knownPersons, infants, foundWords]) => {
        if (mine !== seq.current) return;
        // Дети — первыми, каждый своей строкой; из общего списка персон то же
        // имя убирается: «Мария» одной строкой на всех Марий ни к чему.
        const kids: PersonHint[] = infants.map((k) => ({
          iof: k.iof, place: null, rank: null, gender: k.gender, uses: 0,
          infant: { kin: k.kin, parent: k.parent, place: k.place, rank: k.rank, born: k.born },
        }));
        // «Персона» из одного имени без места и звания — ребёнок из записей о
        // рождении в памяти персон: рядом со словарным «Николай» это лишняя
        // строка-двойник (после импорта их тысячи). Не показываем.
        const whole = knownPersons.filter((p) => /\s/.test(p.iof.trim()) || p.place || p.rank);
        const foundPersons = [...kids, ...whole.filter((p) => !kids.some((k) => k.iof === p.iof))];
        setPersons(foundPersons);
        setWords(foundWords);
        // Тёзки: первая строка — ребёнок, у которого в деле есть полный тёзка.
        // Тогда заранее не выбрана ни одна строка: привычное «имя, Enter»
        // иначе молча подставило бы отца случайной из Марий (ревьюер,
        // 01.10.2026; та же осторожность, что у birth_father с #38).
        const twins = kids.length > 1 && kids.filter((k) => k.iof === kids[0].iof).length > 1;
        setNamesakes(twins ? kids.filter((k) => k.iof === kids[0].iof).length : 0);
        // Словарные имена идут первыми — первая строка не ребёнок, выбирать её
        // заранее безопасно; дети ниже, к ним — стрелкой.
        const namesFirst = !!infantRows && !/\s/.test(value.trimStart()) && foundWords.length > 0;
        setNamesFirstShown(!!infantRows && !/\s/.test(value.trimStart()));
        setActive(twins && !namesFirst ? -1 : 0);
        setOpen(foundPersons.length + foundWords.length > 0);
        setElsewhere(0);
        // НП умершего отсеял всех детей — узнать, есть ли они в приходе
        // вообще: опечатка в НП («Букарина») иначе выглядит как «ребёнка нет».
        const placeTyped = infantPlace?.trim();
        if (wantPersons && infantRows && placeTyped && infants.length === 0) {
          invoke<InfantHint[]>("suggest_infant", {
            // Пол — тот же, с каким спрашивали список выше: разбор имени за
            // это время мог прийти, и счёт разошёлся бы со списком.
            prefix: query, limit: 30, year: infantYear ?? null, place: null, gender: askedGender,
          })
            .then((all) => {
              if (mine !== seq.current || all.length === 0) return;
              setElsewhere(all.length);
              setOpen(true);
            })
            .catch((e) => { if (mine === seq.current) report("Не удалось получить подсказки к ИОФ", e); });
        }
      })
      .catch((e) => {
        if (mine !== seq.current) return;
        setPersons([]);
        setWords([]);
        setOpen(false);
        setElsewhere(0);
        report("Не удалось получить подсказки к ИОФ", e);
      });
    // Пола в зависимостях намеренно нет. Разбор имени приходит асинхронно,
    // и если бы его результат перезапускал этот эффект, список открывался бы
    // заново уже после того, как justPicked израсходован — ровно та поломка,
    // которую здесь и чиним. Поэтому пол читается по месту, из ссылки.
  }, [value, kind, currentWord, wantPersons, gender]);

  const total = persons.length + words.length;
  // Умерший, первое слово: сначала имена словаря, под ними дети и персоны
  // (Роман 05.10.2026: «точно так же, как … при наборе родившегося»).
  // Порядок — из состояния, выставленного вместе со списком: считать его в
  // рендере по текущему тексту нельзя — после пробела строки перевернулись бы
  // под прежним выбором, и Enter подставил бы ребёнка-тёзку (ревьюер 05.10.2026).
  const wordsFirst = namesFirstShown;
  const pOff = wordsFirst ? words.length : 0;
  const wOff = wordsFirst ? 0 : persons.length;

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
    // Пока ждали, человек мог уйти на другую вкладку: окно скрытой формы
    // теперь (portal) всплыло бы поверх чужой (ревьюер #37).
    if (inputEl.current?.offsetParent == null) return;
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
    // У причта — та же сверка, что у всех (Роман 27.09.2026: «у
    // церковнослужителей надо сделать такую же проверку ИОФ»); исключение
    // #35 «два слова у причта — имя и фамилия» снято. «Это не отчество»
    // запоминает фамилию один раз.
    if (p.patr_unknown) {
      // Имя уже подставлено (если было чем), отчество — следующим окном:
      // «Такой же принцип и с отчеством» (Роман 23.09.2026).
      setResolve({ word: toks[1], kind: "patr" });
    }
  }

  function afterResolve() {
    // Окно снимается сразу, до перехода фокуса: вызов идёт после await, вне
    // события React, и без flushSync таймер мог сработать при ещё открытом
    // окне — focus.ts тогда не ведёт никуда, фокус терялся (ревьюер #37).
    flushSync(() => setResolve(null));
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
    flushSync(() => setResolve(null));
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
    // «Персона» из одного имени без места и звания — это ребёнок из записей о
    // рождении, попавший в память персон; после импорта их тысячи. Выбор такой
    // строки — выбор имени: пробел, фокус остаётся, дописываются отчество и
    // фамилия (Роман 05.10.2026: «фокус автоматически перепрыгивает»).
    if (!hint.infant && !/\s/.test(hint.iof.trim()) && !hint.place && !hint.rank) {
      onChange(hint.iof.trim() + " ", parsed);
      return;
    }
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
    if (active >= pOff && active < pOff + persons.length) pickPerson(persons[active - pOff]);
    else pickWord(words[active - wOff]);
  }

  function onKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    const listOpen = open && total > 0;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (listOpen) setActive((i) => (i + 1) % total);
      else focusNextField(e.currentTarget);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      if (listOpen) setActive((i) => (i < 0 ? total - 1 : (i - 1 + total) % total));
      else focusNextField(e.currentTarget, -1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (listOpen && active >= 0) {
        // Ctrl+Enter при открытом списке — только подставить, см. Suggest.tsx.
        if (e.ctrlKey || e.metaKey) e.stopPropagation();
        pickActive();
      } else if (listOpen) {
        // Ни одна строка не выбрана (тёзки): Enter — как без списка, дальше
        // по форме; набранное остаётся как есть. Ctrl+Enter запись при этом
        // не сохраняет — список ещё открыт (то же правило, что выше).
        if (e.ctrlKey || e.metaKey) { e.stopPropagation(); return; }
        closeSuggestions();
        focusNextField(e.currentTarget, e.shiftKey ? -1 : 1);
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

  const personRows = persons.map((p, i) => (
              <li
                key={`p${i}`}
                // Список идёт за стрелками (Роман 28.09.2026: «выбирает
                // элементы вслепую») — как у Suggest и окна сверки.
                ref={pOff + i === active ? scrollInList : undefined}
                className={pOff + i === active ? "active person" : "person"}
                onMouseDown={(e) => {
                  e.preventDefault();
                  pickPerson(p);
                }}
              >
                <span className="val">
                  {p.iof}
                  {p.infant ? (
                    <span className="sub wrap">
                      {[p.infant.parent && `${p.infant.kin ?? "родитель"} ${p.infant.parent}`,
                        p.infant.place, p.infant.born && `род. ${p.infant.born}`].filter(Boolean).join(" · ")}
                    </span>
                  ) : (p.place || p.rank) && (
                    <span className="sub">
                      {[p.rank, p.place].filter(Boolean).join(", ")}
                    </span>
                  )}
                </span>
                <span className="tier t1">{p.infant ? "младенец" : "персона"}</span>
              </li>
            ));
  const wordRows = words.map((w, i) => (
              <li
                key={`w${i}`}
                ref={wOff + i === active ? scrollInList : undefined}
                className={wOff + i === active ? "active" : ""}
                onMouseDown={(e) => {
                  e.preventDefault();
                  pickWord(w);
                }}
              >
                <span className="val">{w.value}</span>
                <span className={`tier t${w.tier}`}>{KIND_TITLE}</span>
              </li>
            ));

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
          title="Имени в книге нет — поставьте *** вместо него"
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => {
            typed.current = true;
            // Заглавные буквы — сразу, при наборе (Роман 05.10.2026). Длина
            // текста не меняется, курсор возвращаем на место.
            // Правим само поле сразу и ставим курсор на место — до того, как
            // React применит состояние: отложенный возврат курсора при быстром
            // наборе вставлял бы следующую букву не туда.
            // Системный метод ввода (композиция) ещё не закончил слово —
            // править поле под ним нельзя, ввод сорвётся (ревьюер 05.10.2026).
            if ((e.nativeEvent as InputEvent).isComposing) {
              onChange(e.target.value, parsed);
              return;
            }
            const el = e.target, at = el.selectionStart;
            // Не при стирании: стёрли первую букву «иван», чтобы поправить, —
            // «ван» не должно тут же стать «Ван» (вышло бы «ИВан»; проверяющий
            // 05.10.2026). Заглавная встанет со следующей набранной буквой.
            const erasing = ((e.nativeEvent as InputEvent).inputType ?? "").startsWith("delete");
            const proper = erasing ? el.value : titleCase(el.value);
            if (proper !== el.value) {
              el.value = proper;
              if (at !== null) el.setSelectionRange(at, at);
            }
            onChange(proper, parsed);
          }}
          onKeyDown={onKeyDown}
          onBlur={(e) => {
            closeSuggestions();
            // «иван иванов» → «Иван Иванов» (Роман 03.10.2026). До сверки:
            // она читает текст из valueRef.
            // Только при настоящем уходе из поля: окно программы потеряло
            // фокус (клик в скан) — человек вернётся и допишет слово; правка
            // текста здесь запустила бы сверку недописанного (ревьюер 03.10.2026).
            // И заглушка вместо имени («—», «?») — сразу как сохранится: «***».
            const proper = showNoName(titleCase(e.currentTarget.value));
            if (proper !== e.currentTarget.value && (e.relatedTarget !== null || document.hasFocus())) {
              valueRef.current = proper;
              onChange(proper, null);
            }
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
        {(modern || parsed?.gender || parsed?.name_missing) && (
          <div className="parsedline">
            {parsed?.name_missing && <span className="tag">имя в книге не указано</span>}
            {modern && <span className="modern">{modern}</span>}
            {parsed?.gender && <span className="tag">{parsed.gender}</span>}
            {parsed?.father_name && <span className="tag">отец: {parsed.father_name}</span>}
            {value.trim() && !parsed?.known_name && (
              <span className="tag warn">имени нет в словаре</span>
            )}
          </div>
        )}
        {open && (total > 0 || elsewhere > 0) && (
          <ul className="suggest">
            {elsewhere > 0 && (
              <li className="empty" data-infant-elsewhere onMouseDown={(e) => e.preventDefault()}>
                в «{infantPlace?.trim()}» детей с таким именем нет, в приходе есть: {elsewhere}
                {elsewhere >= 30 ? " или больше" : ""} — проверьте НП умершего или сотрите его
              </li>
            )}
            {namesakes > 1 && active < 0 && (
              <li className="empty" onMouseDown={(e) => e.preventDefault()}>
                в деле {namesakes} детей с этим именем — выберите стрелкой ↓
              </li>
            )}
            {wordsFirst ? <>{wordRows}{personRows}</> : <>{personRows}{wordRows}</>}
          </ul>
        )}
      </div>
    </div>
  );
}
