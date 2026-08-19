import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import IofField, { type Parsed, type PersonHint } from "./IofField";
import PersonBlock, { EMPTY_PERSON, type Person } from "./PersonBlock";
import NumberField from "./NumberField";
import { report } from "./errors";
import type { Case } from "./CaseHeader";

/**
 * Форма ввода записи о рождении.
 *
 * ПОРЯДОК ПОЛЕЙ ВЗЯТ ИЗ ФОРМЫ ВВОДА EXCEL, лист «МК Ввод» при Части = 1:
 *
 *   Стр., Счёт родившихся, День рождения, Месяц рождения,
 *   День крещения, Месяц крещения, ребёнок,
 *   отец (ИОФ, НП, Звание, Вероисповедания, Прим.),
 *   мать (ИОФ, НП, Звание, Вероисповедания, Прим., Девичья фамилия),
 *   восприемник 1 (ИОФ, НП, Звание, Прим.),
 *   восприемник 2 (ИОФ, НП, Звание, Прим.)
 *
 * Это ключевое требование заказчика: работа не будет принята, если порядок
 * отличается. Прежняя попытка выстроить поля «по порядку чтения записи»
 * (НП, звание, ИОФ) отвергнута 17.08.2026.
 *
 * Счёт — одно поле, как в Excel. Разделение на «счёт М» и «счёт Ж» было
 * нашей самодеятельностью, заказчик просил вернуть одно.
 *
 * МАТЬ ЗАПОЛНЯЕТСЯ САМА: населённый пункт наследуется от отца, звание —
 * «законная жена его». На 294 записях его работы совпадение 292 и 293 раза
 * соответственно. Проверено на сборке 14.08: править не приходилось ни разу.
 */

const MOTHER_RANK = "законная жена его";

type Brief = {
  id: number;
  page: string | null;
  no_male: number | null;
  event_day: number | null;
  event_month: number | null;
  child: string | null;
};

type PersonPayload = {
  role_code: string;
  sort_order: number;
  surname: string | null;
  first_name: string | null;
  patronymic: string | null;
  surname_modern: string | null;
  first_name_modern: string | null;
  patronymic_modern: string | null;
  maiden_surname: string | null;
  gender: string | null;
  rank: string | null;
  confession: string | null;
  place: string | null;
  note: string | null;
  uncertain: string | null;
};

export default function BirthForm({ mkCase }: { mkCase: Case }) {
  const [page, setPage] = useState<number | null>(null);
  const [count, setCount] = useState<number | null>(null);
  const [birthDay, setBirthDay] = useState<number | null>(null);
  const [birthMonth, setBirthMonth] = useState<number | null>(null);
  const [riteDay, setRiteDay] = useState<number | null>(null);
  const [riteMonth, setRiteMonth] = useState<number | null>(null);

  const [child, setChild] = useState("");
  const [childParsed, setChildParsed] = useState<Parsed | null>(null);
  const [father, setFather] = useState<Person>(EMPTY_PERSON);
  const [mother, setMother] = useState<Person>({ ...EMPTY_PERSON, rank: MOTHER_RANK });
  const [god1, setGod1] = useState<Person>(EMPTY_PERSON);
  const [god2, setGod2] = useState<Person>(EMPTY_PERSON);

  const [saved, setSaved] = useState<Brief[]>([]);
  const [busy, setBusy] = useState(false);
  const firstField = useRef<HTMLInputElement>(null);

  useEffect(() => {
    refresh();
  }, [mkCase.id]);

  function refresh() {
    invoke<Brief[]>("entry_list", { caseId: mkCase.id, section: 1 })
      .then(setSaved)
      .catch((e) => report("Не удалось прочитать список набранных записей", e));
  }

  /** Выбрали отца из базы — подставляем его населённый пункт, звание и жену. */
  function pickFather(hint: PersonHint) {
    setFather((f) => ({
      ...f,
      place: hint.place ?? f.place,
      rank: hint.rank ?? f.rank,
    }));
    setMother((m) => ({
      ...m,
      place: hint.place ?? m.place,
      rank: m.rank || MOTHER_RANK,
    }));
    invoke<{ iof: string; place: string | null; rank: string | null } | null>(
      "suggest_spouse", { husband: hint.iof },
    )
      .then((wife) => {
        if (!wife) return;
        setMother((m) => ({
          ...m,
          iof: m.iof || wife.iof,
          place: wife.place ?? m.place,
          rank: wife.rank ?? m.rank,
        }));
      })
      .catch((e) => report("Не удалось найти жену по отцу", e));
  }

  /** Для остальных персон выбор из базы заполняет населённый пункт и звание. */
  function pickInto(set: (fn: (p: Person) => Person) => void) {
    return (hint: PersonHint) =>
      set((p) => ({ ...p, place: hint.place ?? p.place, rank: hint.rank ?? p.rank }));
  }

  function payload(role: string, order: number, p: Person, gender?: string): PersonPayload {
    return {
      role_code: role,
      sort_order: order,
      surname: p.parsed?.surname ?? null,
      first_name: p.parsed?.first_name ?? null,
      patronymic: p.parsed?.patronymic ?? null,
      surname_modern: null,
      first_name_modern: p.parsed?.first_name_modern ?? null,
      patronymic_modern: p.parsed?.patronymic_modern ?? null,
      maiden_surname: p.maiden || null,
      gender: gender ?? p.parsed?.gender ?? null,
      rank: p.rank || null,
      confession: p.confession || null,
      place: p.place || null,
      note: p.note || null,
      uncertain: null,
    };
  }

  async function save() {
    const persons: PersonPayload[] = [{
      role_code: "child",
      sort_order: 10,
      surname: childParsed?.surname ?? null,
      first_name: childParsed?.first_name ?? null,
      patronymic: childParsed?.patronymic ?? null,
      surname_modern: null,
      first_name_modern: childParsed?.first_name_modern ?? null,
      patronymic_modern: childParsed?.patronymic_modern ?? null,
      maiden_surname: null,
      gender: childParsed?.gender ?? null,
      rank: null,
      confession: null,
      place: null,
      note: null,
      uncertain: null,
    }];
    if (father.iof.trim()) persons.push(payload("father", 20, father, "М"));
    if (mother.iof.trim()) persons.push(payload("mother", 30, mother, "Ж"));
    if (god1.iof.trim()) persons.push(payload("godparent1", 40, god1));
    if (god2.iof.trim()) persons.push(payload("godparent2", 50, god2));

    if (!child.trim() && persons.length === 1) {
      report("Запись пустая", "не заполнено ни имя ребёнка, ни родители");
      return;
    }

    setBusy(true);
    try {
      await invoke<number>("entry_save", {
        entry: {
          id: null,
          case_id: mkCase.id,
          section: 1,
          page: page === null ? null : String(page),
          // Счёт в Excel один. Пол ребёнка программа знает по имени,
          // поэтому раскладывать счёт на мужской и женский незачем.
          no_male: count,
          no_female: null,
          event_day: birthDay,
          event_month: birthMonth,
          event_year: mkCase.year ?? null,
          rite_day: riteDay,
          rite_month: riteMonth,
          rite_year: mkCase.year ?? null,
          note: null,
          uncertain: null,
          persons,
        },
      });
      next();
      refresh();
    } catch (e) {
      report("Не удалось сохранить запись", e);
    } finally {
      setBusy(false);
    }
  }

  /** Очищает то, что меняется от записи к записи. Страница, месяц и счёт
   *  остаются и растут сами: записи в книге идут подряд. */
  function next() {
    setChild("");
    setChildParsed(null);
    setFather(EMPTY_PERSON);
    setMother({ ...EMPTY_PERSON, rank: MOTHER_RANK });
    setGod1(EMPTY_PERSON);
    setGod2(EMPTY_PERSON);
    setBirthDay(null);
    setRiteDay(null);
    if (count !== null) setCount(count + 1);
    firstField.current?.focus();
  }

  return (
    <>
      <section>
        <div className="row">
          <NumberField label="Стр." value={page} onChange={setPage} min={1} />
          <NumberField label="Счёт" value={count} onChange={setCount} min={1} />
        </div>
        <div className="row">
          <NumberField label="Рожд., день" value={birthDay} onChange={setBirthDay} min={1} max={31} />
          <NumberField label="месяц" value={birthMonth} onChange={setBirthMonth} min={1} max={12} />
        </div>
        <div className="row">
          <NumberField label="Крещ., день" value={riteDay} onChange={setRiteDay} min={1} max={31} />
          <NumberField label="месяц" value={riteMonth} onChange={setRiteMonth} min={1} max={12} />
        </div>
        <IofField
          label="Ребёнок"
          value={child}
          onChange={(text, parsed) => {
            setChild(text);
            setChildParsed(parsed);
          }}
          placeholder="имя"
          inputRef={firstField}
        />
      </section>

      <PersonBlock
        title="Отец"
        person={father}
        onChange={setFather}
        rankKind="rank_m"
        withConfession
        onPickPerson={pickFather}
      />
      <PersonBlock
        title="Мать"
        person={mother}
        onChange={setMother}
        rankKind="rank_f"
        withConfession
        withMaiden
        onPickPerson={pickInto(setMother)}
      />
      <PersonBlock
        title="Восприемник 1"
        person={god1}
        onChange={setGod1}
        rankKind="rank_m"
        onPickPerson={pickInto(setGod1)}
      />
      <PersonBlock
        title="Восприемник 2"
        person={god2}
        onChange={setGod2}
        rankKind="rank_m"
        onPickPerson={pickInto(setGod2)}
      />

      <section>
        <button className="primary" onClick={save} disabled={busy}>
          {busy ? "Сохраняю…" : "Сохранить и следующая"}
        </button>
      </section>

      {saved.length > 0 && (
        <section>
          <h2>Набрано: {saved.length}</h2>
          <table className="facts">
            <tbody>
              {saved.slice(0, 10).map((e) => (
                <tr key={e.id}>
                  <td>
                    {e.event_day ?? "?"}.{e.event_month ?? "?"} · {e.child || "без имени"}
                  </td>
                  <td>стр. {e.page ?? "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
    </>
  );
}
