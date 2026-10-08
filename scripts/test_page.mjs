// Проверка шага по номеру страницы (src/page.ts). Запуск:
//     node --experimental-strip-types scripts/test_page.mjs
import { stepPage, normalizePage, pageOverflow } from "../src/page.ts";

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

console.log("\nПредохранитель «забытая страница» — Роман 07.10.2026");
// Шесть страниц по четыре записи — обычно 4; порог 150 % — шестая запись.
const usual = ["1", "2", "3", "4", "5", "6"].flatMap((p) => [p, p, p, p]);
const on = (n) => Array(n).fill("7");
check("на странице 4 записи, набирается пятая — молчит", pageOverflow([...usual, ...on(4)], "7"), null);
check("на странице 5 записей, набирается шестая — молчит (6 = 150 %)", pageOverflow([...usual, ...on(5)], "7"), null);
check("на странице 6 записей, набирается седьмая — предупреждает",
      JSON.stringify(pageOverflow([...usual, ...on(6)], "7")), JSON.stringify({ onPage: 6, usual: 4 }));
check("сменили страницу — молчит", pageOverflow([...usual, ...on(6)], "8"), null);
check("других страниц меньше пяти — судить не по чему",
      pageOverflow([...["1", "2", "3", "4"].flatMap((p) => [p, p]), ...on(9)], "7"), null);
check("страница на форме не указана — молчит", pageOverflow([...usual, ...on(9)], null), null);
check("записи без страницы в среднее не идут", pageOverflow([...usual, null, null, null, ...on(4)], "7"), null);
check("пробелы вокруг номера не мешают",
      pageOverflow([...usual, ...on(6)], " 7 ") !== null, true);

console.log(`\nИтог: успешно ${ok}, ошибок ${fail}`);
process.exit(fail ? 1 : 0);
