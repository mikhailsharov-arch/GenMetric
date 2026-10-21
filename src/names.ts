/**
 * Заглавные буквы в ИОФ (Роман 03.10.2026, задача 9): «Пользователи часто
 * вводят имена строчными буквами для скорости. Программа должна сама
 * приводить их к правильному формату» — «иван иванов» → «Иван Иванов».
 *
 * Только первая буква слова; остальные не трогаются («де Толли» станет
 * «Де Толли», но «МакАртур» останется как набран). Слово — после начала
 * строки, пробела, дефиса, скобки или кавычки.
 */
export function titleCase(text: string): string {
  return text.replace(/(^|[\s\-(«"])(\p{Ll})/gu, (_m, before: string, letter: string) => before + letter.toUpperCase());
}

/** Системное слово «имя в книге не указано» — как `records::NO_NAME`. */
export const NO_NAME = "***";

/**
 * Слово-заглушка вместо имени: одни знаки («***», «—», «?») или «Имя».
 * Копия `records::is_no_name` из крейта: правишь одно — правь другое
 * (scripts/test_names.mjs и тест крейта сверяют одни и те же примеры).
 */
export function noNameWord(word: string): boolean {
  const w = word.trim();
  if (w === "") return false;
  if (!/[\p{L}\p{N}]/u.test(w)) return true;
  return NO_NAME_WORDS.includes(w.toLowerCase().replace(/\.+$/, ""));
}

/** Слова-заглушки вместо имени — как `records::NO_NAME_WORDS` (Роман
 *  06.10.2026: «общепринятые текстовые сокращения»). */
export const NO_NAME_WORDS = ["имя", "нрзб", "н/д", "неизвестно", "неизв", "нет", "б/и"];

/**
 * Фамилия отца — матери (Роман 06.10.2026): «Сидоров» → «Сидорова»,
 * «Томский» → «Томская». Фамилия без родового окончания («Шевченко»,
 * «Черных», «Сова») остаётся как есть; прилагательные не на «-ский»
 * («Палий») не трогаем — там не угадать. Дореформенный «ъ» отбрасывается.
 */
export function feminineSurname(surname: string): string {
  const s = surname.trim().replace(/ъ$/, "");
  if (/(ов|ев|ёв|ин|ын)$/i.test(s)) return s + "а";
  if (/(ск|цк)(ий|ой)$/i.test(s)) return s.slice(0, -2) + "ая";
  if (/(ый|ой)$/i.test(s)) return s.slice(0, -2) + "ая";
  return s;
}

/**
 * Звание персоны, выбранной из подсказки, — по роли (Роман 14.10.2026;
 * согласовано Mike): «крестьянский сын» из записи о браке отцом в рождениях
 * становится «крестьянином», «крестьянин» из рождений женихом в браке —
 * «крестьянским сыном». По его базе так верно примерно в 70 % случаев (те же
 * люди женихом и отцом: «крестьянский сын» → «крестьянин» 314 раз из 470).
 * Пары — только те, где сословие одно и то же; остальные звания как есть.
 * Поручитель — как отец (Роман 15.10.2026: «в поручителях не могли выступать
 * „крестьянские сыновья“ — ими записывали только взрослых полноправных
 * „крестьян“»); восприемников правило не касается.
 * Регистр, «ъ» на конце слова и «і» не различаются.
 */
const RANK_FOR_FATHER: Record<string, string> = {
  "крестьянский сын": "крестьянин",
  "умершего крестьянина сын": "крестьянин",
  "умершаго крестьянина сын": "крестьянин",
  "мещанский сын": "мещанин",
  "умершего мещанина сын": "мещанин",
  "умершаго мещанина сын": "мещанин",
};
const RANK_FOR_GROOM: Record<string, string> = {
  "крестьянин": "крестьянский сын",
  "мещанин": "мещанский сын",
};
export function rankForRole(rank: string | null | undefined, role: "father" | "groom" | "guarantor"): string | null {
  if (!rank) return rank ?? null;
  const key = rank.trim().toLowerCase().replace(/і/g, "и").replace(/ѣ/g, "е").replace(/ъ(?=\s|$)/g, "").replace(/\s+/g, " ");
  return (role === "groom" ? RANK_FOR_GROOM : RANK_FOR_FATHER)[key] ?? rank;
}

/**
 * Строка словаря фамилий отличается от набранного слова только женским
 * окончанием: набрано «Курынин», в словаре «Курынина». Мужчине такую строку
 * не показываем — она стояла в списке первой, и Enter подставлял её вместо
 * набранного (Роман 12.10.2026: «я удаляю „а“ на конце, но при переходе к
 * следующему полю программа снова возвращает „а“»). Регистр и конечный «ъ»
 * не различаются.
 */
export function onlyFeminineOf(value: string, typed: string): boolean {
  const bare = (t: string) => t.trim().toLowerCase().replace(/ъ$/, "");
  return typed.trim() !== "" && bare(value) !== bare(typed) && bare(value) === bare(feminineSurname(typed));
}

/**
 * «— Иванова Петрова» → «*** Иванова Петрова»: в записи заглушка всегда
 * хранится как «***», и поле должно показывать то, что сохранится, а не то,
 * что набрано (проверяющий 05.10.2026: замена шла молча).
 */
export function showNoName(text: string): string {
  const m = /^(\s*)(\S+)/.exec(text);
  if (!m || m[2] === NO_NAME || !noNameWord(m[2])) return text;
  return m[1] + NO_NAME + text.slice(m[0].length);
}

/** Первая буква строки — заглавная (губерния, уезд, волость в карточке
 *  пункта; Роман 07.10.2026). Остальное не трогается. */
export function capFirst(text: string): string {
  return text.replace(/^(\s*)(\p{Ll})/u, (_m, before: string, letter: string) => before + letter.toUpperCase());
}

/** Типы пункта из поставки — пока перечень `np_type` не прочитан из базы. */
export const NP_TYPES = ["д.", "с.", "г.", "погост", "посад", "б.г.", "з.г.", "завод", "починок",
                         "приселок", "с-цо", "слобода"];

/**
 * Заглавная буква в названии населённого пункта (Роман 07.10.2026: «точно
 * так же, как это уже реализовано при вводе ИОФ»).
 *
 * Исключение — название, начинающееся с типа пункта: «д.Балахонка,
 * Заобнорская волость», «починок Смыгарев» — его принятые написания (Роман
 * 08.10.2026: «делать первую букву заглавной не нужно»).
 *
 * `final = false` — идёт набор: пока набранное может оказаться типом («по» —
 * начало «починок» и «погост»), букву не трогаем; как только это уже не тип
 * («пок»), первая буква становится заглавной задним числом.
 * `final = true` — ушли из поля: тип без названия после него («слобода»,
 * «починок») — это само название, «Слобода».
 *
 * У названия после типа заглавной становится его первая буква: «починок
 * смыгарев» → «починок Смыгарев», «д.балахонка» → «д.Балахонка» — так
 * написаны его пункты.
 */
export function placeCase(text: string, types: string[], final: boolean): string {
  const lead = /^\s*/.exec(text)![0];
  const body = text.slice(lead.length);
  if (body === "" || !/^\p{Ll}/u.test(body)) return text;
  const low = body.toLowerCase();
  let maybeType = false;
  for (const raw of types) {
    const type = raw.trim().toLowerCase();
    if (type === "") continue;
    if (!final && type.startsWith(low)) maybeType = true; // «по» — ещё может стать «починок»
    if (!low.startsWith(type)) continue;
    const rest = body.slice(type.length);
    // После «д.» название идёт сразу, после «починок» — через пробел.
    const gap = /^\s*/.exec(rest)![0];
    if (!type.endsWith(".") && gap === "" && rest !== "") continue; // «починковский» — не тип
    const name = rest.slice(gap.length);
    if (name === "") {
      if (final) break; // тип без названия — это само название
      return text;
    }
    return lead + body.slice(0, type.length) + gap + name[0].toUpperCase() + name.slice(1);
  }
  if (maybeType) return text;
  return lead + body[0].toUpperCase() + body.slice(1);
}

/**
 * Поправить регистр прямо в поле во время набора — общий приём полей ИОФ, НП
 * и карточки пункта. Длина текста не меняется, курсор возвращается на место
 * сразу, до того как React применит состояние: отложенный возврат при быстром
 * наборе вставлял бы следующую букву не туда.
 *
 * Не при стирании: стёрли первую букву «иван», чтобы поправить, — «ван» не
 * должно тут же стать «Ван» (проверяющий 05.10.2026). И не во время
 * композиции системного метода ввода — ввод сорвётся (ревьюер 05.10.2026).
 */
export function typedCase(e: { target: HTMLInputElement; nativeEvent: Event },
                          fix: (text: string) => string): string {
  const el = e.target;
  const native = e.nativeEvent as InputEvent;
  if (native.isComposing || (native.inputType ?? "").startsWith("delete")) return el.value;
  const proper = fix(el.value);
  if (proper !== el.value) {
    const at = el.selectionStart;
    el.value = proper;
    if (at !== null) el.setSelectionRange(at, at);
  }
  return proper;
}

/**
 * Название пункта в программе: у деревни-тёзки с комментарием — «Хмельничное
 * (Столпино)» (Роман 06.10.2026). Копия `text::place_label` из крейта: правишь
 * одно — правь другое (примеры одни и те же в scripts/test_names.mjs и в
 * тесте крейта place_comment).
 */
export function placeLabel(name: string, comment: string): string {
  const n = name.trim(), c = comment.trim();
  return c === "" ? n : `${n} (${c})`;
}

/**
 * Название без приставки типа: «д.Керша», «с-цо Керша» → «Керша». Копия
 * `text::strip_place_type` из крейта: правишь одно — правь другое.
 */
export function stripPlaceType(name: string): string {
  const n = name.trim();
  for (const p of ["с-цо ", "С-цо ", "дер.", "пос.", "сел.", "Дер.", "Пос.", "Сел.", "с-цо", "С-цо",
                   "сл.", "Сл.", "д.", "с.", "г.", "п.", "Д.", "С.", "Г.", "П."]) {
    if (n.startsWith(p)) {
      const rest = n.slice(p.length).trimStart();
      if (rest) return rest;
    }
  }
  return n;
}

/**
 * Метка пункта для показа: название и комментарий в скобках на конце — он
 * рисуется серым. Только для экрана: настоящий комментарий лежит в карточке
 * пункта, а здесь он угадывается по скобкам — у названия со своими скобками
 * («Никольское (Старое)») серым окажется его хвост, и только.
 */
export function splitPlaceLabel(label: string): { name: string; comment: string } {
  const m = /^(.*\S) \(([^()]+)\)$/.exec(label.trim());
  return m ? { name: m[1], comment: m[2] } : { name: label, comment: "" };
}

/**
 * Обрывки со звёздочками — «***кея», «Ф***», «Яготи***» (Роман 16.10.2026):
 * «этот огрызок нельзя пускать в справочник имен и базу. Но и удалять эту
 * информацию жалко». С 20.10.2026 — в любом слове ИОФ, не только в имени
 * (Роман: «должна работать универсально для всех частей ФИО»): слово
 * становится «***», обрывок без крайних звёздочек — пометкой в примечание.
 * Какое это слово — по месту: первое — имя; из трёх и более второе —
 * отчество, последнее — фамилия; второе из двух — «отчество или фамилия».
 * Обрывков нет (нет звёздочки или букв) — null.
 */
export function maskFragment(text: string): { text: string; note: string } | null {
  const m = /^(\s*)([\s\S]*?)(\s*)$/.exec(text);
  if (!m) return null;
  const words = m[2].split(/(\s+)/);
  const tokens = words.filter((_, i) => i % 2 === 0);
  const notes: string[] = [];
  let k = 0;
  for (let i = 0; i < words.length; i += 2, k++) {
    const w = words[i];
    if (!w.includes("*") || !/\p{L}/u.test(w)) continue;
    const frag = w.replace(/^\*+|\*+$/g, "");
    const what = k === 0 ? "Имя в документе не читаемо"
      : tokens.length >= 3 && k === tokens.length - 1 ? "Фамилия в документе не читаема"
      : tokens.length >= 3 ? "Отчество в документе не читаемо"
      : "Отчество или фамилия в документе не читаемы";
    notes.push(`${what}: ${frag}`);
    words[i] = NO_NAME;
  }
  return notes.length ? { text: m[1] + words.join("") + m[3], note: notes.join("; ") } : null;
}

/** Дата в списке набранных — «01.09», двумя цифрами (Роман 20.10.2026: «1.9»
 *  в общем списке легко потерять); неизвестное — «?». */
export function dayMonth(day: number | null | undefined, month: number | null | undefined): string {
  const two = (v: number | null | undefined) => (v == null ? "?" : String(v).padStart(2, "0"));
  return `${two(day)}.${two(month)}`;
}
