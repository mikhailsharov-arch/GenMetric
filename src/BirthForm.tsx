import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Suggest from "./Suggest";
import IofField from "./IofField";
import NumberField from "./NumberField";
import { report } from "./errors";
import type { Case } from "./CaseHeader";

/**
 * Форма ввода записи о рождении.
 *
 * ПОРЯДОК ПОЛЕЙ повторяет порядок чтения записи в книге, а не порядок колонок
 * в таблице вывода. Запись читается так: «января 5 родилась, 7 крещена
 * Татьяна. Родители: деревни Чертеж Малый крестьянин Никита Алексеев
 * и законная жена его Евлампия Васильева. Восприемник: деревни Чертеж
 * Большой крестьянский сын Александр Арсеньев».
 *
 * Отсюда: сначала даты, потом имя ребёнка, потом каждая персона в порядке
 * населённый пункт, звание, ИОФ. Ровно этот порядок Роман просил для раздела
 * о смертях — оказалось, что это общее правило, а не частный случай.
 *
 * МАТЬ ЗАПОЛНЯЕТСЯ САМА. На 294 записях из его работы населённый пункт матери
 * совпал с отцовским 292 раза, а звание «законная жена его» встретилось
 * 293 раза. Поэтому оба поля подставляются, и человек правит только
 * исключения. У восприемников совпадение 183 из 282 — там подставлять нельзя,
 * населённый пункт отца идёт лишь верхней подсказкой.
 */

type Parsed = {
  first_name: string | null;
  first_name_modern: string | null;
  patronymic: string | null;
  patronymic_modern: string | null;
  surname: string | null;
  gender: string | null;
};

type Person = { iof: string; parsed: Parsed | null; place: string; rank: string; note: string };

/** То, что уходит в базу на каждую упомянутую персону. */
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

const EMPTY: Person = { iof: "", parsed: null, place: "", rank: "", note: "" };

const MOTHER_RANK = "законная жена его";

type Brief = {
  id: number;
  page: string | null;
  no_male: number | null;
  no_female: number | null;
  event_day: number | null;
  event_month: number | null;
  child: string | null;
};

export default function BirthForm({ mkCase }: { mkCase: Case }) {
  const [page, setPage] = useState<number | null>(null);
  const [noMale, setNoMale] = useState<number | null>(null);
  const [noFemale, setNoFemale] = useState<number | null>(null);
  const [birthDay, setBirthDay] = useState<number | null>(null);
  const [birthMonth, setBirthMonth] = useState<number | null>(null);
  const [riteDay, setRiteDay] = useState<number | null>(null);
  const [riteMonth, setRiteMonth] = useState<number | null>(null);

  const [child, setChild] = useState("");
  const [childParsed, setChildParsed] = useState<Parsed | null>(null);
  const [father, setFather] = useState<Person>(EMPTY);
  const [mother, setMother] = useState<Person>({ ...EMPTY, rank: MOTHER_RANK });
  const [god1, setGod1] = useState<Person>(EMPTY);
  const [god2, setGod2] = useState<Person>(EMPTY);

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

  // Населённый пункт и звание матери наследуются от отца — см. замер выше.
  function setFatherPlace(place: string) {
    setFather((f) => ({ ...f, place }));
    setMother((m) => (m.place === "" || m.place === father.place ? { ...m, place } : m));
  }

  function person(role: string, order: number, p: Person, gender?: string): PersonPayload {
    return {
      role_code: role,
      sort_order: order,
      surname: p.parsed?.surname ?? null,
      first_name: p.parsed?.first_name ?? null,
      patronymic: p.parsed?.patronymic ?? null,
      surname_modern: null,
      first_name_modern: p.parsed?.first_name_modern ?? null,
      patronymic_modern: p.parsed?.patronymic_modern ?? null,
      maiden_surname: null,
      gender: gender ?? p.parsed?.gender ?? null,
      rank: p.rank || null,
      confession: null,
      place: p.place || null,
      note: p.note || null,
      uncertain: null,
    };
  }

  async function save() {
    const persons: PersonPayload[] = [
      {
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
      },
    ];
    if (father.iof.trim()) persons.push(person("father", 20, father, "М"));
    if (mother.iof.trim()) persons.push(person("mother", 30, mother, "Ж"));
    if (god1.iof.trim()) persons.push(person("godparent1", 40, god1));
    if (god2.iof.trim()) persons.push(person("godparent2", 50, god2));

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
          no_male: noMale,
          no_female: noFemale,
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

  /** Очищает то, что меняется от записи к записи. Шапка дела, страница,
   *  месяц и счёт остаются: они идут подряд и переписывать их незачем. */
  function next() {
    setChild("");
    setChildParsed(null);
    setFather(EMPTY);
    setMother({ ...EMPTY, rank: MOTHER_RANK });
    setGod1(EMPTY);
    setGod2(EMPTY);
    setBirthDay(null);
    setRiteDay(null);
    if (noMale !== null) setNoMale(noMale + 1);
    if (noFemale !== null) setNoFemale(noFemale + 1);
    firstField.current?.focus();
  }

  const personBlock = (
    title: string,
    p: Person,
    set: (v: Person) => void,
    opts: { placeHint?: string; rankKind: string; onPlace?: (v: string) => void } = { rankKind: "rank_m" },
  ) => (
    <section>
      <h2>{title}</h2>
      <Suggest
        label="Населённый пункт"
        kind="place"
        value={p.place}
        onChange={(v) => (opts.onPlace ? opts.onPlace(v) : set({ ...p, place: v }))}
        hint={opts.placeHint}
      />
      <Suggest
        label="Звание"
        kind={opts.rankKind}
        value={p.rank}
        onChange={(v) => set({ ...p, rank: v })}
      />
      <IofField
        label="ИОФ"
        value={p.iof}
        onChange={(text, parsed) => set({ ...p, iof: text, parsed })}
        placeholder="имя, отчество, фамилия"
      />
    </section>
  );

  return (
    <>
      <section>
        <h2>Запись</h2>
        <div className="row">
          <NumberField label="Страница" value={page} onChange={setPage} min={1} />
          <NumberField label="Счёт М" value={noMale} onChange={setNoMale} min={1} />
          <NumberField label="Счёт Ж" value={noFemale} onChange={setNoFemale} min={1} />
        </div>
        <div className="row">
          <NumberField label="Родился, день" value={birthDay} onChange={setBirthDay} min={1} max={31} />
          <NumberField label="Месяц" value={birthMonth} onChange={setBirthMonth} min={1} max={12} />
          <NumberField label="Крещён, день" value={riteDay} onChange={setRiteDay} min={1} max={31} />
          <NumberField label="Месяц" value={riteMonth} onChange={setRiteMonth} min={1} max={12} />
        </div>
        <p className="hint">
          Год берётся из дела: {mkCase.year ?? "не задан"}. Стрелки влево и вправо
          с зажатым Alt меняют число на единицу.
        </p>
      </section>

      <section>
        <h2>Ребёнок</h2>
        <IofField
          label="Имя"
          value={child}
          onChange={(text, parsed) => {
            setChild(text);
            setChildParsed(parsed);
          }}
          placeholder="например, Татьяна"
        />
      </section>

      {personBlock("Отец", father, setFather, {
        rankKind: "rank_m",
        onPlace: setFatherPlace,
      })}
      {personBlock("Мать", mother, setMother, {
        rankKind: "rank_f",
        placeHint: "Подставлен от отца — на настоящих записях совпадает почти всегда",
      })}
      {personBlock("Восприемник 1", god1, setGod1, { rankKind: "rank_m" })}
      {personBlock("Восприемник 2", god2, setGod2, { rankKind: "rank_m" })}

      <section>
        <button className="primary" onClick={save} disabled={busy}>
          {busy ? "Сохраняю…" : "Сохранить и следующая"}
        </button>
        <p className="hint">
          После сохранения очищаются только поля персон и дни. Страница, месяц
          и счёт остаются и увеличиваются сами — записи идут подряд.
        </p>
      </section>

      {saved.length > 0 && (
        <section>
          <h2>Набрано записей: {saved.length}</h2>
          <table className="facts">
            <tbody>
              {saved.slice(0, 15).map((e) => (
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
