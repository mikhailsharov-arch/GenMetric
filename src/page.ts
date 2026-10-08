/**
 * Номер страницы метрической книги и шаг «+ / −» по нему.
 *
 * Заказчик 21.09.2026: «в метриках номера часто имеют вид „938об-939“, что
 * означает разворот, на котором 938об это оборот листа, а 939 это следующий
 * лист… при прожатии + или − значение должно меняться на 939об-940 при плюсе
 * и 937об-938 при минусе».
 *
 * Правило: шаг прибавляет единицу к каждому числу в строке, всё остальное
 * (буквы «об», дефис, пробелы) остаётся как написано. Так работают и «938»,
 * и «938об», и «938об-939», и «938-939об». Строка без чисел не меняется.
 * Ниже нуля не уходим. Чистая функция — проверяется scripts/test_page.mjs.
 */
export function stepPage(value: string | null, delta: number): string {
  const text = (value ?? "").trim();
  if (text === "") return delta > 0 ? "1" : "";
  if (!/\d/.test(text)) return text;
  return text.replace(/\d+/g, (digits) => String(Math.max(0, Number(digits) + delta)));
}

/** Строка пуста — страницы нет. Пробелы по краям не хранятся. */
export function normalizePage(value: string): string | null {
  const text = value.trim();
  return text === "" ? null : text;
}

/**
 * Предохранитель «забытая страница» (Роман 07.10.2026): «При потоковом вводе
 * часто увлекаешься и забываешь переключить лист, из-за чего потом приходится
 * массово исправлять нумерацию страниц».
 *
 * `pages` — страницы уже сохранённых записей раздела за год книги, `current`
 * — страница на форме. Возвращает, сколько записей на ней уже есть и сколько
 * их обычно, если набираемая запись стала бы больше 150 % среднего; иначе
 * null. Среднее — по остальным страницам; меньше пяти страниц — молчим:
 * судить не по чему. На его данных (в среднем 3,8 записи на разворот, самое
 * большое 7) порог зря сработал бы на 4 разворотах из 289.
 */
export const PAGE_GUARD_MIN_PAGES = 5;
export function pageOverflow(pages: (string | null)[], current: string | null):
    { onPage: number; usual: number } | null {
  const here = (current ?? "").trim();
  if (here === "") return null;
  const perPage = new Map<string, number>();
  for (const p of pages) {
    const key = (p ?? "").trim();
    if (key !== "") perPage.set(key, (perPage.get(key) ?? 0) + 1);
  }
  const onPage = perPage.get(here) ?? 0;
  perPage.delete(here);
  if (perPage.size < PAGE_GUARD_MIN_PAGES) return null;
  let total = 0;
  for (const n of perPage.values()) total += n;
  const usual = total / perPage.size;
  return onPage + 1 > usual * 1.5 ? { onPage, usual } : null;
}
