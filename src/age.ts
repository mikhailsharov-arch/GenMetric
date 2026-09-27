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

const UNIT = "(лет|л|года?|г|мес(?:яц(?:а|ев)?)?|м|нед(?:ел[ьяи]|ель)?|н|дн(?:ей|я)?|день|д)";
const PART = `(\\d{1,3})(?:[.,](\\d{1,2}))?\\s*${UNIT}?\\.?`;

/**
 * Составной возраст: «1 год 3 мес», «2 г. 6 м.», «1 г, 2 нед» — каждая
 * часть с единицей, каждая единица не больше одного раза, без дробей
 * (техдолг после #36: раньше такое оставалось только текстом).
 */
function parseCompound(t: string): Age | null {
  // Части подряд с начала строки: «число единица», между ними пробел,
  // запятая или « и ». После единицы — не буква (в JS \b с кириллицей не
  // работает). Порядок — от лет к дням: «3 мес 1 год» не угадываем
  // (ревьюер #37: прежний разбор пропускал «1 ги 3 ми»).
  const re = new RegExp(`(\\d{1,3})\\s*${UNIT}(?![а-я])\\.?\\s*(?:,\\s*|и\\s+)?`, "y");
  const order: (keyof Age)[] = ["years", "months", "weeks", "days"];
  const out: Age = { ...EMPTY };
  let pos = 0;
  let lastRank = -1;
  let parts = 0;
  while (pos < t.length) {
    re.lastIndex = pos;
    const m = re.exec(t);
    if (!m || !m[2]) return null;
    const u = m[2];
    const key: keyof Age = u.startsWith("л") || u.startsWith("г") ? "years"
      : u.startsWith("м") ? "months" : u.startsWith("н") ? "weeks" : "days";
    const rank = order.indexOf(key);
    if (rank <= lastRank) return null; // повтор или обратный порядок
    lastRank = rank;
    out[key] = Number(m[1]);
    pos = re.lastIndex;
    parts++;
  }
  return parts >= 2 ? out : null;
}

/** null — текст не разобран (или пуст); иначе заполнены разобранные части. */
export function parseAge(text: string): Age | null {
  // Старая орфография («9 лѣтъ», «3 мѣс.») — к современной.
  const t = text.trim().toLowerCase().replace(/ё/g, "е").replace(/ѣ/g, "е").replace(/ъ(?=\s|\.|$)/g, "");
  if (!t) return null;
  const m = new RegExp(`^${PART}$`).exec(t);
  if (!m) return parseCompound(t);
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
