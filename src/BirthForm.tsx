import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import IofField, { type Parsed, type PersonHint } from "./IofField";
import PersonBlock, { EMPTY_PERSON, type Person } from "./PersonBlock";
import NumberField from "./NumberField";
import PageField from "./PageField";
import ClergyBlock from "./ClergyBlock";
import { splitCount, type Sex } from "./count";
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
  no_female: number | null;
  event_day: number | null;
  event_month: number | null;
  event_year: number | null;
  rite_month: number | null;
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
  // Страница — текст: «938об-939» (заказчик 21.09.2026), в базе колонка TEXT.
  const [page, setPage] = useState<string | null>(null);
  // Год — на форме, а не только в деле: «он меняется в процессе индексации»
  // (заказчик 21.09.2026). Начинается с года дела, дальше держится между
  // записями, как страница, и восстанавливается после перезапуска.
  const [year, setYear] = useState<number | null>(mkCase.year ?? null);
  const [count, setCount] = useState<number | null>(null);
  const [birthDay, setBirthDay] = useState<number | null>(null);
  const [birthMonth, setBirthMonth] = useState<number | null>(null);
  const [riteDay, setRiteDay] = useState<number | null>(null);
  const [riteMonth, setRiteMonth] = useState<number | null>(null);

  const [child, setChild] = useState("");
  const [childParsed, setChildParsed] = useState<Parsed | null>(null);
  // Пол ребёнка, указанный руками — только когда по имени его не понять.
  // Пол из разбора имени важнее: он есть у 99,7% имён на данных Романа.
  const [childSexManual, setChildSexManual] = useState<Sex | null>(null);
  // Пытались сохранить, а пол ребёнка неизвестен: подсветить выбор у поля.
  // Полоса ошибок для этого не годится — она говорит «пришлите текст Михаилу».
  const [askSex, setAskSex] = useState(false);
  const childSex: Sex | null =
    (childParsed?.gender as Sex | null | undefined) ?? childSexManual;
  const [father, setFatherState] = useState<Person>(NEW_FATHER);
  const [mother, setMother] = useState<Person>(NEW_MOTHER);
  const [god1, setGod1] = useState<Person>(EMPTY_PERSON);
  const [god2, setGod2] = useState<Person>(EMPTY_PERSON);
  // Восприемников по умолчанию два, кнопкой — до четырёх: «такое встречается
  // в метриках, и в шаблоне Familio присутствует 4 восприемника» (21.09.2026).
  const [god3, setGod3] = useState<Person>(EMPTY_PERSON);
  const [god4, setGod4] = useState<Person>(EMPTY_PERSON);
  const [godCount, setGodCount] = useState(2);

  // Причт держится между записями: в книге он один на весь разворот, а часто
  // и на всё дело. Очищать его каждую запись — заставлять набирать заново.
  const [clergy1, setClergy1] = useState<Person>(EMPTY_PERSON);
  const [clergy2, setClergy2] = useState<Person>(EMPTY_PERSON);
  const [clergy3, setClergy3] = useState<Person>(EMPTY_PERSON);

  // Меняется после каждого сохранения: список причта должен пополняться сразу.
  const [savedTimes, setSavedTimes] = useState(0);
  const [saved, setSaved] = useState<Brief[]>([]);
  const [busy, setBusy] = useState(false);
  const countField = useRef<HTMLInputElement>(null);

  useEffect(() => {
    refresh();
  }, [mkCase.id]);

  // Восстановление места работы — один раз при открытии формы.
  const restored = useRef(false);

  function refresh() {
    invoke<Brief[]>("entry_list", { caseId: mkCase.id, section: 1 })
      .then((rows) => {
        setSaved(rows);
        if (!restored.current) {
          restored.current = true;
          if (rows[0]) resume(rows[0]);
        }
      })
      .catch((e) => report("Не удалось прочитать список набранных записей", e));
  }

  /**
   * Продолжить с того места, где остановились: страница, счёт и месяцы —
   * из последней сохранённой записи дела, как после «Сохранить и следующая».
   * Заказчик 15.09.2026: «при новом открытии приложения все поля не заполнены,
   * но программа должна запоминать, над чем я работал — стр., счёт, месяц».
   * Только в пустую форму: набранное до перезапуска не трогаем.
   */
  function resume(last: Brief) {
    // Через функциональный setState: resume вызывается из ответа на запрос
    // первого рендера, и замкнутые page/count там всегда пусты — проверка
    // «форма пуста» через них была бы мёртвой (проверяющий 18.09.2026).
    setPage((v) => v ?? last.page);
    if (last.event_year !== null) setYear(last.event_year);
    // Причт — тоже (заказчик 21.09.2026). Разбор ИОФ поле сделает само.
    invoke<{ role_code: string; iof: string; rank: string | null; note: string | null }[]>(
      "last_clergy", { caseId: mkCase.id, section: 1 })
      .then((rows) => {
        const setters = { clergy1: setClergy1, clergy2: setClergy2, clergy3: setClergy3 } as const;
        for (const r of rows) {
          const set = setters[r.role_code as keyof typeof setters];
          if (set && r.iof.trim()) {
            set((p) => (p.iof.trim() ? p : { ...p, iof: r.iof, rank: r.rank ?? "", note: r.note ?? "" }));
          }
        }
        if (rows.length > 0) setSavedTimes((n) => n + 1); // свернуть заполненный причт
      })
      .catch((e) => report("Не удалось восстановить причт последней записи", e));
    setCount((v) => v ?? last.no_male ?? last.no_female ?? null);
    setBirthMonth((v) => v ?? last.event_month);
    setRiteMonth((v) => v ?? last.rite_month);
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
      // Пол, выбранный кнопками, тоже уходит в запись: по нему потом
      // отличают девочек при починке данных (migrate.sql, 21.09.2026).
      gender: childSex,
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
    if (god3.iof.trim()) persons.push(payload("godparent3", 60, god3));
    if (god4.iof.trim()) persons.push(payload("godparent4", 70, god4));
    // Причт пишется в каждую запись: в Excel он стоит в той же строке,
    // и выгрузка в Familio ждёт его там же.
    if (clergy1.iof.trim()) persons.push(payload("clergy1", 100, clergy1, "М"));
    if (clergy2.iof.trim()) persons.push(payload("clergy2", 110, clergy2, "М"));
    if (clergy3.iof.trim()) persons.push(payload("clergy3", 120, clergy3, "М"));

    // Причт в счёт не идёт: он держится между записями, и по нему нельзя
    // судить, набрал человек запись или нажал «Сохранить» вхолостую.
    const filled = [child, father.iof, mother.iof, god1.iof, god2.iof, god3.iof, god4.iof]
      .some((v) => v.trim().length > 0);
    if (!filled) {
      report("Запись пустая", "не заполнено ни имя ребёнка, ни родители");
      return;
    }

    // Счёт кладётся в колонку по полу ребёнка. Пол неизвестен и счёт есть —
    // не сохраняем и говорим почему; выбор «мальчик / девочка» стоит под полем
    // ребёнка. До 13.09.2026 здесь молча писалось в мужскую колонку.
    const columns = splitCount(count, childSex);
    if (columns === null) {
      // Кнопки выбора пола есть только под набранным именем. Счёт без имени
      // ребёнка — отдельный случай, и молчать тут нельзя (ревьюер 13.09.2026).
      const buttons = document.querySelector<HTMLButtonElement>(".sexpick button");
      if (buttons) {
        setAskSex(true);
        buttons.focus();
      } else {
        report("Счёт есть, а ребёнка нет",
               "имя ребёнка не набрано — не понять, в мужскую или женскую колонку класть счёт");
      }
      return;
    }

    setBusy(true);
    try {
      await invoke<number>("entry_save", {
        entry: {
          id: null,
          case_id: mkCase.id,
          section: 1,
          page,
          no_male: columns.no_male,
          no_female: columns.no_female,
          event_day: birthDay,
          event_month: birthMonth,
          event_year: year,
          rite_day: riteDay,
          rite_month: riteMonth,
          rite_year: year,
          note: null,
          uncertain: null,
          persons,
        },
      });
      setSavedTimes((n) => n + 1);
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
    setChildSexManual(null);
    setAskSex(false);
    setFatherState({ ...NEW_FATHER });
    setMother({ ...NEW_MOTHER });
    setGod1({ ...EMPTY_PERSON });
    setGod2({ ...EMPTY_PERSON });
    setGod3({ ...EMPTY_PERSON });
    setGod4({ ...EMPTY_PERSON });
    setGodCount(2);
    setBirthDay(null);
    setRiteDay(null);
    countField.current?.focus();
    countField.current?.select();
  }

  /**
   * Ctrl+Enter (на Маке — Cmd+Enter) сохраняет из любого поля.
   *
   * Заказчик 13.09.2026 на вопрос «где приходится браться за мышь»: «чтобы
   * нажать кнопку „сохранить и следующая“». Последнее поле формы было тупиком:
   * Enter из него никуда не вёл. Обычный Enter по-прежнему ведёт по форме.
   */
  function hotkeys(e: React.KeyboardEvent<HTMLDivElement>) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && !busy) {
      e.preventDefault();
      void save();
    }
  }

  return (
    <div onKeyDown={hotkeys}>
      <section>
        <div className="row">
          <NumberField label="Год" value={year} onChange={setYear} min={1700} max={1930} />
          <PageField label="Стр." value={page} onChange={setPage} width="9em" />
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
        {child.trim() && childParsed && !childParsed.gender && (
          <div className="field">
            <label>Пол</label>
            <div className={"fieldbody sexpick" + (askSex ? " ask" : "")}>
              <button
                type="button"
                className={childSexManual === "М" ? "on" : ""}
                onClick={() => { setChildSexManual("М"); setAskSex(false); }}
              >
                мальчик
              </button>
              <button
                type="button"
                className={childSexManual === "Ж" ? "on" : ""}
                onClick={() => { setChildSexManual("Ж"); setAskSex(false); }}
              >
                девочка
              </button>
              <span className="fieldhint">
                {askSex
                  ? "Не сохранено: выберите, мальчик это или девочка, — от пола зависит колонка счёта"
                  : "имени нет в словаре — от пола зависит колонка счёта"}
              </span>
            </div>
          </div>
        )}
      </section>

      <PersonBlock
        title="Отец"
        person={father}
        onChange={setFather}
        rankKind="rank"
        withConfession
        gender="М"
        onPickPerson={pickFather}
      />
      <PersonBlock
        title="Мать"
        person={mother}
        onChange={setMother}
        rankKind="rank"
        withConfession
        gender="Ж"
        onPickPerson={pickInto(setMother)}
      />
      <PersonBlock
        title="Восприемник 1"
        person={god1}
        onChange={setGod1}
        rankKind="rank"
        onPickPerson={pickInto(setGod1)}
      />
      <PersonBlock
        title="Восприемник 2"
        person={god2}
        onChange={setGod2}
        rankKind="rank"
        onPickPerson={pickInto(setGod2)}
      />
      {godCount >= 3 && (
        <PersonBlock
          title="Восприемник 3"
          person={god3}
          onChange={setGod3}
          rankKind="rank"
          onPickPerson={pickInto(setGod3)}
        />
      )}
      {godCount >= 4 && (
        <PersonBlock
          title="Восприемник 4"
          person={god4}
          onChange={setGod4}
          rankKind="rank"
          onPickPerson={pickInto(setGod4)}
        />
      )}
      {godCount < 4 && (
        <div className="addrow">
          <button type="button" className="toggle" onClick={() => setGodCount((n) => n + 1)}>
            Добавить восприемника
          </button>
        </div>
      )}

      {/* Церковнослужители. В Excel они стоят в той же строке записи —
          колонки 47–55 листа «1»: ИОФ, Звание, Прим., без НП
          и вероисповедания. Заполненный причт сворачивается в строку:
          он меняется раз в дело, а высота нужна каждой записи. */}
      <ClergyBlock
        people={[clergy1, clergy2, clergy3]}
        onChange={(i, p) => [setClergy1, setClergy2, setClergy3][i](p)}
        reloadKey={savedTimes}
      />

      {/* Кнопка прилипает к низу окна. Заказчик 27.08.2026: «все поля
          не умещаются по высоте экрана и приходится скролить вниз чтобы нажать
          кнопку сохранить и следующая». Прокрутка на каждой записи — это
          и время, и рука на мыши, которой он просил избегать. */}
      <div className="savebar">
        <button className="primary" onClick={save} disabled={busy} title="Ctrl+Enter">
          {busy ? "Сохраняю…" : "Сохранить и следующая"}
          <span className="kbd">Ctrl+Enter</span>
        </button>
      </div>

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
                  {/* Номер с колонкой — единственное место, где видно, что счёт
                      лёг по полу ребёнка (инцидент 13.09.2026). */}
                  <td>
                    {e.no_male !== null && `№ м. ${e.no_male}`}
                    {e.no_female !== null && `№ ж. ${e.no_female}`}
                    {e.no_male === null && e.no_female === null && "без №"}
                  </td>
                  <td>стр. {e.page ?? "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
    </div>
  );
}
