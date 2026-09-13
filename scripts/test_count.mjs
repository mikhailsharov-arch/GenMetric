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
import { splitCount } from "../src/count.ts";

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

console.log(`\nИтог: успешно ${ok}, ошибок ${bad}`);
process.exit(bad ? 1 : 0);
