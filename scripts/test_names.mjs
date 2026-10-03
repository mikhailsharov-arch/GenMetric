// Заглавные буквы в ИОФ — src/names.ts. Запуск:
//   node --experimental-strip-types scripts/test_names.mjs
import { titleCase } from "../src/names.ts";

let ok = 0, fail = 0;
function check(input, expected) {
  const got = titleCase(input);
  if (got === expected) { ok++; console.log(`  [ок]     «${input}» → «${got}»`); }
  else { fail++; console.log(`  [ОШИБКА] «${input}» → «${got}», ждали «${expected}»`); }
}
check("иван иванов", "Иван Иванов");
check("Иван Иванов", "Иван Иванов");
check("мария", "Мария");
check("  анна  петрова ", "  Анна  Петрова ");
check("анна-мария фон-дервиз", "Анна-Мария Фон-Дервиз");
check("евлампия васильева (сидорова)", "Евлампия Васильева (Сидорова)");
check("ёлкин", "Ёлкин");
check("иоаннъ", "Иоаннъ");
check("МакАртур", "МакАртур");
check("***", "***");
check("", "");
console.log(`\nИтог: успешно ${ok}, ошибок ${fail}`);
process.exit(fail ? 1 : 0);
