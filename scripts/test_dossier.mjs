#!/usr/bin/env node --experimental-strip-types
/**
 * Проверка слов досье — src/dossier.ts (Роман 09.10.2026: «хронологическое
 * „досье“ человека»). Запуск: node --experimental-strip-types scripts/test_dossier.mjs
 */
import { ageWords, dateOf, describe, family, nameless, personTail, roleKind } from "../src/dossier.ts";

let ok = 0, bad = 0;
const check = (title, cond, detail = "") => {
  if (cond) { ok++; console.log(`  [ок]     ${title}${detail ? " — " + detail : ""}`); }
  else { bad++; console.log(`  [ОШИБКА] ${title}${detail ? " — " + detail : ""}`); }
};
const m = (role_code, iof, extra = {}) => ({ key: iof.toLowerCase(), role_code, iof, iof_modern: iof, place: "", rank: null, gender: null, age: null,
  death_cause: null, marriage_order: null, kinship: null, note: null, ...extra });
const ev = (section, me, others, extra = {}) => ({ entry_id: 1, section, year: 1889, day: 12, month: 3, page: "14", note: null, me, others, ...extra });
/** Строка события одной строкой текста: люди — в «ёлочках». */
const text = (e) => { const d = describe(e); return d.what + (d.who.length ? ". " : "") + d.who.map((p) => (typeof p === "string" ? p : `«${p.person.iof}»`)).join(""); };

console.log("\nДата и возраст\n");
check("день и месяц", dateOf({ day: 12, month: 3 }) === "12 марта");
check("только месяц — в именительном", dateOf({ day: null, month: 3 }) === "март" && dateOf({ day: null, month: 5 }) === "май" && dateOf({ day: null, month: 1 }) === "январь",
      `${dateOf({ day: null, month: 3 })}, ${dateOf({ day: null, month: 5 })}, ${dateOf({ day: null, month: 1 })}`);
check("ничего не известно — пусто", dateOf({ day: null, month: null }) === "");
check("возраст: число — годы со словом", ageWords("60") === "60 лет" && ageWords("21") === "21 год" && ageWords("22") === "22 года" && ageWords("11") === "11 лет" && ageWords("1") === "1 год");
check("возраст как в книге остаётся как есть", ageWords("3 мес") === "3 мес" && ageWords("") === "" && ageWords(null) === "");

console.log("\nСтроки событий\n");
const father = m("father", "Иван Капитонов", { gender: "М", place: "Фетинино" });
const mother = m("mother", "Анна Петрова", { gender: "Ж", place: "Фетинино" });
const girl = m("child", "Мария", { gender: "Ж" });
const god = m("godparent1", "Пётр Сидоров Орлов", { gender: "М", place: "Малово" });
check("отец: рождение дочери, мать и восприемники",
  text(ev(1, father, [girl, mother, god])) === "рождение дочери. «Мария»; мать — «Анна Петрова»; восприемники: «Пётр Сидоров Орлов»", text(ev(1, father, [girl, mother, god])));
check("мать: рождение сына, отец",
  text(ev(1, mother, [m("child", "Пётр", { gender: "М" }), father])) === "рождение сына. «Пётр»; отец — «Иван Капитонов»");
check("пол ребёнка неизвестен — «ребёнка»; без матери и восприемников строка короче",
  text(ev(1, father, [m("child", "Нрзб")])) === "рождение ребёнка. «Нрзб»");
check("восприемник: чей ребёнок", text(ev(1, god, [girl, father, mother])) === "восприемник. ребёнок — «Мария»; отец — «Иван Капитонов»; мать — «Анна Петрова»");
check("восприемница — по полу", describe(ev(1, m("godparent2", "Дарья Иванова", { gender: "Ж" }), [girl, mother])).what === "восприемница");
const groom = m("groom", "Фёдор Иванов", { gender: "М", age: "22", marriage_order: "Первым браком" });
const bride = m("bride", "Дарья Петрова", { gender: "Ж", age: "19", place: "Воспица" });
const w1 = m("witness1", "Иван Капитонов", { note: "по жениху" }), w2 = m("witness2", "Семён Иванов", { note: "по невесте" });
const grel = m("groom_relative", "Иван Фёдоров", { kinship: "отец" });
check("жених: возраст, каким браком, невеста, родственник, поручители по сторонам",
  text(ev(2, groom, [bride, grel, w1, w2])) === "брак (22 года, первым браком). невеста — «Дарья Петрова»; отец жениха — «Иван Фёдоров»; поручители по жениху: «Иван Капитонов»; поручители по невесте: «Семён Иванов»", text(ev(2, groom, [bride, grel, w1, w2])));
check("невеста: жених", text(ev(2, bride, [groom])) === "брак (19 лет). жених — «Фёдор Иванов»");
check("поручитель: сторона и кто венчается", text(ev(2, w1, [groom, bride])) === "поручитель по жениху. жених — «Фёдор Иванов»; невеста — «Дарья Петрова»");
check("родственник жениха: родство и кто венчается", text(ev(2, grel, [groom, bride])) === "отец жениха. жених — «Фёдор Иванов»; невеста — «Дарья Петрова»");
const dead = m("deceased", "Иван Капитонов", { gender: "М", age: "60", death_cause: "чахотка" });
const drel = m("deceased_relative", "Анна Петрова", { kinship: "жена", gender: "Ж" });
check("умерший: возраст, причина, родственник", text(ev(3, dead, [drel])) === "смерть (60 лет, причина: чахотка). жена — «Анна Петрова»", text(ev(3, dead, [drel])));
check("умерший без возраста и причины", describe(ev(3, m("deceased", "Иван"), [])).what === "смерть");
check("родственник умершего: кто умер", text(ev(3, drel, [dead])) === "жена умершего. умерший — «Иван Капитонов»");
check("родственник умершей — по полу умершей", describe(ev(3, m("deceased_relative", "Иван", { kinship: "отец" }), [m("deceased", "Мария", { gender: "Ж" })])).what === "отец умершей");
check("причт: звание, раздел, о ком запись", text(ev(1, m("clergy1", "Александр Рождественский", { rank: "священник" }), [girl, father])) === "священник: рождение. ребёнок — «Мария»; отец — «Иван Капитонов»");

check("родство не записано — слово по роли: родитель, супруг", text(ev(3, dead, [m("deceased_parent", "Капитон Иванов"), m("deceased_spouse", "Анна Петрова")])) === "смерть (60 лет, причина: чахотка). родитель — «Капитон Иванов», супруг — «Анна Петрова»",
  text(ev(3, dead, [m("deceased_parent", "Капитон Иванов"), m("deceased_spouse", "Анна Петрова")])));
check("возраст по частям от программы остаётся как есть", ageWords("1 г. 2 мес") === "1 г. 2 мес");
check("имени в книге нет — «***» и пустое узнаются", nameless(m("deceased", "***")) && nameless(m("deceased", "*** Иванова")) && nameless(m("deceased", " ")) && !nameless(m("deceased", "Иван")));

console.log("\nХвост человека и группы ролей\n");
check("чужой НП показывается, свой — нет", personTail(god, "Фетинино") === " (Малово)" && personTail(mother, "Фетинино") === "");
check("у жениха, невесты и умершего — возраст; у умершего — причина", personTail(bride, "Фетинино") === " (Воспица, 19 лет)" && personTail(dead, "") === " (60 лет, чахотка)");
check("у восприемника возраст не выдумывается", personTail(m("godparent1", "А", { age: "30" }), "") === "");
check("группы ролей — как у переключателей", roleKind("father") === "own" && roleKind("deceased") === "own" && roleKind("witness3") === "part"
  && roleKind("deceased_relative") === "part" && roleKind("clergy2") === "clergy" && roleKind("child") === "child");

console.log("\nБлок «Семья»\n");
const anna2 = m("mother", "Анна Петрова Капитонова", { iof_modern: "анна Петровна", gender: "Ж" });
const fam = family([
  ev(2, m("groom", "Иван Капитонов"), [m("bride", "Анна Петрова", { iof_modern: "Анна Петровна" })], { entry_id: 1, year: 1888 }),
  ev(1, father, [girl, m("mother", "Анна Петрова", { iof_modern: "Анна Петровна" }), god], { entry_id: 2, year: 1889 }),
  ev(1, father, [m("child", "Пётр", { gender: "М" }), anna2], { entry_id: 3, year: 1891 }),
  ev(1, father, [m("child", "Олимпиада", { gender: "Ж" }), m("mother", "Дарья Иванова", { iof_modern: "Дарья Ивановна" })], { entry_id: 4, year: 1895 }),
  ev(1, m("godparent1", "Иван Капитонов"), [girl, mother], { entry_id: 5, year: 1890 }),
  ev(1, father, [m("child", "Без матери")], { entry_id: 6, year: 1896 }),
]);
check("брак один, с невестой", fam.marriages.length === 1 && fam.marriages[0].spouse?.iof === "Анна Петрова");
check("дети — по жёнам: одна жена в двух записях — одна группа; вторая жена — своя; без матери — своя",
  fam.spouses.map((g) => `${g.spouse?.iof ?? "—"}:${g.children.map((c) => c.child?.iof).join(",")}`).join(" | ")
    === "Анна Петрова:Мария,Пётр | Дарья Иванова:Олимпиада | —:Без матери",
  fam.spouses.map((g) => `${g.spouse?.iof ?? "—"}:${g.children.map((c) => c.child?.iof).join(",")}`).join(" | "));
check("у ребёнка — его восприемники", fam.spouses[0].children[0].godparents.map((g) => g.iof).join() === "Пётр Сидоров Орлов");
check("чужое рождение, где он восприемник, в семью не идёт", fam.spouses.flatMap((g) => g.children).length === 4);
check("у того, кто не был ни супругом, ни родителем, семьи нет", family([ev(1, god, [girl, father])]).marriages.length === 0 && family([ev(1, god, [girl, father])]).spouses.length === 0);

console.log(`\nИтог: успешно ${ok}, ошибок ${bad}`);
process.exit(bad ? 1 : 0);
