import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { type Parsed, type PersonHint } from "./IofField";
import PersonBlock, { EMPTY_PERSON, usePlaceRenamed, type Person } from "./PersonBlock";
import NumberField from "./NumberField";
import PageField from "./PageField";
import ClergyBlock from "./ClergyBlock";
import Suggest from "./Suggest";
import { useFormClergy } from "./clergy";
import { eventYearOf, riteBeforeEvent, splitCount, type Sex } from "./count";
import { parseAge } from "./age";
import { focusNextField } from "./focus";
import NextYear from "./NextYear";
import { dismissWarn, report, warn } from "./errors";
import { setDirty } from "./dirty";
import { titleCase } from "./names";
import type { FormProps } from "./formprops";

/**
 * Форма ввода записи о смерти (27.09.2026) — по образцу рождений и браков.
 *
 * СОСТАВ ВЗЯТ ИЗ ЛИСТА «3» EXCEL РОМАНА (824 записи):
 *
 *   Стр., Счёт умерших (мужеска / женска пола — раздельно, как у рождений),
 *   дата смерти и дата погребения (погребение есть у всех 824);
 *   умерший — НП, Звание, ИОФ, Прим. = от чего умер, Лет;
 *   родитель или супруг — НП, Звание, ИОФ (565 записей, чаще отец младенца);
 *   церковнослужители 1–3 (общие для всех разделов).
 *
 * Вероисповедания у умерших в его данных нет ни в одной записи — поля нет.
 *
 * ПРИЧИНА СМЕРТИ в Excel стоит в «Прим.»; здесь — своё поле с подсказками
 * (перечень death_cause, 76 значений в поставке, пополняется при сохранении):
 * в шаблоне Familio это отдельная колонка.
 *
 * ВОЗРАСТ набирается как в книге: «5», «3 мес», «2 нед», «1,5 мес» —
 * см. age.ts. Счёт после сохранения не растёт: он раздельный по полу,
 * угадать следующий нельзя (как в рождениях). Возраст стоит над причиной —
 * так идёт запись в книге (Роман 30.09.2026), в Excel было наоборот.
 *
 * ЛИЧНОСТЬ НЕ УСТАНОВЛЕНА (Роман 30.09.2026): «в метрических книгах регулярно
 * встречаются записи о найденных телах без имени», а сверка ИОФ не давала
 * сохранить запись. Флажок в заголовке «Умерший» убирает поле ИОФ; пол
 * тогда выбирается кнопками, «тело неизвестного человека мужеского пола»
 * пишется в звание. Отдельно не хранится: умерший без имени и есть такая
 * запись — при «Открыть» флажок восстанавливается по пустому ИОФ.
 */

const KIN_DEFAULT = "отец";

type Deceased = Person & { cause: string; age: string };
type Relative = Person & { kinship: string };

const NEW_DECEASED: Deceased = { ...EMPTY_PERSON, cause: "", age: "" };
const NEW_RELATIVE: Relative = { ...EMPTY_PERSON, kinship: KIN_DEFAULT };

type Brief = {
  id: number; page: string | null; no_male: number | null; no_female: number | null;
  event_day: number | null; event_month: number | null; event_year: number | null;
  rite_month: number | null; rite_year: number | null; deceased: string | null; clergy_noname: boolean;
};

type MentionOut = {
  role_code: string; surname: string | null; first_name: string | null; patronymic: string | null;
  gender: string | null; rank: string | null; confession: string | null; place: string | null;
  note: string | null; kinship: string | null; age_text: string | null; death_cause: string | null;
};
type EntryFull = {
  id: number; page: string | null; no_male: number | null; no_female: number | null;
  event_day: number | null; event_month: number | null; event_year: number | null;
  rite_day: number | null; rite_month: number | null; rite_year: number | null;
  note: string | null; persons: MentionOut[];
};

/** Пол родственника по родству: «отец» — М, «мать» — Ж, иначе по имени. */
function kinGender(k: string): "М" | "Ж" | undefined {
  const v = k.trim().toLowerCase();
  if (["отец", "брат", "дядя", "дед", "супруг", "муж", "сын"].includes(v)) return "М";
  if (["мать", "сестра", "тетка", "тётка", "бабка", "супруга", "жена", "дочь"].includes(v)) return "Ж";
  return undefined;
}

export default function DeathForm({ mkCase, onSaved, workYear, openReq }: FormProps) {
  const [page, setPage] = useState<string | null>(null);
  const [year, setYear] = useState<number | null>(mkCase.year ?? null);
  const [count, setCount] = useState<number | null>(null);
  const [deathDay, setDeathDay] = useState<number | null>(null);
  const [deathMonth, setDeathMonth] = useState<number | null>(null);
  const [burialDay, setBurialDay] = useState<number | null>(null);
  const [burialMonth, setBurialMonth] = useState<number | null>(null);
  // Погребение в следующем году — отмечает человек (NextYear.tsx).
  const [burialNextYear, setBurialNextYear] = useState(false);
  /** Месяц смерти подставляется в месяц погребения — в одну сторону
   *  (Роман 28.09.2026: «в большинстве записей месяцы совпадают»). */
  function changeDeathMonth(v: number | null) {
    setDeathMonth(v);
    // Исправленный руками месяц погребения не затирается (ревьюер #38).
    setBurialMonth((r) => (r === null || r === deathMonth ? v : r));
  }
  useEffect(() => {
    if (!riteBeforeEvent(deathMonth, burialMonth)) setBurialNextYear(false);
  }, [deathMonth, burialMonth]);

  const [dead, setDead] = useState<Deceased>(NEW_DECEASED);
  const [nameless, setNameless] = useState(false);
  const [rel, setRel] = useState<Relative>(NEW_RELATIVE);
  // Пол умершего руками — только когда по имени его не понять. От него
  // зависит колонка счёта, как у рождений.
  const [sexManual, setSexManual] = useState<Sex | null>(null);
  const [askSex, setAskSex] = useState(false);

  const clergyState = useFormClergy();
  const [clergy1, clergy2, clergy3] = clergyState.people;

  const [editingId, setEditingId] = useState<number | null>(null);
  /** Год, сохранённый на «Деле», пока запись была открыта на правку: после
   *  правки форма встаёт на него, а не на год до правки. */
  const yearWhileEditing = useRef<number | null>(null);
  const beforeEdit = useRef<{ page: string | null; count: number | null; year: number | null;
                               deathMonth: number | null; burialMonth: number | null } | null>(null);
  const [saved, setSaved] = useState<Brief[]>([]);
  const [busy, setBusy] = useState(false);
  const countField = useRef<HTMLInputElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const placeDefaults = { guberniya: mkCase.guberniya ?? "", uyezd: mkCase.uyezd ?? "" };

  // Список «Набрано» — записи года книги, который стоит на форме: после
  // импорта из Excel в приходе тысячи записей (спека 2026-10-02, п. 3.4).
  useEffect(() => { refresh(); }, [year]);

  // Восстановление места работы — один раз при открытии формы, по последней
  // записи раздела в приходе, какого бы года она ни была.
  useEffect(() => {
    invoke<Brief[]>("entry_list", { section: 3, year: null, last: true })
      .then((rows) => { if (rows[0]) resume(rows[0]); })
      .catch((e) => report("Не удалось узнать, на чём остановились", e));
  }, []);

  const listSeq = useRef(0);
  function refresh() {
    // Год набирают по цифре — ответ на прежний год отбрасывается.
    const mine = ++listSeq.current;
    invoke<Brief[]>("entry_list", { section: 3, year, last: false })
      .then((rows) => { if (mine === listSeq.current) setSaved(rows); })
      .catch((e) => report("Не удалось прочитать список набранных смертей", e));
  }

  /** Продолжить с места: страница, год, счёт, месяцы. Причт — общий. */
  function resume(last: Brief) {
    setPage((v) => v ?? last.page);
    if ((last.rite_year ?? last.event_year) !== null) setYear(last.rite_year ?? last.event_year);
    setCount((v) => v ?? last.no_male ?? last.no_female ?? null);
    setDeathMonth((v) => v ?? last.event_month);
    setBurialMonth((v) => v ?? last.rite_month);
  }

  function renamePlace(oldName: string, newName: string) {
    const fix = <T extends Person>(p: T): T => (p.place === oldName ? { ...p, place: newName } : p);
    setDead(fix); setRel(fix);
  }
  usePlaceRenamed(renamePlace);

  function pickInto<T extends Person>(set: (fn: (p: T) => T) => void) {
    return (hint: PersonHint) =>
      set((p) => ({ ...p, place: hint.place ?? p.place, rank: hint.rank ?? p.rank }));
  }

  /**
   * Выбрали умершего из подсказки: если это ребёнок из записи о рождении
   * этого прихода, родственник — его отец из той записи (Роман 28.09.2026).
   *
   * Осторожно, чтобы не записать чужого отца (проверяющий и ревьюер #38):
   * - только в пустого родственника (ИОФ, НП, звание пусты, родство «отец»
   *   или пусто) или в того, кого подставили сами прошлым выбором;
   * - если в приходе несколько рождений с таким именем — не угадываем,
   *   говорим выбрать самому;
   * - ответ, пришедший после смены записи или другого выбора, отбрасывается.
   */
  const pickSeq = useRef(0);
  const autoRel = useRef<string | null>(null);
  const autoPlace = useRef<string | null>(null);
  const autoRank = useRef<string | null>(null);
  /** НП умершего, набранный человеком (не подставленный программой). */
  const placeTyped = autoPlace.current !== null && dead.place === autoPlace.current ? null : dead.place;
  function pickDeceased(hint: PersonHint) {
    pickInto(setDead)(hint);
    const mine = ++pickSeq.current;
    // Выбран конкретный ребёнок (строка «младенец» с родителем) — гадать не
    // нужно: родитель известен из его записи о рождении (Роман 30.09.2026).
    if (hint.infant) {
      const k = hint.infant;
      const wasAuto = autoRel.current;
      const wasPlace = autoPlace.current;
      // Младенец живёт у родителя — его НП. Набранный руками НП не трогается;
      // подставленный прошлым выбором — заменяется (как у родственника).
      // «Подставлено нами» запоминается, только если подстановка была: иначе
      // совпавшее по тексту набранное руками затёрлось бы следующим выбором
      // (ревьюер, 01.10.2026).
      const placeOurs = !dead.place.trim() || (wasPlace !== null && dead.place === wasPlace);
      if (placeOurs) setDead((d) => ({ ...d, place: k.place ?? "" }));
      autoPlace.current = placeOurs ? k.place ?? null : null;
      // Звание младенца — по полу ребёнка из записи о рождении (ошибка из
      // ответа Романа 03.10.2026: «поле „Звание“ … остается пустым»). В его
      // Excel у детей почти всегда так: «сын младенец» 268, «дочь младенец»
      // 253. Набранное руками не трогаем; подставленное прошлым выбором — меняем.
      const babyRank = hint.gender === "М" ? "сын младенец" : hint.gender === "Ж" ? "дочь младенец" : null;
      const rankOurs = !dead.rank.trim() || (autoRank.current !== null && dead.rank === autoRank.current);
      if (rankOurs && babyRank) {
        setDead((d) => ({ ...d, rank: babyRank }));
        autoRank.current = babyRank;
      } else if (!rankOurs) {
        autoRank.current = null;
      }
      const relOurs = (rel.iof.trim() === "" && !rel.place.trim() && !rel.rank.trim()
          && (!rel.kinship.trim() || rel.kinship.trim() === KIN_DEFAULT))
        || (wasAuto !== null && rel.iof === wasAuto);
      if (!relOurs) {
        autoRel.current = null;
        return;
      }
      if (!k.parent) {
        // Родителя в записи о рождении нет — подставленного раньше убираем.
        if (wasAuto !== null) setRel({ ...NEW_RELATIVE });
        autoRel.current = null;
        return;
      }
      setRel({
        ...NEW_RELATIVE, kinship: k.kin ?? KIN_DEFAULT, iof: k.parent, parsed: null,
        place: k.place ?? "", rank: k.rank ?? "",
      });
      autoRel.current = k.parent;
      return;
    }
    // Выбрали не младенца — НП, подставленный от родителя прошлого младенца,
    // этому человеку не принадлежит.
    const stalePlace = autoPlace.current;
    if (stalePlace !== null && !hint.place)
      setDead((d) => (d.place === stalePlace ? { ...d, place: "" } : d));
    // То же со званием «сын/дочь младенец» от прошлого выбора (проверяющий 03.10.2026).
    const staleRank = autoRank.current;
    if (staleRank !== null && !hint.rank)
      setDead((d) => (d.rank === staleRank ? { ...d, rank: "" } : d));
    autoPlace.current = null;
    autoRank.current = null;
    invoke<{ iof: string; place: string | null; rank: string | null; births: number } | null>(
      "birth_father", { iof: hint.iof, year, place: hint.place ? null : placeTyped?.trim() || null })
      .then((f) => {
        if (mine !== pickSeq.current) return;
        const wasAuto = autoRel.current;
        const replaceable = (r: Relative) =>
          (r.iof.trim() === "" && !r.place.trim() && !r.rank.trim()
            && (!r.kinship.trim() || r.kinship.trim() === KIN_DEFAULT))
          || (wasAuto !== null && r.iof === wasAuto);
        if (!f || f.births > 1) {
          // Отца не знаем — подставленного прошлым выбором убираем.
          if (wasAuto !== null)
            setRel((r) => (r.iof === wasAuto ? { ...NEW_RELATIVE } : r));
          autoRel.current = null;
          if (f && f.births > 1)
            warn(`В приходе ${f.births} записи о рождении «${hint.iof}»`,
                 "отец не подставлен — выберите его сами, чтобы не записать чужого");
          return;
        }
        setRel((r) => (replaceable(r) ? {
          ...NEW_RELATIVE, kinship: KIN_DEFAULT, iof: f.iof, parsed: null,
          place: f.place ?? "", rank: f.rank ?? "",
        } : r));
        autoRel.current = f.iof;
      })
      .catch((e) => report("Не удалось найти отца по записи о рождении", e));
  }

  async function withParsed<T extends Person>(p: T): Promise<T> {
    // Ctrl+Enter прямо из поля ИОФ: до заглавных букв (уход из поля) дело
    // не дошло — ставим их здесь, иначе в базу легло бы «иван петров».
    const iof = titleCase(p.iof);
    if (iof !== p.iof) return { ...p, iof, parsed: await invoke<Parsed>("parse_iof", { text: iof }) };
    if (!p.iof.trim() || p.parsed) return p;
    return { ...p, parsed: await invoke<Parsed>("parse_iof", { text: p.iof }) };
  }

  function payload(role: string, order: number, p: Person, extra: Record<string, unknown> = {},
                   gender?: string | null) {
    return {
      role_code: role, sort_order: order,
      surname: p.parsed?.surname ?? null, first_name: p.parsed?.first_name ?? null,
      patronymic: p.parsed?.patronymic ?? null, surname_modern: null,
      first_name_modern: p.parsed?.first_name_modern ?? null,
      patronymic_modern: p.parsed?.patronymic_modern ?? null, maiden_surname: null,
      gender: gender ?? p.parsed?.gender ?? null, rank: p.rank || null,
      confession: p.confession || null, place: p.place || null, note: p.note || null,
      uncertain: null, ...extra,
    };
  }

  async function save() {
    let d = dead, r = rel, cl = [clergy1, clergy2, clergy3];
    try {
      [d, r] = await Promise.all([withParsed(d), withParsed(r)]) as [Deceased, Relative];
      cl = await Promise.all(cl.map(withParsed));
    } catch (e) {
      report("Не удалось разобрать имена перед сохранением", e);
      return;
    }
    const named: [string, Person][] = [
      ["умершего", d], ["родственника", r], ...cl.map((c) => ["причта", c] as [string, Person]),
    ];
    const unknown = named.find(([, p]) => p.iof.trim() && p.parsed && !p.parsed.known_name);
    if (unknown) {
      warn(`Имя ${unknown[0]} «${unknown[1].parsed?.first_name}» не сверено со справочником`,
           unknown[0] === "причта"
             ? "нажмите «Изменить» у причта и выйдите из поля ИОФ — откроется окно сверки"
             : "выйдите из поля ИОФ — откроется окно сверки и предложит имя из справочника");
      return;
    }
    if (!d.iof.trim() && !nameless) {
      warn("Запись пустая", "не заполнено имя умершего; если имени нет в книге — отметьте «личность не установлена»");
      return;
    }
    if (year === null) {
      warn("Не указан год", "год записи стоит в первой строке формы — заполните его один раз, дальше он держится сам");
      root.current?.querySelector<HTMLInputElement>(".row.tight input")?.focus();
      return;
    }
    const sex: Sex | null = (d.parsed?.gender as Sex | null | undefined) ?? sexManual;
    const columns = splitCount(count, sex);
    if (columns === null) {
      const buttons = root.current?.querySelector<HTMLButtonElement>(".sexpick button");
      if (buttons) {
        setAskSex(true);
        buttons.focus();
      } else {
        // Кнопок нет, пока имя не разобрано (сразу после «Открыть») — сказать.
        warn("Не понять пол умершего", "выйдите из поля ИОФ умершего — появится выбор «мужской / женский»; от пола зависит колонка счёта");
      }
      return;
    }
    const age = parseAge(d.age);
    const persons: ReturnType<typeof payload>[] = [
      payload("deceased", 10, d, {
        age_text: d.age.trim() || null,
        age_years: age?.years ?? null, age_months: age?.months ?? null,
        age_weeks: age?.weeks ?? null, age_days: age?.days ?? null,
        death_cause: d.cause.trim() || null,
      }, sex),
    ];
    // Родственник пишется, только если есть имя: в Excel у 259 из 824 его нет.
    if (r.iof.trim())
      persons.push(payload("deceased_relative", 40, r, { kinship: r.kinship.trim() || null }, kinGender(r.kinship)));
    cl.forEach((c, i) => { if (c.iof.trim()) persons.push(payload(`clergy${i + 1}`, 100 + i * 10, c, {}, "М")); });

    setBusy(true);
    try {
      const done = await invoke<{ id: number; case_id: number; new_case_year: number | null;
                                  fallback_case: boolean; fallback_year: number | null }>("entry_save", {
        entry: {
          id: editingId, case_id: mkCase.id, section: 3, page,
          no_male: columns.no_male, no_female: columns.no_female,
          event_day: deathDay, event_month: deathMonth,
          event_year: eventYearOf(year, deathMonth, burialMonth, burialNextYear),
          rite_day: burialDay, rite_month: burialMonth,
          rite_year: year,
          note: null, uncertain: null, persons,
        },
      });
      clergyState.bump();
      dismissWarn();
      // Дело — на год книги: год встретился впервые — дело заведено копией
      // прошлого, реквизиты надо проверить (спека 2026-10-02, п. 3.2).
      if (done.new_case_year !== null)
        warn(`Новый год книги ${done.new_case_year}`,
             "ему заведено своё дело копией прошлого — проверьте фонд, опись и дело на экране «Дело»");
      // Запись без года, а её дело убрано: легла в первое дело прихода — не молча.
      if (done.fallback_case)
        warn("Запись без года привязана к другому делу",
             `дела, с которым работала форма, больше нет — запись легла в дело ${
               done.fallback_year !== null ? `${done.fallback_year} года` : "без года"}; поставьте ей год, и она перейдёт в своё`);
      onSaved(done.case_id);
      if (editingId !== null) restoreAfterEdit();
      else next();
      refresh();
    } catch (e) {
      report(editingId ? "Не удалось сохранить изменения" : "Не удалось сохранить запись о смерти", e);
    } finally {
      setBusy(false);
    }
  }

  // Экран «Дело» сохранён с годом — форма встаёт на него; запись, открытую
  // на правку, это не трогает. «Открыть запись» из списка на сверку — сюда же.
  useEffect(() => {
    if (!workYear) return;
    // Запись открыта на правку — год у неё свой; новый год дела форма
    // подхватит, когда правка закончится (restoreAfterEdit).
    if (editingId === null) setYear(workYear.year);
    else yearWhileEditing.current = workYear.year;
  }, [workYear?.n]);
  useEffect(() => {
    if (openReq && openReq.section === 3) void openEntry(openReq.id);
  }, [openReq?.n]);

  // Несохранённое — для смены прихода: окно «Приходы» не даст потерять молча.
  useEffect(() => {
    setDirty("Смерти", formDirty() || editingId !== null);
    return () => setDirty("Смерти", false);
  });

  function formDirty(): boolean {
    return [dead, rel].some((p) => p.iof.trim() || p.place.trim() || p.rank.trim())
      || dead.cause.trim().length > 0 || dead.age.trim().length > 0 || nameless;
  }

  async function openEntry(id: number) {
    if (editingId !== null && editingId !== id) {
      warn("Сначала сохраните изменения или нажмите «Отменить»", "открыта другая запись — её правки иначе пропадут");
      return;
    }
    if (editingId === null && formDirty()) {
      warn("Сначала сохраните или очистите набранное",
           "открыть запись для правки можно только с пустой формы — иначе набранное пропадёт");
      return;
    }
    try {
      const e = await invoke<EntryFull>("entry_load", { id });
      pickSeq.current++;
      autoRel.current = null;
      autoPlace.current = null;
    autoRank.current = null;
      if (editingId === null)
        beforeEdit.current = { page, count, year, deathMonth, burialMonth };
      const iof = (m: MentionOut) => [m.first_name, m.patronymic, m.surname].filter(Boolean).join(" ");
      const person = (m: MentionOut | undefined, base: Person): Person =>
        m ? { ...base, iof: iof(m), parsed: null, place: m.place ?? "", rank: m.rank ?? "",
              confession: m.confession ?? "", note: m.note ?? "" } : { ...base, note: "" };
      const by = (role: string) => e.persons.find((m) => m.role_code === role);
      const dm = by("deceased");
      const rm = by("deceased_relative");
      setPage(e.page);
      setCount(e.no_male ?? e.no_female ?? null);
      setDeathDay(e.event_day); setDeathMonth(e.event_month);
      setBurialDay(e.rite_day); setBurialMonth(e.rite_month);
      setBurialNextYear(e.rite_year !== null && e.event_year !== null && e.event_year < e.rite_year);
      // Год формы — год книги, то есть год погребения.
      if ((e.rite_year ?? e.event_year) !== null) setYear(e.rite_year ?? e.event_year);
      setDead({ ...person(dm, NEW_DECEASED), cause: dm?.death_cause ?? "", age: dm?.age_text ?? "" });
      setNameless(!!dm && !iof(dm));
      setSexManual((dm?.gender as Sex | null | undefined) ?? null);
      setAskSex(false);
      setRel({ ...person(rm, NEW_RELATIVE), kinship: rm ? rm.kinship ?? "" : KIN_DEFAULT });
      clergyState.open([person(by("clergy1"), EMPTY_PERSON), person(by("clergy2"), EMPTY_PERSON),
                        person(by("clergy3"), EMPTY_PERSON)]);
      setEditingId(e.id);
      window.scrollTo({ top: 0 });
      countField.current?.focus();
    } catch (err) {
      report("Не удалось открыть запись о смерти", err);
    }
  }

  function restoreAfterEdit() {
    const b = beforeEdit.current;
    beforeEdit.current = null;
    setEditingId(null);
    next();
    if (b) {
      setPage(b.page); setCount(b.count); setYear(yearWhileEditing.current ?? b.year);
      setDeathMonth(b.deathMonth); setBurialMonth(b.burialMonth);
    }
    yearWhileEditing.current = null;
    clergyState.close();
  }

  /** Следующая запись: страница, год, счёт, месяцы и причт остаются. */
  function next() {
    pickSeq.current++;
    autoRel.current = null;
    autoPlace.current = null;
    autoRank.current = null;
    setBurialNextYear(false);
    setDead({ ...NEW_DECEASED });
    setNameless(false);
    setRel({ ...NEW_RELATIVE });
    setSexManual(null);
    setAskSex(false);
    setDeathDay(null);
    setBurialDay(null);
    countField.current?.focus();
    countField.current?.select();
  }

  function hotkeys(e: React.KeyboardEvent<HTMLDivElement>) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && !busy) {
      e.preventDefault();
      void save();
    }
  }

  const parsedSex = nameless ? undefined : dead.parsed?.gender as Sex | null | undefined;

  /** Флажок «личность не установлена»: ИОФ и подставленный по нему отец уходят. */
  function changeNameless(v: boolean) {
    setNameless(v);
    if (!v) return;
    pickSeq.current++;
    setDead((s) => ({ ...s, iof: "", parsed: null }));
    const wasAuto = autoRel.current;
    if (wasAuto !== null) setRel((r) => (r.iof === wasAuto ? { ...NEW_RELATIVE } : r));
    autoRel.current = null;
    autoPlace.current = null;
    autoRank.current = null;
  }

  const namelessBox = (
    <label className="unknownbox" title="Имени в книге нет — запись сохранится без ИОФ">
      {/* Вне обхода Tab: нужен редко, мышью (Роман 03.10.2026). */}
      <input type="checkbox" checked={nameless} tabIndex={-1}
             onChange={(e) => changeNameless(e.target.checked)}
             onKeyDown={(e) => {
               // Не data-field: иначе Enter заходил бы на флажок в каждой записи.
               // Enter с флажка — в первое поле блока (НП или ИОФ).
               if (e.key === "Enter") {
                 e.preventDefault();
                 e.currentTarget.closest("section")?.querySelector<HTMLInputElement>("input[data-field]")?.focus();
               }
             }} />
      личность не установлена
    </label>
  );
  const ageParsed = parseAge(dead.age);

  const deadExtra = (
    <>
      <div className="field">
        <label title="Как в книге: 5, 3 мес, 2 нед, 1,5 мес, 5 дней">Возраст</label>
        <div className="fieldbody">
          <input data-field className="age" value={dead.age} autoComplete="off" spellCheck={false}
                 title="Как в книге: 5 (лет), 3 мес, 2 нед, 1,5 мес, 5 дней"
                 onChange={(e) => setDead((s) => ({ ...s, age: e.target.value }))}
                 onKeyDown={(e) => {
                   if (e.key === "Enter" || e.key === "ArrowDown") {
                     e.preventDefault();
                     focusNextField(e.currentTarget, e.shiftKey && e.key === "Enter" ? -1 : 1);
                   } else if (e.key === "ArrowUp") {
                     e.preventDefault();
                     focusNextField(e.currentTarget, -1);
                   }
                 }} />
          {dead.age.trim() && !ageParsed && (
            <div className="fieldhint">не разобрано — сохранится как написано</div>
          )}
        </div>
      </div>
      <Suggest label="Причина" kind="death_cause" value={dead.cause} browse
               onChange={(cause) => setDead((s) => ({ ...s, cause }))} />
      {(nameless || (dead.iof.trim() && dead.parsed && !parsedSex)) && (
        <div className="field sexline">
          <label>Пол</label>
          <div className={"fieldbody sexpick" + (askSex ? " ask" : "")}>
            <button type="button" className={sexManual === "М" ? "on" : ""}
                    onClick={() => { setSexManual("М"); setAskSex(false); }}>мужской</button>
            <button type="button" className={sexManual === "Ж" ? "on" : ""}
                    onClick={() => { setSexManual("Ж"); setAskSex(false); }}>женский</button>
            <span className="fieldhint">
              {askSex
                ? "Не сохранено: выберите пол — от него зависит колонка счёта"
                : nameless ? "от пола зависит колонка счёта"
                : "имени нет в словаре — от пола зависит колонка счёта"}
            </span>
          </div>
        </div>
      )}
    </>
  );

  const common = { placeDefaults, onPlaceRenamed: renamePlace, rankKind: "rank" as const };

  return (
    <div onKeyDown={hotkeys} ref={root} className="death formroot">
      {editingId !== null && (
        <div className="editbar">
          <b>Правка записи</b> — сохранённая запись о смерти открыта в форме. «Сохранить
          изменения» перепишет её; «Отменить» оставит как была.
          <button type="button" className="toggle" onClick={restoreAfterEdit}>Отменить</button>
        </div>
      )}
      <section>
        <div className="row tight">
          <NumberField label="Год" value={year} onChange={setYear} min={1700} max={1930} />
          <PageField label="Стр." value={page} onChange={setPage} />
          <NumberField label="Счёт" value={count} onChange={setCount} min={1} inputRef={countField} />
        </div>
        <div className="row wrap">
          <NumberField label="Смерть, день" value={deathDay} onChange={setDeathDay} min={1} max={31} />
          <NumberField label="месяц" value={deathMonth} onChange={changeDeathMonth} min={1} max={12} />
          <NumberField label="Погреб., день" value={burialDay} onChange={setBurialDay} min={1} max={31} />
          <NumberField label="месяц" value={burialMonth} onChange={setBurialMonth} min={1} max={12} noTab />
        </div>
        <NextYear eventMonth={deathMonth} riteMonth={burialMonth} year={year} rite="погребение"
                  checked={burialNextYear} onChange={setBurialNextYear} />
      </section>

      <div className="cols">
        <PersonBlock title="Умерший" person={dead} onChange={(p) => setDead((s) => ({ ...s, ...p }))}
                     gender={parsedSex ?? sexManual ?? undefined}
                     onPickPerson={pickDeceased} preferInfant infantRows infantYear={year}
                     // НП, который программа сама подставила от родителя прошлого
                     // выбора, список не сужает: выбрали не ту «Евдокию» — нужная
                     // из другой деревни должна остаться в списке (ревьюер 03.10.2026).
                     infantPlace={placeTyped}
                     {...common} extra={deadExtra}
                     noIof={nameless} titleAfter={namelessBox} />
        <PersonBlock title="Родственник" person={rel}
                     onChange={(p) => setRel((s) => ({ ...s, ...p }))}
                     gender={kinGender(rel.kinship)} onPickPerson={pickInto(setRel)} {...common}
                     before={
                       <Suggest label="Родство" kind="kinship" value={rel.kinship} browse
                                onChange={(kinship) => setRel((s) => ({ ...s, kinship }))} />
                     } />
      </div>

      <ClergyBlock
        people={[clergy1, clergy2, clergy3]}
        onChange={(i, p) => clergyState.setAt(i, p)}
        reloadKey={clergyState.savedTimes}
      />

      <div className="savebar">
        <button className="primary" onClick={save} disabled={busy} title="Ctrl+Enter">
          {busy ? "Сохраняю…" : editingId !== null ? "Сохранить изменения" : "Сохранить и следующая"}
          <span className="kbd">Ctrl+Enter</span>
        </button>
      </div>

      {saved.length > 0 && (
        <section>
          <h2>Набрано смертей: {saved.length}{year !== null && <span className="ofyear"> — за {year} год</span>}</h2>
          <p className="hint">Нажмите «Открыть», чтобы поправить запись. Форма при этом должна быть пустой.</p>
          <table className="facts saved">
            <tbody>
              {saved.map((e) => (
                <tr key={e.id} className={e.id === editingId ? "editing" : ""}>
                  <td>
                    {e.no_male !== null ? `№ м. ${e.no_male}` : e.no_female !== null ? `№ ж. ${e.no_female}` : "без №"}
                    {" · "}{e.event_day ?? "?"}.{e.event_month ?? "?"} · {e.deceased || "личность не установлена"}
                  </td>
                  <td>стр. {e.page ?? "—"}</td>
                  <td>
                    <button type="button" className="linkish" onClick={() => void openEntry(e.id)}>
                      {e.clergy_noname ? "Открыть — причт без имени" : "Открыть"}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
    </div>
  );
}
