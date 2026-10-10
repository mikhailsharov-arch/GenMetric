#!/usr/bin/env node --experimental-strip-types
/**
 * Проверка правила «месяц — сам» — src/month.ts (Роман 09.10.2026).
 *
 * Запускается на Node 22 без сборки: node --experimental-strip-types
 * scripts/test_month.mjs. Проверяет ту же функцию, которую вызывают формы
 * рождений и смертей.
 */
import { dayUnfinished, eventMonthFor, monthOnYearChange, riteMonthFor, settleMonths } from "../src/month.ts";

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

console.log("\nПервая цифра двузначного дня месяц не двигает\n");
check("«1» при дне последней записи 12 — может быть началом «15»", dayUnfinished(1, 12));
check("«3» при дне 28 — может быть началом «30»", dayUnfinished(3, 28));
check("«4» началом дня не бывает", !dayUnfinished(4, 28));
check("«2» при дне 2 — и так не меньше, решать нечего", !dayUnfinished(2, 2));
check("дня для сравнения нет — решать нечего", !dayUnfinished(1, null) && !dayUnfinished(null, 5));
const none = { event: false, rite: false };
const form = (eventDay, eventMonth, riteDay, riteMonth) => ({ eventDay, eventMonth, riteDay, riteMonth });
{
  // Последняя запись — 12 марта. Набирают день рождения «15»: сначала «1».
  const a = settleMonths(form(1, 3, null, 3), last(12, 3), none, "event", none);
  check("на «1» месяц рождения не меняется, решение отложено", a.eventMonth === 3 && a.riteMonth === 3 && a.pending.event, JSON.stringify(a));
  const b = settleMonths(form(15, 3, null, 3), last(12, 3), none, "event", a.pending);
  check("на «15» месяц тот же, ждать больше нечего", b.eventMonth === 3 && !b.pending.event && !b.pending.rite, JSON.stringify(b));
  // Набрали «1» и ушли из поля: это 1-е число следующего месяца.
  const c = settleMonths(form(1, 3, null, 3), last(12, 3), none, null, a.pending);
  check("ушли из поля с «1» — следующий месяц, и обряд за ним", c.eventMonth === 4 && c.riteMonth === 4 && !c.pending.event, JSON.stringify(c));
  const d = settleMonths(form(5, 3, null, 3), last(12, 3), none, "event", none);
  check("«5» началом дня не бывает — месяц меняется сразу", d.eventMonth === 4 && d.riteMonth === 4 && !d.pending.event);
  // Крещение: рождение 28 марта, набирают «30» — сначала «3».
  const e = settleMonths(form(28, 3, 3, 3), last(12, 3), none, "rite", none);
  check("на «3» в дне крещения месяц крещения не меняется", e.riteMonth === 3 && e.pending.rite && e.eventMonth === 3, JSON.stringify(e));
  const f = settleMonths(form(28, 3, 30, 3), last(12, 3), none, "rite", e.pending);
  check("на «30» — тот же месяц", f.riteMonth === 3 && !f.pending.rite);
  const g = settleMonths(form(28, 3, 3, 3), last(12, 3), none, null, e.pending);
  check("ушли из поля с «3» — крещение в следующем месяце", g.riteMonth === 4 && g.eventMonth === 3 && !g.pending.rite, JSON.stringify(g));
  // Сохранение без отложенного ничего не пересчитывает: месяц мог быть поправлен.
  const h = settleMonths(form(28, 5, 30, 7), last(12, 3), none, null, none);
  check("уход из поля без отложенного решения месяцы не трогает", h.eventMonth === 5 && h.riteMonth === 7);
  const i = settleMonths(form(1, 7, null, 7), last(12, 3), { event: true, rite: false }, "event", none);
  check("месяц события набран руками — день его не меняет, обряд идёт за ним", i.eventMonth === 7 && i.riteMonth === 7 && !i.pending.event);
  const j = settleMonths(form(5, 3, 2, 9), last(12, 3), { event: false, rite: true }, "event", none);
  check("месяц обряда набран руками — не трогаем", j.eventMonth === 4 && j.riteMonth === 9);
  const k = settleMonths(form(20, 3, 5, 3), undefined, none, "rite", none);
  check("день обряда «5» меньше дня события и началом дня быть не может — следующий месяц сразу", k.riteMonth === 4 && !k.pending.rite, JSON.stringify(k));
}

console.log("\nМесяц при смене года на форме\n");
check("год тот же — месяц не трогаем", monthOnYearChange(1889, 1889, last(30, 12), 3) === null);
check("в новом году записи есть — по его последней записи и набранному дню", monthOnYearChange(1889, 1890, last(20, 2), 3) === 3);
check("…день не набран — месяц последней записи нового года", monthOnYearChange(1889, 1890, last(20, 2), null) === 2);
check("записей нет, год следующий — январь", monthOnYearChange(1889, 1890, undefined, 3) === 1);
check("записей нет, год не следующий — сказать нечего", monthOnYearChange(1889, 1895, undefined, 3) === null && monthOnYearChange(1889, 1888, undefined, 3) === null);
check("первое открытие формы — не смена года: месяцы восстанавливает сама форма", monthOnYearChange(null, 1889, undefined, null) === null && monthOnYearChange(null, 1889, last(30, 3), null) === null);
check("года нет — сказать нечего", monthOnYearChange(1889, null, last(1, 1), 3) === null);

console.log(`\nИтог: успешно ${ok}, ошибок ${bad}`);
process.exit(bad ? 1 : 0);
