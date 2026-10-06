// Заглавные буквы в ИОФ и слово «имени нет» — src/names.ts. Запуск:
//   node --experimental-strip-types scripts/test_names.mjs
import { feminineSurname, noNameWord, showNoName, titleCase } from "../src/names.ts";

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

// «Имени нет»: копия records::is_no_name из крейта — те же примеры, что в его
// тесте no_name_and_surnames (src-tauri/core/src/records.rs).
function same(title, got, expected) {
  if (got === expected) { ok++; console.log(`  [ок]     ${title}`); }
  else { fail++; console.log(`  [ОШИБКА] ${title}: получили ${JSON.stringify(got)}, ждали ${JSON.stringify(expected)}`); }
}
for (const word of ["***", "*", "—", "?", "-", "Имя", "имя", "ИМЯ", "_", "...", "нрзб", "Нрзб.", "н/д", "Н/Д",
                    "неизвестно", "Неизв", "неизв.", "нет", "Нет", "б/и", "Б/и"]) same(`«${word}» — имени нет`, noNameWord(word), true);
for (const word of ["Иван", "2", "", "Имярек", "Им", "N", "*а", "Нета", "Неизвестнов", "нд"]) same(`«${word}» — не заглушка`, noNameWord(word), false);
same("«— Иванова Петрова» → «*** Иванова Петрова»", showNoName("— Иванова Петрова"), "*** Иванова Петрова");
same("«Имя Иванова» → «*** Иванова»", showNoName("Имя Иванова"), "*** Иванова");
same("«***» не меняется", showNoName("*** Иванова"), "*** Иванова");
same("обычное имя не меняется", showNoName("Иван Петров"), "Иван Петров");
same("цифра не меняется — её увидит окно сверки", showNoName("2 Иванова"), "2 Иванова");
same("пробел в начале сохраняется", showNoName(" ? Иванова"), " *** Иванова");
same("пустое поле не меняется", showNoName(""), "");
same("«нрзб Иванова» → «*** Иванова»", showNoName("Нрзб Иванова"), "*** Иванова");
// Фамилия отца — матери (Роман 06.10.2026).
for (const [his, hers] of [["Сидоров", "Сидорова"], ["Томский", "Томская"], ["Томилин", "Томилина"], ["Лисицын", "Лисицына"],
                           ["Соловьёв", "Соловьёва"], ["Трубецкой", "Трубецкая"], ["Белый", "Белая"], ["Толстой", "Толстая"],
                           ["Ивановъ", "Иванова"], ["Шевченко", "Шевченко"], ["Черных", "Черных"], ["Сова", "Сова"],
                           ["Палий", "Палий"], [" Петров ", "Петрова"]])
  same(`«${his}» → «${hers}»`, feminineSurname(his), hers);
console.log(`\nИтог: успешно ${ok}, ошибок ${fail}`);
process.exit(fail ? 1 : 0);
