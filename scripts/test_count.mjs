#!/usr/bin/env node --experimental-strip-types
/**
 * Проверка раскладки счёта по полу ребёнка — src/count.ts.
 *
 * Запускается на Node 22 без сборки: node --experimental-strip-types
 * scripts/test_count.mjs. Проверяет ту же функцию, которую вызывает форма,
 * а не её пересказ.
 *
 * Инцидент 13.09.2026: счёт всегда ложился в мужскую колонку. Половина
 * записей с неверным номером, на экране не видно.
 */
import { eventYearOf, riteBeforeEvent, splitCount, nextCount } from "../src/count.ts";

let ok = 0, bad = 0;
const check = (title, cond, detail = "") => {
  if (cond) { ok++; console.log(`  [ок]     ${title}${detail ? " — " + detail : ""}`); }
  else { bad++; console.log(`  [ОШИБКА] ${title}${detail ? " — " + detail : ""}`); }
};
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

console.log("\nСчёт родившихся по полу ребёнка\n");

check("девочка → женская колонка, мужская пуста",
  same(splitCount(3, "Ж"), { no_male: null, no_female: 3 }));
check("мальчик → мужская колонка, женская пуста",
  same(splitCount(5, "М"), { no_male: 5, no_female: null }));
check("пол неизвестен, счёт есть → сохранять нельзя",
  splitCount(2, null) === null);
check("счёта нет → обе колонки пусты, пол не важен",
  same(splitCount(null, null), { no_male: null, no_female: null }));
check("счёта нет, пол известен → обе колонки пусты",
  same(splitCount(null, "Ж"), { no_male: null, no_female: null }));
check("ноль — тоже счёт, а не «нет счёта»",
  same(splitCount(0, "М"), { no_male: 0, no_female: null }));

// Ровно то, что было поломкой: девочка не должна попасть в мужскую колонку
// ни при каком поле.
for (const sex of ["Ж"]) {
  const r = splitCount(1, sex);
  check("девочка никогда не в мужской колонке", r && r.no_male === null);
}

console.log("\nСобытие в предыдущем году — только по отметке человека\n");
check("декабрь → январь — спросить", riteBeforeEvent(12, 1) === true);
check("тот же месяц или позже — не спрашивать", !riteBeforeEvent(5, 5) && !riteBeforeEvent(5, 6));
check("месяц неизвестен — не спрашивать", !riteBeforeEvent(null, 1) && !riteBeforeEvent(12, null));
check("не отмечено — год события = год книги", eventYearOf(1887, 12, 1, false) === 1887);
check("отмечено — событие в предыдущем году", eventYearOf(1887, 12, 1, true) === 1886);
check("отмечено, но месяцы уже не наоборот — год книги", eventYearOf(1887, 5, 6, true) === 1887);

console.log("\nСледующий номер — по полу (Роман 08.10.2026)\n");
// Список «Набрано» идёт от последней записи к первой.
const list = [{ no_male: 15, no_female: null }, { no_male: null, no_female: 12 }, { no_male: 14, no_female: null }];
check("тот же пол, что у последней записи: мальчик 15 → 16", nextCount(list, "М") === 16);
check("другой пол: последняя девочка 12 → 13", nextCount(list, "Ж") === 13);
check("пол ещё не известен — как у последней записи", nextCount(list, null) === 16);
check("девочек в году ещё нет → 1", nextCount([{ no_male: 3, no_female: null }], "Ж") === 1);
check("год пуст, пол известен → 1", nextCount([], "М") === 1);
check("год пуст, пол неизвестен → поле не трогаем", nextCount([], null) === null);
check("записи без номера пропускаются",
  nextCount([{ no_male: null, no_female: null }, { no_male: null, no_female: 7 }], null) === 8);
check("добрали пропущенный № 2 после № 40 — следующему 41, а не 3",
  nextCount([{ no_male: 2, no_female: null }, { no_male: 40, no_female: null }], "М") === 41);
check("то же, пока пол не известен: по полу последней записи, наибольший номер",
  nextCount([{ no_male: 2, no_female: null }, { no_male: null, no_female: 9 }, { no_male: 40, no_female: null }], null) === 41);

console.log(`\nИтог: успешно ${ok}, ошибок ${bad}`);
process.exit(bad ? 1 : 0);
