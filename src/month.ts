/**
 * Месяц — сам (Роман 09.10.2026: «найти похожее элегантное алгоритмическое
 * решение и для месяцев… чтобы они тоже переключались автоматически»).
 *
 * Записи в книге идут по порядку дней ОБРЯДА — крещения и погребения, а не
 * рождения и смерти (Роман 12.10.2026: «хронология записей в метрических
 * книгах всегда идет по датам обрядов… в одной записи рождение и крещение
 * стоит 22-го числа, а в следующей записи рождение 21-го, а крещение — 22-го»).
 * Поэтому: день обряда меньше, чем в последней записи, — наступил следующий
 * месяц; не меньше — месяц тот же. Месяц события считается от месяца обряда:
 * день события больше дня обряда — событие было в предыдущем месяце.
 *
 * На файле заказчика (1072 рождения, 812 смертей) правило по дню обряда
 * промахивается 3 и 12 раз (99,7 % и 98,5 %); прежнее, по дню события, — 4 и
 * 15. В браках — 83,7 %: между свадьбами бывают пропуски в несколько
 * месяцев, по дню их не видно; там правило не включено.
 *
 * После декабря месяц не переводится: дело — на год книги, год человек
 * меняет сам, и «январь» при прежнем годе был бы ошибкой в двух полях сразу.
 *
 * Функции без зависимостей — их проверяет scripts/test_month.mjs.
 */

/** Последняя запись года, сколько от неё нужно правилу. */
export type Dated = {
  event_day: number | null; event_month: number | null;
  rite_day?: number | null; rite_month?: number | null;
};

/** День и месяц, по которым запись стоит в книге: обряд, а без него — событие. */
export const bookDay = (last: Dated | undefined): number | null => last?.rite_day ?? last?.event_day ?? null;
export const bookMonth = (last: Dated | undefined): number | null => last?.rite_month ?? last?.event_month ?? null;

/**
 * Месяц новой записи по её дню в книге (дню обряда; без обряда — события).
 * null — сказать нечего (последней записи нет или у неё нет месяца): поле не
 * трогаем. День ещё не набран — месяц последней записи.
 */
export function monthAfter(last: Dated | undefined, day: number | null): number | null {
  const month = bookMonth(last), was = bookDay(last);
  if (month === null) return null;
  if (day === null || was === null || day >= was) return month;
  return month >= 12 ? month : month + 1;
}

/**
 * Месяц события по месяцу обряда: событие не бывает позже обряда, значит
 * день события больше дня обряда — событие в предыдущем месяце. В январе
 * назад не переводим: «декабрь» означал бы событие в прошлом году, а год и
 * отметку «событие в предыдущем году» ставит только человек (проверяющий
 * 09.10.2026 — про тот же переход в другую сторону). null — месяц обряда
 * неизвестен.
 */
export function eventMonthFrom(riteMonth: number | null, eventDay: number | null,
                               riteDay: number | null): number | null {
  if (riteMonth === null) return null;
  if (eventDay === null || riteDay === null || eventDay <= riteDay) return riteMonth;
  return riteMonth <= 1 ? riteMonth : riteMonth - 1;
}

/**
 * Месяц обряда после того, как человек набрал месяц события `event` руками.
 * Обычно он стоит на месте. Пустой — заполняется. `follow` (новая запись,
 * месяц обряда руками не набран): событие оказалось позже обряда — обряд
 * встаёт в месяц события. Обряд в январе при событии в ноябре-декабре — не
 * «позже», а переход через год. `filled` — месяц обряда был пуст и заполнен
 * этой же правкой месяца события.
 */
export function followEvent(rite: number | null, event: number | null, follow: boolean,
                            filled = false): number | null {
  // Запись на правке, месяц обряда набран руками — обряд стоит, и пустой тоже.
  if (!follow) return rite;
  // Пустой заполняется; и пока он заполнен этим же набором (`filled`), идёт
  // за событием цифра в цифру: месяц «12» набирают как «1» и «2», и обряд,
  // застрявший на «1», сошёл бы за «переход через год» (ревьюер 13.10.2026).
  if (rite === null || filled) return event;
  if (event === null || event <= rite || (rite === 1 && event >= 11)) return rite;
  return event;
}

/**
 * Правило упёрлось в декабрь: день в книге меньше, чем в последней,
 * декабрьской, записи — на деле это уже январь, но месяц после декабря
 * программа не переводит. Месяц события тогда назад не считается: «рождение
 * 30-го, крещение 2-го» после 31 декабря — это декабрь и январь, а не ноябрь
 * и декабрь (проверяющий 13.10.2026).
 */
export function pastDecember(last: Dated | undefined, day: number | null): boolean {
  const was = bookDay(last);
  return bookMonth(last) === 12 && day !== null && was !== null && day < was;
}

/**
 * Набранный день может оказаться первой цифрой двузначного: «1» — начало
 * «15». Пока это так и цифра меньше дня, с которым её сравнивают, месяц не
 * трогаем: иначе на первой цифре он прыгал на следующий, на второй —
 * обратно, и форма дёргалась (ревьюер 09.10.2026). Решение — когда набрана
 * вторая цифра или человек ушёл из поля.
 */
export function dayUnfinished(day: number | null, than: number | null): boolean {
  return day !== null && than !== null && day >= 1 && day <= 3 && day < than;
}

/**
 * Месяц обряда после смены года на форме. `prevYear` — год, для которого
 * месяц считали в прошлый раз; `last` — последняя запись нового года; `day`
 * — день новой записи в книге.
 * В новом году записи уже есть — обычное правило по его последней записи.
 * Записей нет, а год следующий за прежним — книга начинается с января.
 * null — месяц не трогаем (год тот же, форма только открыта, или сказать нечего).
 */
export function monthOnYearChange(prevYear: number | null, year: number | null,
                                  last: Dated | undefined, day: number | null): number | null {
  // Первое открытие формы — не смена года: место работы (страница, месяцы)
  // восстанавливает сама форма по последней записи.
  if (year === null || prevYear === null || year === prevYear) return null;
  if (last) return monthAfter(last, day);
  return year === prevYear + 1 ? 1 : null;
}

/** Дни и месяцы формы, какими их видит правило. */
export type Months = { eventDay: number | null; eventMonth: number | null; riteDay: number | null; riteMonth: number | null };
/** Какой месяц набран руками — его правило не трогает. */
export type Manual = { event: boolean; rite: boolean };
/**
 * Что правило помнит между нажатиями. `rite` — в дне обряда стоит цифра,
 * которая может быть началом двузначного дня: решение отложено. `byEvent` —
 * из пустого дня обряда уже ушли, и месяц решён по дню события: пока обряд
 * пуст, правка дня события решает заново сразу.
 */
export type Wait = { rite: boolean; byEvent: boolean };
export const NO_WAIT: Wait = { rite: false, byEvent: false };

/**
 * Месяцы формы после действия человека — одно место для рождений и смертей.
 *
 * `touched` — какой день только что набран; null — человек ушёл из поля дня
 * обряда или сохраняет запись: отложенное решается сейчас. `pending` — что
 * правило помнит с прошлого нажатия (`Wait`).
 *
 * Месяц обряда меняется только от дня обряда. Пока дня обряда нет, месяцы
 * стоят: день события набирают первым, и решать по нему рано — сначала
 * месяц прыгнул бы по дню рождения, потом вернулся по дню крещения. Обряд
 * так и не набран (некрещёный, обряд без даты) — при уходе из поля или
 * сохранении правило считается по дню события. Месяц события идёт за
 * месяцем обряда; набранный руками — стоит, и месяц обряда за собой не ведёт
 * (Роман 12.10.2026).
 */
export function settleMonths(s: Months, last: Dated | undefined, manual: Manual,
                             touched: "event" | "rite" | null, pending: Wait): Months & { pending: Wait } {
  let { eventMonth, riteMonth } = s;
  let wait = pending.rite;
  // День обряда появился — решает он; режим «по дню события» кончился.
  let byEvent = pending.byEvent && s.riteDay === null && !manual.rite;
  let redone = false;
  const final = touched === null;
  if (manual.rite) wait = false;
  else if (touched === "rite") {
    if (dayUnfinished(s.riteDay, bookDay(last))) wait = true;
    else {
      // День обряда стёрт — месяц последней записи, как до набора.
      const guess = monthAfter(last, s.riteDay);
      if (guess !== null) riteMonth = guess;
      wait = false;
    }
  } else if (final && (wait || s.riteDay === null) && (s.riteDay ?? s.eventDay) !== null) {
    const guess = monthAfter(last, s.riteDay ?? s.eventDay);
    if (guess !== null) riteMonth = guess;
    wait = false;
    byEvent = s.riteDay === null;
  } else if (touched === "event" && byEvent && bookMonth(last) !== null) {
    // Обряд пуст, и по дню события здесь уже решали (ушли из пустого дня
    // обряда). День события правят — решаем заново сразу, иначе до сохранения
    // на экране стоял бы месяц от прежнего, ошибочного дня. Первая цифра
    // двузначного дня — пока месяц последней записи; уход из поля или
    // сохранение решат окончательно.
    riteMonth = dayUnfinished(s.eventDay, bookDay(last)) ? bookMonth(last) : monthAfter(last, s.eventDay);
    redone = true;
  }
  // Решение о месяце обряда отложено — событие ждёт вместе с ним.
  const settled = s.riteDay !== null ? !wait : redone || (final && s.eventDay !== null);
  if (!manual.event && settled && riteMonth !== null)
    eventMonth = !manual.rite && riteMonth === 12 && pastDecember(last, s.riteDay ?? s.eventDay)
      ? 12 : eventMonthFrom(riteMonth, s.eventDay, s.riteDay);
  return { ...s, eventMonth, riteMonth, pending: { rite: wait, byEvent } };
}
