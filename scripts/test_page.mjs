// Проверка шага по номеру страницы (src/page.ts). Запуск:
//     node --experimental-strip-types scripts/test_page.mjs
import { stepPage, normalizePage } from "../src/page.ts";

let ok = 0, fail = 0;
function check(title, got, want) {
  if (got === want) { ok++; console.log(`  [ок]     ${title}`); }
  else { fail++; console.log(`  [ОШИБКА] ${title} — получено «${got}», ожидалось «${want}»`); }
}

console.log("\nШаг по странице — заказчик 21.09.2026");
check("разворот, плюс: 938об-939 → 939об-940", stepPage("938об-939", 1), "939об-940");
check("разворот, минус: 938об-939 → 937об-938", stepPage("938об-939", -1), "937об-938");
check("простое число, плюс", stepPage("957", 1), "958");
check("простое число, минус", stepPage("957", -1), "956");
check("оборот одного листа", stepPage("12об", 1), "13об");
check("разворот с оборотом справа", stepPage("12-13об", 1), "13-14об");
check("пробелы вокруг дефиса сохраняются", stepPage("938об - 939", 1), "939об - 940");
check("пусто, плюс → 1", stepPage("", 1), "1");
check("пусто, минус → пусто", stepPage(null, -1), "");
check("ниже нуля не уходит", stepPage("0", -1), "0");
check("без чисел не меняется", stepPage("об", 1), "об");
check("нормализация: пробелы срезаются", normalizePage("  938об-939 "), "938об-939");
check("нормализация: пустое → null", normalizePage("   "), null);

console.log(`\nИтог: успешно ${ok}, ошибок ${fail}`);
process.exit(fail ? 1 : 0);
