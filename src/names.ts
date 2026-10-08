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
