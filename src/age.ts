/**
 * Возраст умершего — как написан в книге, и разобранный на части.
 *
 * Лист «3» Excel Романа (824 записи): в колонке «Лет» числом — 404 («5» —
 * это годы), «3 мес» — 328, «2 нед» — 65, «1,5 мес» — 16, «5 дней» — 8,
 * «1 дня» — 1. В шаблоне Familio — отдельные «Лет / Месяцев / Недель / Дней».
 *
 * Поэтому поле одно, набирается как в книге, а в базу уходят и текст
 * (age_text — выгрузка в Excel повторит его один в один), и числа по
 * колонкам Familio. Дробь («1,5 мес») — целая часть и остаток днями
 * (0,5 мес = 15 дней). Непонятный текст не ошибка: остаётся текстом.
 *
 * Без зависимостей нарочно: проверяется scripts/test_age.mjs напрямую.
 */

export type Age = {
  years: number | null;
  months: number | null;
  weeks: number | null;
  days: number | null;
};

const EMPTY: Age = { years: null, months: null, weeks: null, days: null };

/** null — текст не разобран (или пуст); иначе заполнена одна-две части. */
export function parseAge(text: string): Age | null {
  // Старая орфография («9 лѣтъ», «3 мѣс.») — к современной.
  const t = text.trim().toLowerCase().replace(/ё/g, "е").replace(/ѣ/g, "е").replace(/ъ(?=\s|\.|$)/g, "");
  if (!t) return null;
  const m = /^(\d{1,3})(?:[.,](\d{1,2}))?\s*(лет|л|года?|г|мес(?:яц(?:а|ев)?)?|м|нед(?:ел[ьяи]|ель)?|н|дн(?:ей|я)?|день|д)?\.?$/.exec(t);
  if (!m) return null;
  const whole = Number(m[1]);
  const frac = m[2] ? Number(`0.${m[2]}`) : 0;
  const unit = m[3] ?? "";
  if (unit === "" || unit.startsWith("л") || unit.startsWith("г")) {
    if (frac) return { ...EMPTY, years: whole, months: Math.round(frac * 12) };
    return { ...EMPTY, years: whole };
  }
  if (unit.startsWith("м")) {
    if (frac) return { ...EMPTY, months: whole, days: Math.round(frac * 30) };
    return { ...EMPTY, months: whole };
  }
  if (unit.startsWith("н")) {
    if (frac) return { ...EMPTY, weeks: whole, days: Math.round(frac * 7) };
    return { ...EMPTY, weeks: whole };
  }
  if (frac) return null; // «1,5 дня» — такого в книгах нет, оставим текстом
  return { ...EMPTY, days: whole };
}
