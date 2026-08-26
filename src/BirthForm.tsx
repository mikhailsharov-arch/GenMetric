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

// В работе Романа по Борисоглебскому приходу вероисповедание родителей стоит
// «православного» во всех 4885 упоминаниях. Заказчик 24.08.2026:
// «вероисповедания должно быть по-умолчанию заполнено». Поле остаётся
// обычным — исправить можно всегда.
const CONFESSION = "православного";

const NEW_FATHER = { ...EMPTY_PERSON, confession: CONFESSION };
const NEW_MOTHER = { ...EMPTY_PERSON, rank: MOTHER_RANK, confession: CONFESSION };

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
  const [father, setFatherState] = useState<Person>(NEW_FATHER);
  const [mother, setMother] = useState<Person>(NEW_MOTHER);
  const [god1, setGod1] = useState<Person>(EMPTY_PERSON);
  const [god2, setGod2] = useState<Person>(EMPTY_PERSON);

  // Причт держится между записями: в книге он один на весь разворот, а часто
  // и на всё дело. Очищать его каждую запись — заставлять набирать заново.
  const [clergy1, setClergy1] = useState<Person>(EMPTY_PERSON);
  const [clergy2, setClergy2] = useState<Person>(EMPTY_PERSON);
  const [clergy3, setClergy3] = useState<Person>(EMPTY_PERSON);

  const [saved, setSaved] = useState<Brief[]>([]);
  const [busy, setBusy] = useState(false);
  const countField = useRef<HTMLInputElement>(null);

  useEffect(() => {
    refresh();
  }, [mkCase.id]);

  function refresh() {
    invoke<Brief[]>("entry_list", { caseId: mkCase.id, section: 1 })
      .then(setSaved)
      .catch((e) => report("Не удалось прочитать список набранных записей", e));
  }

  /**
   * Правка отца руками.
   *
   * Населённый пункт матери повторяет отцовский: на 294 записях его работы
   * совпало 292 раза. Раньше это работало только при выборе отца из базы,
   * а при наборе НП руками — нет. Заказчик 24.08.2026: «при заполнении НП
   * для отца, НП матери должен автоматически заполняться».
   *
   * Свой НП матери не затирается: как только он отличается от отцовского,
   * значит его поставили руками, и трогать его нельзя.
   */
  function setFather(next: Person) {
    const prev = father;
    if (next.place !== prev.place) {
      setMother((m) =>
        !m.place || m.place === prev.place ? { ...m, place: next.place } : m);
    }
    setFatherState(next);
  }

  /** Выбрали отца из базы — подставляем его населённый пункт, звание и жену. */
  function pickFather(hint: PersonHint) {
    setFatherState((f) => ({
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

  /** У причта населённого пункта нет — только звание. */
  function pickRank(set: (fn: (p: Person) => Person) => void) {
    return (hint: PersonHint) => set((p) => ({ ...p, rank: hint.rank ?? p.rank }));
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
    // Причт пишется в каждую запись: в Excel он стоит в той же строке,
    // и выгрузка в Familio ждёт его там же.
    if (clergy1.iof.trim()) persons.push(payload("clergy1", 100, clergy1, "М"));
    if (clergy2.iof.trim()) persons.push(payload("clergy2", 110, clergy2, "М"));
    if (clergy3.iof.trim()) persons.push(payload("clergy3", 120, clergy3, "М"));

    // Причт в счёт не идёт: он держится между записями, и по нему нельзя
    // судить, набрал человек запись или нажал «Сохранить» вхолостую.
    const filled = [child, father.iof, mother.iof, god1.iof, god2.iof]
      .some((v) => v.trim().length > 0);
    if (!filled) {
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

  /**
   * Готовит форму к следующей записи.
   *
   * Счёт НЕ меняется сам. Счёт родившихся идёт раздельно по мальчикам
   * и девочкам, поэтому угадать следующий нельзя, а поправленное программой
   * число надо каждый раз перенабирать. Заказчик 24.08.2026: «счёт в этом поле
   * не должен меняться, он должен оставаться как последний набранный».
   *
   * Курсор уходит в «Счёт», а не в «Ребёнок»: с него начинается запись
   * в Excel, и оттуда стрелка вниз ведёт по форме до конца.
   *
   * Страница, месяц и причт остаются: они меняются реже, чем раз в запись.
   */
  function next() {
    setChild("");
    setChildParsed(null);
    setFatherState({ ...NEW_FATHER });
    setMother({ ...NEW_MOTHER });
    setGod1({ ...EMPTY_PERSON });
    setGod2({ ...EMPTY_PERSON });
    setBirthDay(null);
    setRiteDay(null);
    countField.current?.focus();
    countField.current?.select();
  }

  return (
    <>
      <section>
        <div className="row">
          <NumberField label="Стр." value={page} onChange={setPage} min={1} />
          <NumberField
            label="Счёт"
            value={count}
            onChange={setCount}
            min={1}
            inputRef={countField}
          />
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
        />
      </section>

      <PersonBlock
        title="Отец"
        person={father}
        onChange={setFather}
        rankKind="rank_m"
        withConfession
        gender="М"
        onPickPerson={pickFather}
      />
      <PersonBlock
        title="Мать"
        person={mother}
        onChange={setMother}
        rankKind="rank_f"
        withConfession
        withMaiden
        gender="Ж"
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

      {/* Церковнослужители. В Excel они стоят в той же строке записи —
          колонки 47–55 листа «1»: ИОФ, Звание, Прим., без НП
          и вероисповедания. Заказчик 24.08.2026: «нет полей для заполнения
          церковнослужителей». Прежнее решение держать причт в шапке дела
          отменено. Значения не сбрасываются между записями, поэтому набирать
          их приходится один раз на дело. */}
      <section className="person">
        <h2>Церковнослужители</h2>
        <p className="hint">
          Набранное здесь переходит в следующую запись само — причт в книге
          обычно один на всё дело.
        </p>
        <PersonBlock
          title="Первый"
          person={clergy1}
          onChange={setClergy1}
          rankKind="rank_clergy"
          gender="М"
          compact
          onPickPerson={pickRank(setClergy1)}
        />
        <PersonBlock
          title="Второй"
          person={clergy2}
          onChange={setClergy2}
          rankKind="rank_clergy"
          gender="М"
          compact
          onPickPerson={pickRank(setClergy2)}
        />
        <PersonBlock
          title="Третий"
          person={clergy3}
          onChange={setClergy3}
          rankKind="rank_clergy"
          gender="М"
          compact
          onPickPerson={pickRank(setClergy3)}
        />
      </section>

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
