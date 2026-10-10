/**
 * Месяц события — сам (Роман 09.10.2026: «найти похожее элегантное
 * алгоритмическое решение и для месяцев… чтобы они тоже переключались
 * автоматически»).
 *
 * Записи в книге идут по порядку дней. Набрали день меньше, чем в последней
 * записи, — наступил следующий месяц; не меньше — месяц тот же. На файле
 * заказчика (2089 записей) правило угадывает месяц в 99,7 % рождений (3
 * промаха из 1068) и в 98,2 % смертей (15 из 812). В браках — 83,7 %: между
 * свадьбами бывают пропуски в несколько месяцев, по дню их не видно; там
 * правило не включено.
 *
 * После декабря месяц не переводится: дело — на год книги, год человек
 * меняет сам, и «январь» при прежнем годе был бы ошибкой в двух полях сразу.
 *
 * Функции без зависимостей — их проверяет scripts/test_month.mjs.
 */

/** Последняя запись года, сколько от неё нужно правилу. */
export type Dated = { event_day: number | null; event_month: number | null };

/**
 * Месяц события новой записи по набранному дню. null — сказать нечего
 * (последней записи нет или у неё нет дня и месяца): поле не трогаем.
 * День ещё не набран — месяц последней записи.
 */
export function eventMonthFor(last: Dated | undefined, day: number | null): number | null {
  if (!last || last.event_month === null) return null;
  if (day === null || last.event_day === null || day >= last.event_day) return last.event_month;
  return last.event_month >= 12 ? last.event_month : last.event_month + 1;
}

/**
 * Месяц обряда (крещения, погребения) по дням события и обряда: обряд не
 * бывает раньше события, значит день обряда меньше дня события — следующий
 * месяц. После декабря не переводим, как и месяц события: «январь» означал
 * бы обряд в другом году, а год и отметку «событие в предыдущем году» ставит
 * только человек — программа сама записала бы крещение раньше рождения
 * (проверяющий 09.10.2026). null — месяц события неизвестен.
 */
export function riteMonthFor(eventDay: number | null, eventMonth: number | null,
                             riteDay: number | null): number | null {
  if (eventMonth === null) return null;
  if (eventDay === null || riteDay === null || riteDay >= eventDay) return eventMonth;
  return eventMonth >= 12 ? eventMonth : eventMonth + 1;
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
 * Месяц события после смены года на форме. `prevYear` — год, для которого
 * месяц считали в прошлый раз; `last` — последняя запись нового года.
 * В новом году записи уже есть — обычное правило по его последней записи.
 * Записей нет, а год следующий за прежним — книга начинается с января.
 * null — месяц не трогаем (год тот же, форма только открыта, или сказать нечего).
 */
export function monthOnYearChange(prevYear: number | null, year: number | null,
                                  last: Dated | undefined, day: number | null): number | null {
  // Первое открытие формы — не смена года: место работы (страница, месяцы)
  // восстанавливает сама форма по последней записи.
  if (year === null || prevYear === null || year === prevYear) return null;
  if (last) return eventMonthFor(last, day);
  return year === prevYear + 1 ? 1 : null;
}

/** Дни и месяцы формы, какими их видит правило. */
export type Months = { eventDay: number | null; eventMonth: number | null; riteDay: number | null; riteMonth: number | null };
/** Какой месяц ждёт решения: набрана цифра, которая может быть началом дня. */
export type Pending = { event: boolean; rite: boolean };

/**
 * Месяцы формы после действия человека — одно место для рождений и смертей.
 *
 * `touched` — какой день только что набран; null — человек ушёл из поля дня
 * или сохраняет запись: отложенное решается сейчас. `manual` — месяц набран
 * руками, его не трогаем. Месяц события меняется только от своего дня;
 * месяц обряда идёт за событием по riteMonthFor.
 */
export function settleMonths(s: Months, last: Dated | undefined, manual: Pending,
                             touched: "event" | "rite" | null, pending: Pending): Months & { pending: Pending } {
  let { eventMonth, riteMonth } = s;
  let { event: waitEvent, rite: waitRite } = pending;
  const final = touched === null;
  if (!manual.event && (touched === "event" || (final && waitEvent))) {
    if (!final && dayUnfinished(s.eventDay, last?.event_day ?? null)) waitEvent = true;
    else {
      const guess = eventMonthFor(last, s.eventDay);
      if (guess !== null) eventMonth = guess;
      waitEvent = false;
    }
  }
  if (manual.event) waitEvent = false;
  if (!manual.rite && eventMonth !== null && (!final || waitRite || eventMonth !== s.eventMonth)) {
    // Месяц события ещё не решён — обряд ждёт вместе с ним.
    if (waitEvent || (touched === "rite" && dayUnfinished(s.riteDay, s.eventDay))) waitRite = true;
    else {
      riteMonth = riteMonthFor(s.eventDay, eventMonth, s.riteDay);
      waitRite = false;
    }
  }
  if (manual.rite) waitRite = false;
  return { ...s, eventMonth, riteMonth, pending: { event: waitEvent, rite: waitRite } };
}
