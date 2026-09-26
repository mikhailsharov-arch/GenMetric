import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { type Parsed, type PersonHint } from "./IofField";
import PersonBlock, { EMPTY_PERSON, usePlaceRenamed, type Person } from "./PersonBlock";
import NumberField from "./NumberField";
import PageField from "./PageField";
import ClergyBlock from "./ClergyBlock";
import Suggest from "./Suggest";
import { report } from "./errors";
import type { Case } from "./CaseHeader";

/**
 * Форма ввода записи о браке (25.09.2026) — по образцу рождений.
 *
 * СОСТАВ ВЗЯТ ИЗ ЛИСТА «2» EXCEL РОМАНА (180 его записей по Борисоглебскому):
 *
 *   Стр., Счёт бракосочетавшихся, Дата бракосочетания;
 *   жених и невеста — НП, Звание, ИОФ, Вероисповедания, «каким браком», Лет;
 *   родственник жениха и родственник невесты — родство, ИОФ, звание;
 *   поручители 1–4 — ИОФ, НП, Звание, Прим. («по жениху» / «по невесте»);
 *   церковнослужители 1–3.
 *
 * Порядок полей у персоны — как в рождениях (ИОФ первым): Роман принял его
 * 17.08.2026, потому что выбор персоны из памяти заполняет НП и звание разом.
 *
 * ЗАГОТОВКИ — по его данным: вероисповедание «православного» у 180 из 180;
 * «Первым браком» у жениха 151 и у невесты 168 из 180; родственник — «отец»
 * у всех 180, с именем лишь в 13–15; поручителей четверо в 174 записях,
 * первые двое «по жениху» (177), вторые двое «по невесте» (176).
 *
 * СЧЁТ растёт на единицу после сохранения: у браков он один сквозной на год
 * (1, 2, 3 … в его данных). В рождениях счёт не трогается — там он раздельный
 * по полу и угадать его нельзя.
 */

const CONFESSION = "православного";
const FIRST_MARRIAGE = "Первым браком";
const KIN_DEFAULT = "отец";

type Spouse = Person & { order: string; age: number | null };
type Relative = Person & { kinship: string };

const NEW_SPOUSE: Spouse = { ...EMPTY_PERSON, confession: CONFESSION, order: FIRST_MARRIAGE, age: null };
const NEW_RELATIVE: Relative = { ...EMPTY_PERSON, kinship: KIN_DEFAULT };
const SIDES = ["по жениху", "по невесте"] as const;
const WITNESS_SIDE = [SIDES[0], SIDES[0], SIDES[1], SIDES[1]] as const;
// Сторона поручителя — в «Прим.», как в Excel, но на форме — переключатель
// у заголовка, а «Прим.» остаётся свёрнутым для своего текста.
type Witness = Person & { side: string };
const newWitness = (i: number): Witness => ({ ...EMPTY_PERSON, side: WITNESS_SIDE[i] });
/** «по жениху; своё» → сторона и своё примечание. */
function splitSide(note: string | null, fallback: string): { side: string; note: string } {
  const parts = (note ?? "").split(";").map((s) => s.trim()).filter(Boolean);
  if (parts[0] && (SIDES as readonly string[]).includes(parts[0]))
    return { side: parts[0], note: parts.slice(1).join("; ") };
  return { side: parts.length ? "" : fallback, note: parts.join("; ") };
}

type Brief = {
  id: number; page: string | null; no_male: number | null;
  event_day: number | null; event_month: number | null; event_year: number | null;
  groom: string | null; bride: string | null; clergy_noname: boolean;
};

type MentionOut = {
  role_code: string; surname: string | null; first_name: string | null; patronymic: string | null;
  gender: string | null; rank: string | null; confession: string | null; place: string | null;
  note: string | null; age_years: number | null; marriage_order: string | null; kinship: string | null;
};
type EntryFull = {
  id: number; page: string | null; no_male: number | null; event_day: number | null;
  event_month: number | null; event_year: number | null; note: string | null; persons: MentionOut[];
};

/** Пол родственника по родству: «отец» — М, «мать» — Ж, иначе по имени. */
function kinGender(k: string): "М" | "Ж" | undefined {
  const v = k.trim().toLowerCase();
  if (["отец", "брат", "дядя", "дед", "супруг", "сын"].includes(v)) return "М";
  if (["мать", "сестра", "тетка", "тётка", "бабка", "супруга", "дочь"].includes(v)) return "Ж";
  return undefined;
}

export default function MarriageForm({ mkCase }: { mkCase: Case }) {
  const [page, setPage] = useState<string | null>(null);
  const [year, setYear] = useState<number | null>(mkCase.year ?? null);
  const [count, setCount] = useState<number | null>(null);
  const [day, setDay] = useState<number | null>(null);
  const [month, setMonth] = useState<number | null>(null);

  const [groom, setGroom] = useState<Spouse>(NEW_SPOUSE);
  const [bride, setBride] = useState<Spouse>(NEW_SPOUSE);
  const [groomRel, setGroomRel] = useState<Relative>(NEW_RELATIVE);
  const [brideRel, setBrideRel] = useState<Relative>(NEW_RELATIVE);
  const [w1, setW1] = useState<Witness>(newWitness(0));
  const [w2, setW2] = useState<Witness>(newWitness(1));
  const [w3, setW3] = useState<Witness>(newWitness(2));
  const [w4, setW4] = useState<Witness>(newWitness(3));
  const [clergy1, setClergy1] = useState<Person>(EMPTY_PERSON);
  const [clergy2, setClergy2] = useState<Person>(EMPTY_PERSON);
  const [clergy3, setClergy3] = useState<Person>(EMPTY_PERSON);

  const [savedTimes, setSavedTimes] = useState(0);
  const [editingId, setEditingId] = useState<number | null>(null);
  const beforeEdit = useRef<{ page: string | null; count: number | null; year: number | null;
                               month: number | null; clergy: [Person, Person, Person] } | null>(null);
  const [saved, setSaved] = useState<Brief[]>([]);
  const [busy, setBusy] = useState(false);
  const countField = useRef<HTMLInputElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const placeDefaults = { guberniya: mkCase.guberniya ?? "", uyezd: mkCase.uyezd ?? "" };

  const witnessSetters = [setW1, setW2, setW3, setW4];
  const witnesses = [w1, w2, w3, w4];

  useEffect(() => { refresh(); }, [mkCase.id]);
  const restored = useRef(false);

  function refresh() {
    invoke<Brief[]>("entry_list", { caseId: mkCase.id, section: 2 })
      .then((rows) => {
        setSaved(rows);
        if (!restored.current) {
          restored.current = true;
          if (rows[0]) resume(rows[0]);
        }
      })
      .catch((e) => report("Не удалось прочитать список набранных браков", e));
  }

  /** Продолжить с места: страница, год, месяц, следующий номер, причт. */
  function resume(last: Brief) {
    setPage((v) => v ?? last.page);
    if (last.event_year !== null) setYear(last.event_year);
    setCount((v) => v ?? (last.no_male !== null ? last.no_male + 1 : null));
    setMonth((v) => v ?? last.event_month);
    invoke<{ role_code: string; iof: string; rank: string | null; note: string | null }[]>(
      "last_clergy", { caseId: mkCase.id, section: 2 })
      .then((rows) => {
        const setters = { clergy1: setClergy1, clergy2: setClergy2, clergy3: setClergy3 } as const;
        for (const r of rows) {
          const set = setters[r.role_code as keyof typeof setters];
          if (set && r.iof.trim())
            set((p) => (p.iof.trim() ? p : { ...p, iof: r.iof, rank: r.rank ?? "", note: r.note ?? "" }));
        }
        if (rows.length > 0) setSavedTimes((n) => n + 1);
      })
      .catch((e) => report("Не удалось восстановить причт последнего брака", e));
  }

  /** Пункт переименован в карточке — то же название у всех персон записи. */
  function renamePlace(oldName: string, newName: string) {
    const fix = <T extends Person>(p: T): T => (p.place === oldName ? { ...p, place: newName } : p);
    setGroom(fix); setBride(fix); setGroomRel(fix); setBrideRel(fix);
    for (const set of witnessSetters) set(fix);
  }

  usePlaceRenamed(renamePlace);

  function pickInto<T extends Person>(set: (fn: (p: T) => T) => void) {
    return (hint: PersonHint) =>
      set((p) => ({ ...p, place: hint.place ?? p.place, rank: hint.rank ?? p.rank }));
  }

  /** Персона без разбора (поднята из базы, причт после перезапуска) — разобрать. */
  async function withParsed<T extends Person>(p: T): Promise<T> {
    if (!p.iof.trim() || p.parsed) return p;
    return { ...p, parsed: await invoke<Parsed>("parse_iof", { text: p.iof }) };
  }

  function payload(role: string, order: number, p: Person, extra: Record<string, unknown> = {},
                   gender?: string) {
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
    let g = groom, b = bride, gr = groomRel, br = brideRel;
    let ws = witnesses, cl = [clergy1, clergy2, clergy3];
    try {
      [g, b, gr, br] = await Promise.all([g, b, gr, br].map(withParsed)) as [Spouse, Spouse, Relative, Relative];
      ws = await Promise.all(ws.map(withParsed));
      cl = await Promise.all(cl.map(withParsed));
    } catch (e) {
      report("Не удалось разобрать имена перед сохранением", e);
      return;
    }
    const named: [string, Person][] = [
      ["жениха", g], ["невесты", b], ["родственника жениха", gr], ["родственника невесты", br],
      ...ws.map((w) => ["поручителя", w] as [string, Person]),
      ...cl.map((c) => ["причта", c] as [string, Person]),
    ];
    const unknown = named.find(([, p]) => p.iof.trim() && p.parsed && !p.parsed.known_name);
    if (unknown) {
      report(`Имя ${unknown[0]} «${unknown[1].parsed?.first_name}» не сверено со справочником`,
             unknown[0] === "причта"
               ? "нажмите «Изменить» у причта и выйдите из поля ИОФ — откроется окно сверки"
               : "выйдите из поля ИОФ — откроется окно сверки и предложит имя из справочника");
      return;
    }
    if (!g.iof.trim() && !b.iof.trim()) {
      report("Запись пустая", "не заполнены ни жених, ни невеста");
      return;
    }
    if (year === null) {
      report("Не указан год", "год записи стоит в первой строке формы — заполните его один раз, дальше он держится сам");
      root.current?.querySelector<HTMLInputElement>(".row.tight input")?.focus();
      return;
    }
    const persons: ReturnType<typeof payload>[] = [];
    if (g.iof.trim()) persons.push(payload("groom", 10, g, { age_years: g.age, marriage_order: g.order.trim() || null }, "М"));
    if (b.iof.trim()) persons.push(payload("bride", 20, b, { age_years: b.age, marriage_order: b.order.trim() || null }, "Ж"));
    // Родственник пишется и без имени: «отец» стоит у всех 180 записей Excel,
    // а имя — лишь у 13–15. Выгрузка в Familio ждёт его в колонке «Отец жениха».
    const rel = (role: string, order: number, r: Relative) => {
      if (r.iof.trim() || r.kinship.trim())
        persons.push(payload(role, order, r, { kinship: r.kinship.trim() || null }, kinGender(r.kinship)));
    };
    rel("groom_relative", 30, gr);
    rel("bride_relative", 50, br);
    ws.forEach((w, i) => {
      if (!w.iof.trim()) return;
      const note = [w.side, w.note.trim()].filter(Boolean).join("; ");
      persons.push(payload(`witness${i + 1}`, 60 + i * 10, { ...w, note }));
    });
    cl.forEach((c, i) => { if (c.iof.trim()) persons.push(payload(`clergy${i + 1}`, 100 + i * 10, c, {}, "М")); });

    setBusy(true);
    try {
      await invoke<number>("entry_save", {
        entry: {
          id: editingId, case_id: mkCase.id, section: 2, page,
          no_male: count, no_female: null,
          event_day: day, event_month: month, event_year: year,
          rite_day: null, rite_month: null, rite_year: null,
          note: null, uncertain: null, persons,
        },
      });
      setSavedTimes((n) => n + 1);
      if (editingId !== null) restoreAfterEdit();
      else {
        next();
        setCount((c) => (c === null ? null : c + 1));
      }
      refresh();
    } catch (e) {
      report(editingId ? "Не удалось сохранить изменения" : "Не удалось сохранить запись о браке", e);
    } finally {
      setBusy(false);
    }
  }

  function formDirty(): boolean {
    return [groom, bride, groomRel, brideRel, ...witnesses].some((p) => p.iof.trim().length > 0);
  }

  async function openEntry(id: number) {
    if (editingId !== null && editingId !== id) {
      report("Сначала сохраните изменения или нажмите «Отменить»", "открыта другая запись — её правки иначе пропадут");
      return;
    }
    if (editingId === null && formDirty()) {
      report("Сначала сохраните или очистите набранное",
             "открыть запись для правки можно только с пустой формы — иначе набранное пропадёт");
      return;
    }
    try {
      const e = await invoke<EntryFull>("entry_load", { id });
      if (editingId === null)
        beforeEdit.current = { page, count, year, month, clergy: [clergy1, clergy2, clergy3] };
      const iof = (m: MentionOut) => [m.first_name, m.patronymic, m.surname].filter(Boolean).join(" ");
      const person = (m: MentionOut | undefined, base: Person): Person =>
        m ? { ...base, iof: iof(m), parsed: null, place: m.place ?? "", rank: m.rank ?? "",
              confession: m.confession ?? "", note: m.note ?? "" } : { ...base, note: "" };
      const by = (role: string) => e.persons.find((m) => m.role_code === role);
      const spouse = (m: MentionOut | undefined): Spouse =>
        ({ ...person(m, NEW_SPOUSE), order: m?.marriage_order ?? "", age: m?.age_years ?? null });
      const relative = (m: MentionOut | undefined): Relative =>
        ({ ...person(m, EMPTY_PERSON), kinship: m?.kinship ?? "" });
      setPage(e.page);
      setCount(e.no_male);
      setDay(e.event_day);
      setMonth(e.event_month);
      if (e.event_year !== null) setYear(e.event_year);
      setGroom(spouse(by("groom")));
      setBride(spouse(by("bride")));
      setGroomRel(relative(by("groom_relative")));
      setBrideRel(relative(by("bride_relative")));
      witnessSetters.forEach((set, i) => {
        const m = by(`witness${i + 1}`);
        const p = person(m, EMPTY_PERSON);
        const { side, note } = splitSide(m?.note ?? null, m ? "" : WITNESS_SIDE[i]);
        set({ ...p, note, side });
      });
      setClergy1(person(by("clergy1"), EMPTY_PERSON));
      setClergy2(person(by("clergy2"), EMPTY_PERSON));
      setClergy3(person(by("clergy3"), EMPTY_PERSON));
      setEditingId(e.id);
      window.scrollTo({ top: 0 });
      countField.current?.focus();
    } catch (err) {
      report("Не удалось открыть запись о браке", err);
    }
  }

  function restoreAfterEdit() {
    const b = beforeEdit.current;
    beforeEdit.current = null;
    setEditingId(null);
    next();
    if (b) {
      setPage(b.page); setCount(b.count); setYear(b.year); setMonth(b.month);
      setClergy1(b.clergy[0]); setClergy2(b.clergy[1]); setClergy3(b.clergy[2]);
    }
  }

  /** Следующая запись: страница, год, месяц и причт остаются. */
  function next() {
    setGroom({ ...NEW_SPOUSE });
    setBride({ ...NEW_SPOUSE });
    setGroomRel({ ...NEW_RELATIVE });
    setBrideRel({ ...NEW_RELATIVE });
    witnessSetters.forEach((set, i) => set(newWitness(i)));
    setDay(null);
    countField.current?.focus();
    countField.current?.select();
  }

  function hotkeys(e: React.KeyboardEvent<HTMLDivElement>) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && !busy) {
      e.preventDefault();
      void save();
    }
  }

  /** «Каким браком» и «Лет» — под вероисповеданием жениха и невесты. */
  const spouseExtra = (p: Spouse, set: (fn: (s: Spouse) => Spouse) => void) => (
    <>
      <Suggest label="Каким браком" kind="marriage_order" value={p.order} browse
               onChange={(order) => set((s) => ({ ...s, order }))} />
      <div className="row">
        <NumberField label="Лет" value={p.age} min={1} max={100}
                     onChange={(age) => set((s) => ({ ...s, age }))} />
      </div>
    </>
  );

  const relativeBefore = (r: Relative, set: (fn: (s: Relative) => Relative) => void) => (
    <Suggest label="Родство" kind="kinship" value={r.kinship} browse
             onChange={(kinship) => set((s) => ({ ...s, kinship }))} />
  );

  const common = { placeDefaults, onPlaceRenamed: renamePlace, rankKind: "rank" as const };

  return (
    <div onKeyDown={hotkeys} ref={root} className="marriage">
      {editingId !== null && (
        <div className="editbar">
          <b>Правка записи</b> — сохранённая запись о браке открыта в форме. «Сохранить
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
        <div className="row">
          <NumberField label="Венч., день" value={day} onChange={setDay} min={1} max={31} />
          <NumberField label="месяц" value={month} onChange={setMonth} min={1} max={12} />
        </div>
      </section>

      <PersonBlock title="Жених" person={groom} onChange={(p) => setGroom((s) => ({ ...s, ...p }))}
                   gender="М" withConfession onPickPerson={pickInto(setGroom)} {...common}
                   extra={spouseExtra(groom, setGroom)} />
      <PersonBlock title="Невеста" person={bride} onChange={(p) => setBride((s) => ({ ...s, ...p }))}
                   gender="Ж" withConfession onPickPerson={pickInto(setBride)} {...common}
                   extra={spouseExtra(bride, setBride)} />

      <section className="person">
        <h2>Родственники</h2>
        <PersonBlock title="Жениха" person={groomRel} compact
                     onChange={(p) => setGroomRel((s) => ({ ...s, ...p }))}
                     gender={kinGender(groomRel.kinship)} {...common}
                     before={relativeBefore(groomRel, setGroomRel)} />
        <PersonBlock title="Невесты" person={brideRel} compact
                     onChange={(p) => setBrideRel((s) => ({ ...s, ...p }))}
                     gender={kinGender(brideRel.kinship)} {...common}
                     before={relativeBefore(brideRel, setBrideRel)} />
      </section>

      {witnesses.map((w, i) => (
        <PersonBlock key={i} title={`Поручитель ${i + 1}`} person={w}
                     onChange={(p) => witnessSetters[i]((s) => ({ ...s, ...p }))}
                     onPickPerson={pickInto(witnessSetters[i])} {...common}
                     titleExtra={
                       <button type="button" className="linkish side" tabIndex={-1}
                               title="Сменить сторону поручителя"
                               onClick={() => witnessSetters[i]((s) => ({
                                 ...s, side: s.side === SIDES[0] ? SIDES[1] : SIDES[0] }))}>
                         {w.side || "сторона не указана"} ⇄
                       </button>
                     } />
      ))}

      <ClergyBlock
        people={[clergy1, clergy2, clergy3]}
        onChange={(i, p) => [setClergy1, setClergy2, setClergy3][i](p)}
        reloadKey={savedTimes}
      />

      <div className="savebar">
        <button className="primary" onClick={save} disabled={busy} title="Ctrl+Enter">
          {busy ? "Сохраняю…" : editingId !== null ? "Сохранить изменения" : "Сохранить и следующая"}
          <span className="kbd">Ctrl+Enter</span>
        </button>
      </div>

      {saved.length > 0 && (
        <section>
          <h2>Набрано браков: {saved.length}</h2>
          <p className="hint">Нажмите «Открыть», чтобы поправить запись. Форма при этом должна быть пустой.</p>
          <table className="facts saved">
            <tbody>
              {saved.map((e) => (
                <tr key={e.id} className={e.id === editingId ? "editing" : ""}>
                  <td>
                    {e.event_day ?? "?"}.{e.event_month ?? "?"} · {e.groom || "жених не указан"}
                    <span className="sub"> и {e.bride || "невеста не указана"}</span>
                  </td>
                  <td>{e.no_male !== null ? `№ ${e.no_male}` : "без №"}</td>
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
