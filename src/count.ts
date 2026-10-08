/**
 * Раскладка счёта родившихся по полу ребёнка.
 *
 * В книге и в Excel две колонки — «счёт родившихся мужеска пола» и «женска
 * пола», а в форме ввода поле одно, как в Excel. Раскладывать по колонкам
 * обязана программа, как это делает макрос.
 *
 * До 13.09.2026 форма всегда писала счёт в мужскую колонку. Половина записей
 * ложилась с неверным номером, на экране это не видно, выгрузка в Familio
 * ушла бы с ошибкой. Нашлось при наборе настоящих сканов, не заказчиком.
 *
 * Функция вынесена отдельно и без зависимостей нарочно: её проверяет
 * scripts/test_count.mjs напрямую, а не через интерфейс.
 */

export type Sex = "М" | "Ж";

export type CountColumns = {
  no_male: number | null;
  no_female: number | null;
};

/**
 * Возвращает колонки для сохранения или null, если пол неизвестен, а счёт
 * есть — тогда сохранять нельзя, надо спросить человека. Угадывать запрещено:
 * именно догадка и была поломкой.
 */
export function splitCount(count: number | null, sex: Sex | null): CountColumns | null {
  if (count === null) return { no_male: null, no_female: null };
  if (sex === "М") return { no_male: count, no_female: null };
  if (sex === "Ж") return { no_male: null, no_female: count };
  return null;
}

/** Обряд через Новый год (28.09.2026) — см. NextYear.tsx. */
/** Показывать ли вопрос: месяц обряда раньше месяца события. */
export function riteBeforeEvent(eventMonth: number | null, riteMonth: number | null): boolean {
  return eventMonth !== null && riteMonth !== null && riteMonth < eventMonth;
}

/**
 * Год события (рождения, смерти). «Год» на форме — год книги: у Романа он
 * один на запись (скриншот «МК Ввод» 28.09.2026), а в книгу года Y запись
 * попадает по обряду. Поэтому при «декабрь → январь» в предыдущем году —
 * событие, а обряд остаётся в году книги (ревьюер #38). Только если человек
 * отметил: угадывать нельзя.
 */
export function eventYearOf(year: number | null, eventMonth: number | null, riteMonth: number | null,
                            prev: boolean): number | null {
  if (year === null) return null;
  return prev && riteBeforeEvent(eventMonth, riteMonth) ? year - 1 : year;
}

/** Запись списка «Набрано», сколько от неё нужно счёту. Список идёт от
 *  последней записи к первой. */
export type Counted = { no_male: number | null; no_female: number | null };

/**
 * Следующий номер записи — по полу (Роман 08.10.2026: «выбираем точный
 * вариант с привязкой к полу»).
 *
 * Счёт родившихся и умерших идёт раздельно по мальчикам и девочкам. «+1 к
 * прошлому номеру» на его данных попадает в 55 % записей, «+1 к номеру того
 * же пола» — в 99 %. Пол известен — берём наибольший номер этого пола в году
 * книги; записей этого пола нет — 1: год начинается с первого номера. Пол
 * ещё не известен (имя не набрано) — считаем, что он тот же, что у последней
 * записи; когда имя наберут, номер поправится.
 *
 * Наибольший, а не «у последней набранной»: добрали пропущенную запись № 3
 * после № 40 — следующему положен 41, а не 4 (ревьюер 08.10.2026). Замер на
 * данных заказчика сделан так же.
 *
 * null — сказать нечего (год пуст и пол неизвестен): поле не трогаем.
 */
export function nextCount(saved: Counted[], sex: Sex | null): number | null {
  const numbered = saved.filter((e) => e.no_male !== null || e.no_female !== null);
  let of: Sex | null = sex;
  if (of === null) {
    const last = numbered[0];
    if (!last) return null;
    of = last.no_male !== null ? "М" : "Ж";
  }
  let top = 0;
  for (const e of numbered) top = Math.max(top, (of === "М" ? e.no_male : e.no_female) ?? 0);
  return top + 1;
}
