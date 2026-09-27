import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { type Parsed, type PersonHint } from "./IofField";
import PersonBlock, { EMPTY_PERSON, usePlaceRenamed, type Person } from "./PersonBlock";
import NumberField from "./NumberField";
import PageField from "./PageField";
import ClergyBlock from "./ClergyBlock";
import Suggest from "./Suggest";
import { useFormClergy } from "./clergy";
import { splitCount, type Sex } from "./count";
import { parseAge } from "./age";
import { focusNextField } from "./focus";
import { dismissWarn, report, warn } from "./errors";
import type { Case } from "./CaseHeader";

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
 * угадать следующий нельзя (как в рождениях).
 */

const KIN_DEFAULT = "отец";

type Deceased = Person & { cause: string; age: string };
type Relative = Person & { kinship: string };

const NEW_DECEASED: Deceased = { ...EMPTY_PERSON, cause: "", age: "" };
const NEW_RELATIVE: Relative = { ...EMPTY_PERSON, kinship: KIN_DEFAULT };

type Brief = {
  id: number; page: string | null; no_male: number | null; no_female: number | null;
  event_day: number | null; event_month: number | null; event_year: number | null;
  rite_month: number | null; deceased: string | null; clergy_noname: boolean;
};

type MentionOut = {
  role_code: string; surname: string | null; first_name: string | null; patronymic: string | null;
  gender: string | null; rank: string | null; confession: string | null; place: string | null;
  note: string | null; kinship: string | null; age_text: string | null; death_cause: string | null;
};
type EntryFull = {
  id: number; page: string | null; no_male: number | null; no_female: number | null;
  event_day: number | null; event_month: number | null; event_year: number | null;
  rite_day: number | null; rite_month: number | null; note: string | null; persons: MentionOut[];
};

/** Пол родственника по родству: «отец» — М, «мать» — Ж, иначе по имени. */
function kinGender(k: string): "М" | "Ж" | undefined {
  const v = k.trim().toLowerCase();
  if (["отец", "брат", "дядя", "дед", "супруг", "муж", "сын"].includes(v)) return "М";
  if (["мать", "сестра", "тетка", "тётка", "бабка", "супруга", "жена", "дочь"].includes(v)) return "Ж";
  return undefined;
}

export default function DeathForm({ mkCase }: { mkCase: Case }) {
  const [page, setPage] = useState<string | null>(null);
  const [year, setYear] = useState<number | null>(mkCase.year ?? null);
  const [count, setCount] = useState<number | null>(null);
  const [deathDay, setDeathDay] = useState<number | null>(null);
  const [deathMonth, setDeathMonth] = useState<number | null>(null);
  const [burialDay, setBurialDay] = useState<number | null>(null);
  const [burialMonth, setBurialMonth] = useState<number | null>(null);

  const [dead, setDead] = useState<Deceased>(NEW_DECEASED);
  const [rel, setRel] = useState<Relative>(NEW_RELATIVE);
  // Пол умершего руками — только когда по имени его не понять. От него
  // зависит колонка счёта, как у рождений.
  const [sexManual, setSexManual] = useState<Sex | null>(null);
  const [askSex, setAskSex] = useState(false);

  const clergyState = useFormClergy();
  const [clergy1, clergy2, clergy3] = clergyState.people;

  const [editingId, setEditingId] = useState<number | null>(null);
  const beforeEdit = useRef<{ page: string | null; count: number | null; year: number | null;
                               deathMonth: number | null; burialMonth: number | null } | null>(null);
  const [saved, setSaved] = useState<Brief[]>([]);
  const [busy, setBusy] = useState(false);
  const countField = useRef<HTMLInputElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const placeDefaults = { guberniya: mkCase.guberniya ?? "", uyezd: mkCase.uyezd ?? "" };

  useEffect(() => { refresh(); }, [mkCase.id]);
  const restored = useRef(false);

  function refresh() {
    invoke<Brief[]>("entry_list", { caseId: mkCase.id, section: 3 })
      .then((rows) => {
        setSaved(rows);
        if (!restored.current) {
          restored.current = true;
          if (rows[0]) resume(rows[0]);
        }
      })
      .catch((e) => report("Не удалось прочитать список набранных смертей", e));
  }

  /** Продолжить с места: страница, год, счёт, месяцы. Причт — общий. */
  function resume(last: Brief) {
    setPage((v) => v ?? last.page);
    if (last.event_year !== null) setYear(last.event_year);
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

  async function withParsed<T extends Person>(p: T): Promise<T> {
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
    if (!d.iof.trim()) {
      warn("Запись пустая", "не заполнено имя умершего");
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
      await invoke<number>("entry_save", {
        entry: {
          id: editingId, case_id: mkCase.id, section: 3, page,
          no_male: columns.no_male, no_female: columns.no_female,
          event_day: deathDay, event_month: deathMonth, event_year: year,
          rite_day: burialDay, rite_month: burialMonth, rite_year: year,
          note: null, uncertain: null, persons,
        },
      });
      clergyState.bump();
      dismissWarn();
      if (editingId !== null) restoreAfterEdit();
      else next();
      refresh();
    } catch (e) {
      report(editingId ? "Не удалось сохранить изменения" : "Не удалось сохранить запись о смерти", e);
    } finally {
      setBusy(false);
    }
  }

  function formDirty(): boolean {
    return [dead, rel].some((p) => p.iof.trim() || p.place.trim() || p.rank.trim())
      || dead.cause.trim().length > 0 || dead.age.trim().length > 0;
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
      if (e.event_year !== null) setYear(e.event_year);
      setDead({ ...person(dm, NEW_DECEASED), cause: dm?.death_cause ?? "", age: dm?.age_text ?? "" });
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
      setPage(b.page); setCount(b.count); setYear(b.year);
      setDeathMonth(b.deathMonth); setBurialMonth(b.burialMonth);
    }
    clergyState.close();
  }

  /** Следующая запись: страница, год, счёт, месяцы и причт остаются. */
  function next() {
    setDead({ ...NEW_DECEASED });
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

  const parsedSex = dead.parsed?.gender as Sex | null | undefined;
  const ageParsed = parseAge(dead.age);

  const deadExtra = (
    <>
      <Suggest label="Причина" kind="death_cause" value={dead.cause} browse
               onChange={(cause) => setDead((s) => ({ ...s, cause }))} />
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
      {dead.iof.trim() && dead.parsed && !parsedSex && (
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
          <NumberField label="месяц" value={deathMonth} onChange={setDeathMonth} min={1} max={12} />
          <NumberField label="Погреб., день" value={burialDay} onChange={setBurialDay} min={1} max={31} />
          <NumberField label="месяц" value={burialMonth} onChange={setBurialMonth} min={1} max={12} />
        </div>
      </section>

      <div className="cols">
        <PersonBlock title="Умерший" person={dead} onChange={(p) => setDead((s) => ({ ...s, ...p }))}
                     gender={parsedSex ?? sexManual ?? undefined}
                     onPickPerson={pickInto(setDead)} {...common} extra={deadExtra} />
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
          <h2>Набрано смертей: {saved.length}</h2>
          <p className="hint">Нажмите «Открыть», чтобы поправить запись. Форма при этом должна быть пустой.</p>
          <table className="facts saved">
            <tbody>
              {saved.map((e) => (
                <tr key={e.id} className={e.id === editingId ? "editing" : ""}>
                  <td>
                    {e.no_male !== null ? `№ м. ${e.no_male}` : e.no_female !== null ? `№ ж. ${e.no_female}` : "без №"}
                    {" · "}{e.event_day ?? "?"}.{e.event_month ?? "?"} · {e.deceased || "умерший не указан"}
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
