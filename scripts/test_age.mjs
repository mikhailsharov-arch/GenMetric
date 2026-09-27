#!/usr/bin/env node --experimental-strip-types
/**
 * Разбор возраста умершего — src/age.ts (раздел «Смерти», 27.09.2026).
 * Примеры — все виды записи из листа «3» Excel Романа.
 */
import { parseAge } from "../src/age.ts";

let ok = 0, bad = 0;
const check = (title, cond, detail = "") => {
  if (cond) { ok++; console.log(`  [ок]     ${title}${detail ? " — " + detail : ""}`); }
  else { bad++; console.log(`  [ОШИБКА] ${title}${detail ? " — " + detail : ""}`); }
};
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const A = (years, months, weeks, days) => ({ years, months, weeks, days });

console.log("\nВозраст умершего\n");
check("«5» — годы", same(parseAge("5"), A(5, null, null, null)));
check("«75 лет» — годы", same(parseAge("75 лет"), A(75, null, null, null)));
check("«3 мес» — месяцы", same(parseAge("3 мес"), A(null, 3, null, null)));
check("«3 мес.» с точкой", same(parseAge("3 мес."), A(null, 3, null, null)));
check("«1,5 мес» — месяц и 15 дней", same(parseAge("1,5 мес"), A(null, 1, null, 15)));
check("«2 нед» — недели", same(parseAge("2 нед"), A(null, null, 2, null)));
check("«5 дней» — дни", same(parseAge("5 дней"), A(null, null, null, 5)));
check("«1 дня» — дни", same(parseAge("1 дня"), A(null, null, null, 1)));
check("«1 день» — дни", same(parseAge("1 день"), A(null, null, null, 1)));
check("«1 год» — годы", same(parseAge("1 год"), A(1, null, null, null)));
check("пусто — null", parseAge("  ") === null);
check("непонятное — null (остаётся текстом)", parseAge("около года") === null);
check("«5 лет.» с точкой", same(parseAge("5 лет."), A(5, null, null, null)));
check("«9 лѣтъ» — старая орфография", same(parseAge("9 лѣтъ"), A(9, null, null, null)));
check("«3 мѣс.» — старая орфография", same(parseAge("3 мѣс."), A(null, 3, null, null)));
check("«1 годъ»", same(parseAge("1 годъ"), A(1, null, null, null)));
check("«1 год 3 мес» — по частям", same(parseAge("1 год 3 мес"), A(1, 3, null, null)));
check("«2 г. 6 м.» — по частям", same(parseAge("2 г. 6 м."), A(2, 6, null, null)));
check("«1 мес, 2 нед и 3 дня»", same(parseAge("1 мес, 2 нед и 3 дня"), A(null, 1, 2, 3)));
check("«1 год 2 года» — не угадываем", parseAge("1 год 2 года") === null);
check("«около 1 год 3 мес» — не возраст", parseAge("около 1 год 3 мес") === null);
check("«1 ги 3 ми» — не возраст", parseAge("1 ги 3 ми") === null);
check("«3 мес 1 год» — обратный порядок не угадываем", parseAge("3 мес 1 год") === null);
check("«10 лет 11 месяцев»", same(parseAge("10 лет 11 месяцев"), A(10, 11, null, null)));
check("«3мес» без пробела", same(parseAge("3мес"), A(null, 3, null, null)));

console.log(`\nИтог: успешно ${ok}, ошибок ${bad}`);
process.exit(bad ? 1 : 0);
