/**
 * Досье персоны: из событий (записей, где стоит её ИОФ) — строки
 * человеческими словами и блок «Семья».
 *
 * Роман 09.10.2026: «я бы хотел видеть результаты не просто как сухую таблицу
 * строк, а как некое хронологическое „досье“ человека, где видно развитие его
 * жизни (родился, женился, родил детей, был свидетелем)»; в строке «обязательно
 * должны быть видны „запись о ком“ (имена родителей, супругов, детей), возраст
 * (жениха/невесты/умершего), а также, желательно, причина смерти».
 *
 * Имена в строках стоят в именительном падеже, после тире: склонять «Иоаннъ
 * Капитоновъ» программа не умеет, а «восприемник у Петра» с ошибкой в падеже
 * хуже, чем «ребёнок — Пётр».
 *
 * Функции без зависимостей — их проверяет scripts/test_dossier.mjs. Данные
 * собирает search.rs; здесь только слова.
 */

/** Человек в записи — сама персона или тот, кто рядом (search.rs, Mention). */
export type Mention = {
  /** Ключ персоны — с ним и с НП открывают досье именно этого человека. */
  key: string;
  role_code: string;
  iof: string;
  iof_modern: string;
  place: string;
  rank: string | null;
  gender: string | null;
  age: string | null;
  death_cause: string | null;
  marriage_order: string | null;
  kinship: string | null;
  note: string | null;
};

export type DossierEvent = {
  entry_id: number;
  section: number;
  year: number | null;
  day: number | null;
  month: number | null;
  page: string | null;
  note: string | null;
  /** Файл скана разворота этой записи (search.rs, Event). */
  scan_file?: string | null;
  /** Год книги записи (её дело): папка сканов — у дела. */
  book_year?: number | null;
  me: Mention;
  others: Mention[];
};

/** Кусок строки: текст или человек (на экране — ссылка на его досье). */
export type Part = string | { person: Mention };

const MONTHS = ["января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа",
                "сентября", "октября", "ноября", "декабря"];
const MONTH_NAMES = ["январь", "февраль", "март", "апрель", "май", "июнь", "июль", "август",
                     "сентябрь", "октябрь", "ноябрь", "декабрь"];

/** «12 марта»; дня нет — «март»; месяца нет — пусто. */
export function dateOf(e: { day: number | null; month: number | null }): string {
  if (e.month === null || e.month < 1 || e.month > 12) return e.day !== null ? `${e.day}-го` : "";
  return e.day !== null ? `${e.day} ${MONTHS[e.month - 1]}` : MONTH_NAMES[e.month - 1];
}

/** Какая это группа ролей — то же деление, что у переключателей экрана. */
export function roleKind(role: string): "own" | "part" | "clergy" | "child" {
  if (role === "child") return "child";
  if (["groom", "bride", "father", "mother", "deceased"].includes(role)) return "own";
  return role.startsWith("clergy") ? "clergy" : "part";
}

const of = (e: DossierEvent, ...roles: string[]) => e.others.filter((m) => roles.some((r) => m.role_code === r || (r.endsWith("*") && m.role_code.startsWith(r.slice(0, -1)))));
const one = (e: DossierEvent, ...roles: string[]) => of(e, ...roles)[0] as Mention | undefined;
const female = (m: Mention | undefined) => m?.gender === "Ж";

/** Возраст словами: «60 лет», «3 мес». Число без единицы — годы. */
export function ageWords(age: string | null): string {
  const a = (age ?? "").trim();
  if (!a) return "";
  if (!/^\d+$/.test(a)) return a;
  const n = Number(a), d = n % 10, h = n % 100;
  return `${n} ${d === 1 && h !== 11 ? "год" : d >= 2 && d <= 4 && (h < 12 || h > 14) ? "года" : "лет"}`;
}

/** Список людей через запятую, с подписью перед каждым, если она есть. */
function people(list: Mention[], label?: (m: Mention) => string): Part[] {
  const out: Part[] = [];
  list.forEach((m, i) => {
    if (i > 0) out.push(", ");
    const l = label?.(m);
    if (l) out.push(`${l} — `);
    out.push({ person: m });
  });
  return out;
}

/** «подпись — человек» или ничего, если человека в записи нет. */
function named(label: string, m: Mention | undefined): Part[] {
  return m ? [`${label} — `, { person: m }] : [];
}

/** Склеить непустые куски через «; ». */
function join(...chunks: Part[][]): Part[] {
  const out: Part[] = [];
  for (const c of chunks.filter((x) => x.length > 0)) {
    if (out.length) out.push("; ");
    out.push(...c);
  }
  return out;
}

/** Родство словами; не записано — по роли: «родитель», «супруг», иначе «родственник». */
const kinWord = (m: Mention) => (m.kinship ?? "").trim()
  || (m.role_code.endsWith("_parent") ? "родитель" : m.role_code.endsWith("_spouse") ? "супруг" : "родственник");
const relWord = (m: Mention, whose: string) => `${kinWord(m)} ${whose}`;

/** Имени в книге нет («***») — в досье так и сказано, ссылки на такого нет. */
export const nameless = (m: Mention) => !m.iof.trim() || m.iof.trim().startsWith("***");

/** Кто венчается и кто умер — главные лица чужой записи. */
function couple(e: DossierEvent): Part[] {
  return join(named("жених", one(e, "groom")), named("невеста", one(e, "bride")));
}

/**
 * Строка события: `what` — что произошло и кем в нём была персона, `who` —
 * кто стоит рядом в записи.
 */
export function describe(e: DossierEvent): { what: string; who: Part[] } {
  const me = e.me, role = me.role_code;
  const child = one(e, "child");
  const childWord = !child ? "ребёнка" : child.gender === "Ж" ? "дочери" : child.gender === "М" ? "сына" : "ребёнка";

  if (role === "father" || role === "mother") {
    const other = one(e, role === "father" ? "mother" : "father");
    return {
      what: `рождение ${childWord}`,
      who: join(child ? [{ person: child }] : [], named(role === "father" ? "мать" : "отец", other),
                of(e, "godparent*").length ? ["восприемники: ", ...people(of(e, "godparent*"))] : []),
    };
  }
  if (role.startsWith("godparent")) {
    return {
      what: female(me) ? "восприемница" : "восприемник",
      who: join(named("ребёнок", child), named("отец", one(e, "father")), named("мать", one(e, "mother"))),
    };
  }
  if (role === "groom" || role === "bride") {
    const spouse = one(e, role === "groom" ? "bride" : "groom");
    const mine = [ageWords(me.age), (me.marriage_order ?? "").toLowerCase()].filter(Boolean).join(", ");
    const sides = (side: string) => of(e, "witness*").filter((w) => (w.note ?? "").includes(side));
    const unsided = of(e, "witness*").filter((w) => !/жених|невест/.test(w.note ?? ""));
    return {
      what: "брак" + (mine ? ` (${mine})` : ""),
      who: join(named(role === "groom" ? "невеста" : "жених", spouse),
                people(of(e, "groom_relative"), (m) => relWord(m, "жениха")),
                people(of(e, "bride_relative", "bride_parent"), (m) => relWord(m, "невесты")),
                sides("жених").length ? ["поручители по жениху: ", ...people(sides("жених"))] : [],
                sides("невест").length ? ["поручители по невесте: ", ...people(sides("невест"))] : [],
                unsided.length ? ["поручители: ", ...people(unsided)] : []),
    };
  }
  if (role.startsWith("witness")) {
    return { what: ["поручитель", (me.note ?? "").trim()].filter(Boolean).join(" "), who: couple(e) };
  }
  if (role === "groom_relative" || role === "bride_relative" || role === "bride_parent") {
    return { what: relWord(me, role === "groom_relative" ? "жениха" : "невесты"), who: couple(e) };
  }
  if (role === "deceased") {
    const tail = [ageWords(me.age), me.death_cause ? `причина: ${me.death_cause}` : ""].filter(Boolean).join(", ");
    return {
      what: "смерть" + (tail ? ` (${tail})` : ""),
      who: people(of(e, "deceased_relative", "deceased_parent", "deceased_spouse"), kinWord),
    };
  }
  if (role.startsWith("deceased_")) {
    return { what: relWord(me, female(one(e, "deceased")) ? "умершей" : "умершего"), who: named(female(one(e, "deceased")) ? "умершая" : "умерший", one(e, "deceased")) };
  }
  // Причт и роли, которых мы не знаем по имени: раздел и главные лица записи.
  const main = e.section === 1 ? join(named("ребёнок", child), named("отец", one(e, "father")))
    : e.section === 2 ? couple(e) : named("умерший", one(e, "deceased"));
  return { what: `${(me.rank ?? "").trim() || "упомянут"}: ${["", "рождение", "брак", "смерть"][e.section] ?? "запись"}`, who: main };
}

/** Что дописать к человеку в строке: НП, возраст, причина смерти. */
export function personTail(m: Mention, here: string): string {
  const bits = [m.place && m.place !== here ? m.place : "", m.role_code === "groom" || m.role_code === "bride" || m.role_code === "deceased" ? ageWords(m.age) : "",
                m.role_code === "deceased" && m.death_cause ? m.death_cause : ""].filter(Boolean);
  return bits.length ? ` (${bits.join(", ")})` : "";
}

/**
 * Брак персоны в блоке «Семья»: кем она в него вступала и кто рядом в записи.
 * До 10.10.2026 там стояли только год и имя супруга — звание, «каким браком»
 * и родственников приходилось искать ниже, в строке события.
 *
 * `mine` — возраст, «каким браком» и звание самой персоны на день свадьбы;
 * `spouseNote` — звание и «каким браком» супруга (пункт и возраст дописывает
 * personTail); `relatives` — родственники жениха и невесты с родством;
 * `missing` — что сказать, когда супруга в записи нет.
 */
export function marriageLine(e: DossierEvent): { mine: string; spouseNote: string; relatives: Part[]; missing: string } {
  const groomSide = e.me.role_code === "groom";
  const spouse = one(e, groomSide ? "bride" : "groom");
  const words = (m: Mention | undefined, withAge: boolean) =>
    [withAge ? ageWords(m?.age ?? null) : "", (m?.marriage_order ?? "").trim().toLowerCase(), (m?.rank ?? "").trim()].filter(Boolean).join(", ");
  return {
    mine: words(e.me, true),
    spouseNote: words(spouse, false),
    relatives: join(people(of(e, "groom_relative"), (m) => relWord(m, "жениха")),
                    people(of(e, "bride_relative", "bride_parent"), (m) => relWord(m, "невесты"))),
    missing: groomSide ? "невеста не записана" : "жених не записан",
  };
}

/** Блок «Семья»: браки персоны и её дети — по супругам. */
export type Family = {
  marriages: { event: DossierEvent; spouse: Mention | undefined }[];
  spouses: { spouse: Mention | undefined; children: { event: DossierEvent; child: Mention | undefined; godparents: Mention[] }[] }[];
};

export function family(events: DossierEvent[]): Family {
  const marriages = events
    .filter((e) => e.me.role_code === "groom" || e.me.role_code === "bride")
    .map((e) => ({ event: e, spouse: one(e, e.me.role_code === "groom" ? "bride" : "groom") }));
  const spouses: Family["spouses"] = [];
  for (const e of events.filter((x) => x.me.role_code === "father" || x.me.role_code === "mother")) {
    const spouse = one(e, e.me.role_code === "father" ? "mother" : "father");
    // Одна и та же жена в разных записях — по современному ИОФ; регистр и «ё» не важны.
    const key = (m: Mention | undefined) => (m ? m.iof_modern.toLowerCase().replace(/ё/g, "е") : "");
    let group = spouses.find((g) => key(g.spouse) === key(spouse));
    if (!group) spouses.push(group = { spouse, children: [] });
    group.children.push({ event: e, child: one(e, "child"), godparents: of(e, "godparent*") });
  }
  return { marriages, spouses };
}
