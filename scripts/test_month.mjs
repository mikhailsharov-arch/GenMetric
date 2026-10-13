#!/usr/bin/env node --experimental-strip-types
/**
 * Проверка правила «месяц — сам» — src/month.ts (Роман 09.10.2026).
 *
 * Запускается на Node 22 без сборки: node --experimental-strip-types
 * scripts/test_month.mjs. Проверяет ту же функцию, которую вызывают формы
 * рождений и смертей.
 */
import { bookDay, bookMonth, dayUnfinished, eventMonthFrom, followEvent, pastDecember, monthAfter, monthOnYearChange, settleMonths } from "../src/month.ts";

let ok = 0, bad = 0;
const check = (title, cond, detail = "") => {
  if (cond) { ok++; console.log(`  [ок]     ${title}${detail ? " — " + detail : ""}`); }
  else { bad++; console.log(`  [ОШИБКА] ${title}${detail ? " — " + detail : ""}`); }
};
// Последняя запись: день и месяц события, день и месяц обряда.
const last = (event_day, event_month, rite_day = event_day, rite_month = event_month) => ({ event_day, event_month, rite_day, rite_month });

console.log("\nПо какому дню запись стоит в книге\n");
check("обряд есть — день и месяц обряда", bookDay(last(28, 3, 2, 4)) === 2 && bookMonth(last(28, 3, 2, 4)) === 4);
check("обряда нет — день и месяц события", bookDay(last(28, 3, null, null)) === 28 && bookMonth(last(28, 3, null, null)) === 3);
check("запись без дня обряда из прежней сборки (поля нет вовсе)", bookDay({ event_day: 7, event_month: 2 }) === 7 && bookMonth({ event_day: 7, event_month: 2, rite_month: 3 }) === 3);
check("записи нет — сказать нечего", bookDay(undefined) === null && bookMonth(undefined) === null);

console.log("\nМесяц по дню обряда\n");
check("день больше, чем в последней записи, — месяц тот же", monthAfter(last(12, 3), 15) === 3);
check("день тот же — месяц тот же (две записи одного дня)", monthAfter(last(12, 3), 12) === 3);
check("день меньше — следующий месяц", monthAfter(last(28, 3), 2) === 4);
check("сравнивается день обряда последней записи, а не день события", monthAfter(last(28, 3, 2, 4), 3) === 4 && monthAfter(last(28, 3, 2, 4), 1) === 5);
check("день ещё не набран — месяц последней записи", monthAfter(last(28, 3), null) === 3);
check("после декабря месяц не переводится: год человек меняет сам", monthAfter(last(30, 12), 2) === 12);
check("ноябрь → декабрь переводится", monthAfter(last(30, 11), 2) === 12);
check("последней записи нет — сказать нечего", monthAfter(undefined, 5) === null);
check("у последней записи нет месяца — сказать нечего", monthAfter(last(5, null), 3) === null);
check("у последней записи нет дня — месяц её же", monthAfter(last(null, 7), 3) === 7);

console.log("\nМесяц события по месяцу обряда\n");
check("обряд в тот же день — тот же месяц", eventMonthFrom(5, 10, 10) === 5);
check("обряд позже в том же месяце", eventMonthFrom(5, 10, 12) === 5);
check("день события больше дня обряда — событие в предыдущем месяце", eventMonthFrom(6, 29, 2) === 5);
check("в январе назад не переводим: год и отметку «в предыдущем году» ставит человек", eventMonthFrom(1, 30, 3) === 1);
check("день обряда не набран — месяц обряда", eventMonthFrom(5, 29, null) === 5);
check("день события не набран — месяц обряда", eventMonthFrom(5, null, 2) === 5);
check("месяц обряда неизвестен — сказать нечего", eventMonthFrom(null, 29, 2) === null);

console.log("\nМесяц обряда после ручной правки месяца события\n");
check("месяц события опустили (родился в конце прошлого месяца) — обряд стоит", followEvent(4, 3, true) === 4);
check("месяц события тот же — обряд стоит", followEvent(4, 4, true) === 4);
check("месяц события подняли выше обряда — обряд идёт за ним: раньше события он не бывает", followEvent(4, 12, true) === 12);
check("обряд в январе, событие в декабре или ноябре — переход через год, обряд стоит", followEvent(1, 12, true) === 1 && followEvent(1, 11, true) === 1);
check("месяц обряда пуст — заполняется; у записи на правке — нет", followEvent(null, 5, true) === 5 && followEvent(null, 5, false) === null);
check("первая запись раздела, месяц «12» набирают по цифре: обряд идёт за событием 1 → 12, а не застревает на январе",
      followEvent(followEvent(null, 1, true, true), 12, true, true) === 12 && followEvent(1, 11, true, true) === 11);
check("…и вниз тоже, пока заполнен этим набором: 12 → 1 (стёрли цифру)", followEvent(12, 1, true, true) === 1 && followEvent(12, null, true, true) === null);
check("запись на правке или месяц обряда набран руками — обряд стоит", followEvent(4, 12, false) === 4);
check("месяц события стёрли — обряд стоит", followEvent(4, null, true) === 4);

console.log("\nПервая цифра двузначного дня месяц не двигает\n");
check("«1» при дне последней записи 12 — может быть началом «15»", dayUnfinished(1, 12));
check("«3» при дне 28 — может быть началом «30»", dayUnfinished(3, 28));
check("«4» началом дня не бывает", !dayUnfinished(4, 28));
check("«2» при дне 2 — и так не меньше, решать нечего", !dayUnfinished(2, 2));
check("дня для сравнения нет — решать нечего", !dayUnfinished(1, null) && !dayUnfinished(null, 5));

console.log("\nМесяцы формы\n");
const none = { event: false, rite: false };
const no = { rite: false, byEvent: false }, yes = { rite: true, byEvent: false }, byEvent = { rite: false, byEvent: true };
const form = (eventDay, eventMonth, riteDay, riteMonth) => ({ eventDay, eventMonth, riteDay, riteMonth });
const months = (o) => `${o.eventMonth}/${o.riteMonth}${o.pending.rite ? " ждёт" : ""}${o.pending.byEvent ? " по событию" : ""}`;
{
  // Пример Романа 12.10.2026: рождение и крещение 22-го, следом рождение
  // 21-го и крещение 22-го — месяц прежний.
  const prev = last(22, 3, 22, 3);
  const a = settleMonths(form(21, 3, null, 3), prev, none, "event", no);
  check("пример Романа: набрали день рождения 21 после 22 — месяцы стоят, решать рано", a.eventMonth === 3 && a.riteMonth === 3 && !a.pending.rite, months(a));
  const b = settleMonths(form(21, 3, 22, 3), prev, none, "rite", no);
  check("пример Романа: день крещения 22 — оба месяца прежние", b.eventMonth === 3 && b.riteMonth === 3, months(b));
  // Обычный переход месяца: последняя запись крещена 28 марта.
  const c = settleMonths(form(30, 3, 5, 3), last(27, 3, 28, 3), none, "rite", no);
  check("день крещения 5 после 28 — крещение в апреле, рождение 30-го — в марте", c.riteMonth === 4 && c.eventMonth === 3, months(c));
  const d = settleMonths(form(2, 3, 5, 3), last(27, 3, 28, 3), none, "rite", no);
  check("день крещения 5 после 28, рождение 2-го — оба в апреле", d.riteMonth === 4 && d.eventMonth === 4, months(d));
  // Первая цифра двузначного дня обряда.
  const e = settleMonths(form(10, 3, 1, 3), last(12, 3), none, "rite", no);
  check("на «1» в дне крещения месяцы не меняются, решение отложено", e.riteMonth === 3 && e.eventMonth === 3 && e.pending.rite, months(e));
  const f = settleMonths(form(10, 3, 15, 3), last(12, 3), none, "rite", e.pending);
  check("на «15» — месяц тот же, ждать больше нечего", f.riteMonth === 3 && f.eventMonth === 3 && !f.pending.rite, months(f));
  const g = settleMonths(form(30, 3, 1, 3), last(12, 3), none, null, yes);
  check("ушли из поля с «1» — крещение в апреле, рождение 30-го осталось в марте", g.riteMonth === 4 && g.eventMonth === 3 && !g.pending.rite, months(g));
  // День события месяц обряда не двигает.
  const h = settleMonths(form(2, 3, null, 3), last(28, 3), none, "event", no);
  check("день рождения 2 после 28, крещение не набрано — месяцы ждут дня крещения", h.eventMonth === 3 && h.riteMonth === 3 && !h.pending.rite, months(h));
  const i = settleMonths(form(2, 3, null, 3), last(28, 3), none, null, no);
  check("крещение так и не набрано, ушли из поля или сохраняют — считаем по дню рождения", i.eventMonth === 4 && i.riteMonth === 4, months(i));
  const j = settleMonths(form(null, 3, null, 3), last(28, 3), none, null, no);
  check("дни не набраны — уход из поля месяцы не трогает", j.eventMonth === 3 && j.riteMonth === 3);
  // Обряд пуст, по дню события уже решили — день события правят.
  check("уход из пустого дня обряда включает режим «по дню события»", i.pending.byEvent && !h.pending.byEvent);
  const w0 = settleMonths(form(30, 4, null, 4), last(28, 3), none, "event", no);
  check("без этого режима день события месяцы не трогает", w0.eventMonth === 4 && w0.riteMonth === 4, months(w0));
  const w1 = settleMonths(form(30, 4, 5, 4), last(28, 3), none, "rite", byEvent);
  check("день обряда появился — режим кончился, решает день обряда", !w1.pending.byEvent && w1.riteMonth === 4 && w1.eventMonth === 3, months(w1));
  const w = settleMonths(form(30, 4, null, 4), last(28, 3), none, "event", byEvent);
  check("решили по дню рождения (апрель), день исправили на 30 — месяц сразу вернулся к марту", w.eventMonth === 3 && w.riteMonth === 3, months(w));
  const x = settleMonths(form(null, 4, null, 4), last(28, 3), none, "event", byEvent);
  check("…день стёрли — месяц последней записи", x.eventMonth === 3 && x.riteMonth === 3, months(x));
  const y = settleMonths(form(1, 4, null, 4), last(28, 3), none, "event", byEvent);
  check("…набирают заново, первая цифра «1» — пока месяц последней записи", y.eventMonth === 3 && y.riteMonth === 3, months(y));
  const z = settleMonths(form(5, 4, null, 4), last(28, 3), none, "event", byEvent);
  check("…новый день 5 — снова следующий месяц", z.eventMonth === 4 && z.riteMonth === 4, months(z));
  // Уже решённое при сохранении не пересчитывается.
  const k = settleMonths(form(30, 3, 5, 4), last(27, 3, 28, 3), none, null, no);
  check("сохранение без отложенного решения месяц обряда не трогает", k.riteMonth === 4 && k.eventMonth === 3, months(k));
  // Ручная правка.
  const l = settleMonths(form(30, 2, 5, 3), last(27, 3, 28, 3), { event: true, rite: false }, "rite", no);
  check("месяц события набран руками — стоит, месяц обряда считается своим порядком", l.eventMonth === 2 && l.riteMonth === 4, months(l));
  const m = settleMonths(form(30, 3, 5, 9), last(27, 3, 28, 3), { event: false, rite: true }, "rite", no);
  check("месяц обряда набран руками — стоит, событие считается от него", m.riteMonth === 9 && m.eventMonth === 8, months(m));
  const n = settleMonths(form(30, 2, 1, 9), last(12, 3), { event: false, rite: true }, "rite", yes);
  check("месяц обряда набран руками — отложенного решения нет", !n.pending.rite && n.riteMonth === 9);
  // Правка дня события после дня обряда.
  const o = settleMonths(form(30, 4, 5, 4), last(27, 3, 28, 3), none, "event", no);
  check("день рождения поправили на 30 при крещении 5-го — рождение в предыдущем месяце", o.eventMonth === 3 && o.riteMonth === 4, months(o));
  const p = settleMonths(form(3, 3, 5, 4), last(27, 3, 28, 3), none, "event", no);
  check("день рождения поправили на 3 при крещении 5-го — тот же месяц", p.eventMonth === 4 && p.riteMonth === 4, months(p));
  // День обряда стёрли.
  const q = settleMonths(form(30, 3, null, 4), last(27, 3, 28, 3), none, "rite", no);
  check("день крещения стёрли — месяц крещения снова как в последней записи", q.riteMonth === 3, months(q));
  // Декабрь и январь.
  const r = settleMonths(form(30, 12, 3, 12), last(27, 12, 28, 12), none, "rite", no);
  check("после декабря месяц обряда не переводится", r.riteMonth === 12 && r.eventMonth === 12, months(r));
  const t = settleMonths(form(30, 1, 3, 1), last(1, 1, 2, 1), none, "rite", no);
  check("январь: день рождения больше дня крещения — месяц рождения назад не переводится", t.riteMonth === 1 && t.eventMonth === 1, months(t));
  const dec = settleMonths(form(30, 12, 5, 12), last(30, 12, 31, 12), none, "rite", no);
  check("после 31 декабря: рождение 30, крещение 5 — месяц рождения остаётся декабрём, а не ноябрём", dec.riteMonth === 12 && dec.eventMonth === 12, months(dec));
  const dec2 = settleMonths(form(28, 12, 29, 12), last(26, 12, 27, 12), none, "rite", no);
  check("обычный декабрь: рождение 28, крещение 29 — оба декабрь", dec2.riteMonth === 12 && dec2.eventMonth === 12, months(dec2));
  const nov = settleMonths(form(30, 11, 5, 11), last(28, 11, 29, 11), none, "rite", no);
  check("ноябрь → декабрь: рождение 30, крещение 5 — крещение в декабре, рождение в ноябре", nov.riteMonth === 12 && nov.eventMonth === 11, months(nov));
  check("упёрлись в декабрь — только когда день меньше, чем в декабрьской записи", pastDecember(last(30, 12, 31, 12), 2) && !pastDecember(last(30, 12, 31, 12), 31) && !pastDecember(last(28, 11, 29, 11), 2) && !pastDecember(undefined, 2));
  // Последняя запись без обряда.
  const u = settleMonths(form(4, 3, 5, 3), last(28, 3, null, null), none, "rite", no);
  check("у последней записи нет обряда — сравниваем с её днём события", u.riteMonth === 4 && u.eventMonth === 4, months(u));
  const v = settleMonths(form(20, 3, 5, 3), undefined, none, "rite", no);
  check("последней записи нет — месяц обряда не трогаем, событие от него", v.riteMonth === 3 && v.eventMonth === 2, months(v));
}

console.log("\nМесяц при смене года на форме\n");
check("год тот же — месяц не трогаем", monthOnYearChange(1889, 1889, last(30, 12), 3) === null);
check("в новом году записи есть — по его последней записи и набранному дню", monthOnYearChange(1889, 1890, last(20, 2), 3) === 3);
check("…сравнивается день обряда его последней записи", monthOnYearChange(1889, 1890, last(20, 2, 2, 3), 3) === 3 && monthOnYearChange(1889, 1890, last(20, 2, 2, 3), 1) === 4);
check("…день не набран — месяц последней записи нового года", monthOnYearChange(1889, 1890, last(20, 2), null) === 2);
check("записей нет, год следующий — январь", monthOnYearChange(1889, 1890, undefined, 3) === 1);
check("записей нет, год не следующий — сказать нечего", monthOnYearChange(1889, 1895, undefined, 3) === null && monthOnYearChange(1889, 1888, undefined, 3) === null);
check("первое открытие формы — не смена года: месяцы восстанавливает сама форма", monthOnYearChange(null, 1889, undefined, null) === null && monthOnYearChange(null, 1889, last(30, 3), null) === null);
check("года нет — сказать нечего", monthOnYearChange(1889, null, last(1, 1), 3) === null);

console.log(`\nИтог: успешно ${ok}, ошибок ${bad}`);
process.exit(bad ? 1 : 0);
