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
