#!/usr/bin/env node --experimental-strip-types
/**
 * Проверка правила «месяц — сам» — src/month.ts (Роман 09.10.2026).
 *
 * Запускается на Node 22 без сборки: node --experimental-strip-types
 * scripts/test_month.mjs. Проверяет ту же функцию, которую вызывают формы
 * рождений и смертей.
 */
import { eventMonthFor, riteMonthFor } from "../src/month.ts";

let ok = 0, bad = 0;
const check = (title, cond, detail = "") => {
  if (cond) { ok++; console.log(`  [ок]     ${title}${detail ? " — " + detail : ""}`); }
  else { bad++; console.log(`  [ОШИБКА] ${title}${detail ? " — " + detail : ""}`); }
};
const last = (event_day, event_month) => ({ event_day, event_month });

console.log("\nМесяц события по дню\n");
check("день больше, чем в последней записи, — месяц тот же", eventMonthFor(last(12, 3), 15) === 3);
check("день тот же — месяц тот же (две записи одного дня)", eventMonthFor(last(12, 3), 12) === 3);
check("день меньше — следующий месяц", eventMonthFor(last(28, 3), 2) === 4);
check("меньше на единицу — тоже следующий: порог не нужен (замер на файле заказчика)", eventMonthFor(last(12, 3), 11) === 4);
check("день ещё не набран — месяц последней записи", eventMonthFor(last(28, 3), null) === 3);
check("после декабря месяц не переводится: год человек меняет сам", eventMonthFor(last(30, 12), 2) === 12);
check("ноябрь → декабрь переводится", eventMonthFor(last(30, 11), 2) === 12);
check("последней записи нет — сказать нечего", eventMonthFor(undefined, 5) === null);
check("у последней записи нет месяца — сказать нечего", eventMonthFor(last(5, null), 3) === null);
check("у последней записи нет дня — месяц её же", eventMonthFor(last(null, 7), 3) === 7);

console.log("\nМесяц обряда по дням события и обряда\n");
check("обряд в тот же день — месяц события", riteMonthFor(10, 5, 10) === 5);
check("обряд позже в том же месяце", riteMonthFor(10, 5, 12) === 5);
check("день обряда меньше дня события — следующий месяц", riteMonthFor(29, 5, 2) === 6);
check("после декабря месяц обряда не переводится: год и отметку «в предыдущем году» ставит человек", riteMonthFor(30, 12, 3) === 12);
check("ноябрь → декабрь у обряда переводится", riteMonthFor(30, 11, 3) === 12);
check("день обряда не набран — месяц события", riteMonthFor(29, 5, null) === 5);
check("день события не набран — месяц события", riteMonthFor(null, 5, 2) === 5);
check("месяц события неизвестен — сказать нечего", riteMonthFor(29, null, 2) === null);

console.log(`\nИтог: успешно ${ok}, ошибок ${bad}`);
process.exit(bad ? 1 : 0);
