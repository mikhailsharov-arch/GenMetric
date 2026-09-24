import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import IofField, { type Parsed, type PersonHint } from "./IofField";
import PersonBlock, { EMPTY_PERSON, appendNote, markDocNotes, staleDocNotes, type DocFor, type Person } from "./PersonBlock";
import { focusNextField } from "./focus";
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
// Звание матери не с самого начала, а когда появится отец: в записи без
// отца (незаконнорождённые) «законная жена его» и НП приходилось стирать
// (Роман, приоритет № 3 от 23.09.2026). Подстановка — в setFather.
const NEW_MOTHER = { ...EMPTY_PERSON, confession: CONFESSION };

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
  father: string | null;
  clergy_noname: boolean;
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
  // Примечание записи: у ребёнка нет своего «Прим.», и пометка «Имя в
  // документе: …» после сверки идёт сюда (entry.note).
  const [entryNote, setEntryNote] = useState("");
  const childDocFor = useRef<DocFor>({});
  const placeDefaults = { guberniya: mkCase.guberniya ?? "", uyezd: mkCase.uyezd ?? "" };
  // Пол ребёнка, указанный руками — только когда по имени его не понять.
  // Пол из разбора имени важнее: он есть у 99,7% имён на данных Романа.
  const [childSexManual, setChildSexManual] = useState<Sex | null>(null);
  // Пытались сохранить, а пол ребёнка неизвестен: подсветить выбор у поля.
  // Полоса ошибок для этого не годится — она говорит «пришлите текст Михаилу».
  const [askSex, setAskSex] = useState(false);
  const [fatherRaw, setFatherState] = useState<Person>(NEW_FATHER);
  const [motherRaw, setMother] = useState<Person>(NEW_MOTHER);
  const [god1Raw, setGod1] = useState<Person>(EMPTY_PERSON);
  const [god2Raw, setGod2] = useState<Person>(EMPTY_PERSON);
  // Восприемников по умолчанию два, кнопкой — до четырёх: «такое встречается
  // в метриках, и в шаблоне Familio присутствует 4 восприемника» (21.09.2026).
  const [god3Raw, setGod3] = useState<Person>(EMPTY_PERSON);
  const [god4Raw, setGod4] = useState<Person>(EMPTY_PERSON);
  const [godCount, setGodCount] = useState(2);

  // Причт держится между записями: в книге он один на весь разворот, а часто
  // и на всё дело. Очищать его каждую запись — заставлять набирать заново.
  const [clergy1Raw, setClergy1] = useState<Person>(EMPTY_PERSON);
  const [clergy2Raw, setClergy2] = useState<Person>(EMPTY_PERSON);
  const [clergy3Raw, setClergy3] = useState<Person>(EMPTY_PERSON);
  // Вне save() персоны — как есть; save() работает с разобранными копиями.
  const father = fatherRaw, mother = motherRaw, god1 = god1Raw, god2 = god2Raw,
        god3 = god3Raw, god4 = god4Raw, clergy1 = clergy1Raw, clergy2 = clergy2Raw,
        clergy3 = clergy3Raw;

  // Меняется после каждого сохранения: список причта должен пополняться сразу.
  const [savedTimes, setSavedTimes] = useState(0);
  // Правка сохранённой записи: id открытой записи или null — новая.
  // Заказчик 22.09.2026 индексирует в программе по-настоящему; до этого
  // единственный способ поправить запись был перенабрать её.
  const [editingId, setEditingId] = useState<number | null>(null);
  // Где стояли до правки: страница, счёт, год, месяцы. После правки старой
  // записи форма возвращается сюда, а не к странице той записи.
  // Причт — тоже: в открытой записи он может быть другим (или пустым у
  // пострадавших записей), а после правки следующие записи должны идти
  // с прежним (ревьюер 22.09.2026).
  const beforeEdit = useRef<{ page: string | null; count: number | null; year: number | null;
                               birthMonth: number | null; riteMonth: number | null;
                               clergy: [Person, Person, Person] } | null>(null);
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
  // НП матери, скопированный от отца, — чтобы при стирании отца убрать
  // именно его, а не набранный руками (проверяющий 24.09.2026: уходил любой
  // НП матери, совпадающий с отцовским, и при перенаборе не возвращался).
  const copiedPlace = useRef<string | null>(null);

  function setFather(next: Person) {
    const prev = father;
    const hadFather = prev.iof.trim().length > 0;
    const hasFather = next.iof.trim().length > 0;
    if (next.place !== prev.place) {
      setMother((m) => {
        if (m.place && m.place !== prev.place) return m;
        copiedPlace.current = next.place || null;
        return { ...m, place: next.place };
      });
    }
    // Отец появился — матери «законная жена его», если звание пусто, и НП
    // отца, если у матери пусто (перенабрали отца после стирания).
    // Отца стёрли — подставленное звание и скопированный НП уходят;
    // набранное руками остаётся. Приоритет № 3 Романа от 23.09.2026.
    if (!hadFather && hasFather) {
      setMother((m) => {
        let out = m.rank ? m : { ...m, rank: MOTHER_RANK };
        if (!out.place && next.place) {
          copiedPlace.current = next.place;
          out = { ...out, place: next.place };
        }
        return out;
      });
    } else if (hadFather && !hasFather) {
      setMother((m) => {
        const copied = copiedPlace.current !== null && m.place === copiedPlace.current;
        if (copied) copiedPlace.current = null;
        return {
          ...m,
          rank: m.rank === MOTHER_RANK ? "" : m.rank,
          place: copied ? "" : m.place,
        };
      });
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
    if (hint.place) copiedPlace.current = hint.place;
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

  /**
   * Персона с ИОФ, но без разбора — так приходят причт после перезапуска
   * и все персоны при открытии записи на правку: поле в свёрнутом причте
   * не смонтировано и разобрать строку некому. Без разбора payload() уронил
   * бы имена в NULL (проверяющий 22.09.2026: так терялись имена причта
   * с 21.09). Разбираем здесь, той же командой, что и поле.
   */
  async function withParsed(p: Person): Promise<Person> {
    if (!p.iof.trim() || p.parsed) return p;
    const parsed = await invoke<Parsed>("parse_iof", { text: p.iof });
    return { ...p, parsed };
  }

  async function save() {
    let father = fatherRaw, mother = motherRaw, god1 = god1Raw, god2 = god2Raw,
        god3 = god3Raw, god4 = god4Raw, clergy1 = clergy1Raw, clergy2 = clergy2Raw,
        clergy3 = clergy3Raw;
    let parsedChild = childParsed;
    try {
      [father, mother, god1, god2, god3, god4, clergy1, clergy2, clergy3] = await Promise.all(
        [father, mother, god1, god2, god3, god4, clergy1, clergy2, clergy3].map(withParsed));
      // Ребёнок — тем же порядком: при открытии записи разбор обнуляется,
      // а сохранить можно раньше, чем поле ответит (ревьюер 22.09.2026).
      if (child.trim() && !parsedChild) parsedChild = await invoke<Parsed>("parse_iof", { text: child });
    } catch (e) {
      report("Не удалось разобрать имена перед сохранением", e);
      return;
    }
    // Имя вне справочника не сохраняется: заказчик 28.08 и 23.09.2026 —
    // «нельзя пропускать несуществующие имена». Окно сверки открывается при
    // уходе из поля; сюда доходит только то, что осталось несверенным.
    const unknown = [
      ["ребёнка", child.trim() ? parsedChild : null],
      ["отца", father.iof.trim() ? father.parsed : null],
      ["матери", mother.iof.trim() ? mother.parsed : null],
      ["восприемника", god1.iof.trim() ? god1.parsed : null],
      ["восприемника", god2.iof.trim() ? god2.parsed : null],
      ["восприемника", god3.iof.trim() ? god3.parsed : null],
      ["восприемника", god4.iof.trim() ? god4.parsed : null],
      ["причта", clergy1.iof.trim() ? clergy1.parsed : null],
      ["причта", clergy2.iof.trim() ? clergy2.parsed : null],
      ["причта", clergy3.iof.trim() ? clergy3.parsed : null],
    ].find(([, p]) => p && !(p as Parsed).known_name) as [string, Parsed] | undefined;
    if (unknown) {
      report(`Имя ${unknown[0]} «${unknown[1].first_name}» не сверено со справочником`,
             unknown[0] === "причта"
               ? "нажмите «Изменить» у причта и выйдите из поля ИОФ — откроется окно сверки (ревьюер 23.09.2026: свёрнутый причт поля не показывает)"
               : "выйдите из поля ИОФ — откроется окно сверки; выберите имя из словаря или «Новое имя»");
      return;
    }
    const sexForSave: Sex | null = (parsedChild?.gender as Sex | null | undefined) ?? childSexManual;
    const persons: PersonPayload[] = [{
      role_code: "child",
      sort_order: 10,
      surname: parsedChild?.surname ?? null,
      first_name: parsedChild?.first_name ?? null,
      patronymic: parsedChild?.patronymic ?? null,
      surname_modern: null,
      first_name_modern: parsedChild?.first_name_modern ?? null,
      patronymic_modern: parsedChild?.patronymic_modern ?? null,
      maiden_surname: null,
      // Пол, выбранный кнопками, тоже уходит в запись: по нему потом
      // отличают девочек при починке данных (migrate.sql, 21.09.2026).
      gender: sexForSave,
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
    // Год теперь только на форме; у нового дела он пуст, и запись без года
    // молча ушла бы в базу (проверяющий 22.09.2026).
    if (year === null) {
      report("Не указан год", "год записи стоит в первой строке формы — заполните его один раз, дальше он держится сам");
      document.querySelector<HTMLInputElement>(".row.tight input")?.focus();
      return;
    }
    const columns = splitCount(count, sexForSave);
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
          id: editingId,
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
          note: entryNote.trim() || null,
          uncertain: null,
          persons,
        },
      });
      setSavedTimes((n) => n + 1);
      if (editingId !== null) restoreAfterEdit();
      else next();
      refresh();
    } catch (e) {
      report(editingId ? "Не удалось сохранить изменения" : "Не удалось сохранить запись", e);
    } finally {
      setBusy(false);
    }
  }

  /** В форме уже что-то набрано — открывать поверх нельзя, потеряется. */
  function formDirty(): boolean {
    return [child, father.iof, mother.iof, god1.iof, god2.iof, god3.iof, god4.iof]
      .some((v) => v.trim().length > 0);
  }

  type MentionOut = {
    role_code: string; sort_order: number; surname: string | null; first_name: string | null;
    patronymic: string | null; gender: string | null; rank: string | null;
    confession: string | null; place: string | null; note: string | null;
  };
  type EntryFull = {
    id: number; page: string | null; no_male: number | null; no_female: number | null;
    event_day: number | null; event_month: number | null; event_year: number | null;
    rite_day: number | null; rite_month: number | null; rite_year: number | null;
    note: string | null; persons: MentionOut[];
  };

  /**
   * Поднимает сохранённую запись в форму. Разбор ИОФ поля сделают сами —
   * значения приходят строкой, как если бы их набрали.
   */
  async function openEntry(id: number) {
    if (editingId !== null && editingId !== id) {
      report("Сначала сохраните изменения или нажмите «Отменить»",
             "открыта другая запись — её правки иначе пропадут");
      return;
    }
    if (editingId === null && formDirty()) {
      report("Сначала сохраните или очистите набранное",
             "открыть запись для правки можно только с пустой формы — иначе набранное пропадёт");
      return;
    }
    try {
      const e = await invoke<EntryFull>("entry_load", { id });
      if (editingId === null) {
        beforeEdit.current = { page, count, year, birthMonth, riteMonth,
                               clergy: [clergy1, clergy2, clergy3] };
      }
      const iof = (m: MentionOut) => [m.first_name, m.patronymic, m.surname].filter(Boolean).join(" ");
      const person = (m: MentionOut | undefined, base: Person): Person =>
        m ? { ...base, iof: iof(m), parsed: null, place: m.place ?? "", rank: m.rank ?? "",
              confession: m.confession ?? base.confession, note: m.note ?? "" }
          : { ...base };
      const by = (role: string) => e.persons.find((m) => m.role_code === role);
      const ch = by("child");
      setChild(ch ? iof(ch) : "");
      setChildParsed(null);
      setChildSexManual((ch?.gender as Sex | null | undefined) ?? null);
      setAskSex(false);
      setPage(e.page);
      setCount(e.no_male ?? e.no_female ?? null);
      setBirthDay(e.event_day);
      setBirthMonth(e.event_month);
      if (e.event_year !== null) setYear(e.event_year);
      setRiteDay(e.rite_day);
      setRiteMonth(e.rite_month);
      setEntryNote(e.note ?? "");
      setFatherState(person(by("father"), { ...NEW_FATHER }));
      setMother(person(by("mother"), { ...NEW_MOTHER }));
      copiedPlace.current = null;
      childDocFor.current = {};
      setGod1(person(by("godparent1"), { ...EMPTY_PERSON }));
      setGod2(person(by("godparent2"), { ...EMPTY_PERSON }));
      setGod3(person(by("godparent3"), { ...EMPTY_PERSON }));
      setGod4(person(by("godparent4"), { ...EMPTY_PERSON }));
      setGodCount(by("godparent4") ? 4 : by("godparent3") ? 3 : 2);
      setClergy1(person(by("clergy1"), { ...EMPTY_PERSON }));
      setClergy2(person(by("clergy2"), { ...EMPTY_PERSON }));
      setClergy3(person(by("clergy3"), { ...EMPTY_PERSON }));
      setEditingId(e.id);
      window.scrollTo({ top: 0 });
      countField.current?.focus();
    } catch (err) {
      report("Не удалось открыть запись", err);
    }
  }

  /** Вернуться туда, где стояли до правки. */
  function restoreAfterEdit() {
    const b = beforeEdit.current;
    beforeEdit.current = null;
    setEditingId(null);
    next();
    if (b) {
      setPage(b.page);
      setCount(b.count);
      setYear(b.year);
      setBirthMonth(b.birthMonth);
      setRiteMonth(b.riteMonth);
      setClergy1(b.clergy[0]);
      setClergy2(b.clergy[1]);
      setClergy3(b.clergy[2]);
    }
  }

  /** Отменить правку: форма пустая, запись в базе не тронута. */
  function cancelEdit() {
    restoreAfterEdit();
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
    copiedPlace.current = null;
    childDocFor.current = {};
    setChild("");
    setChildParsed(null);
    setEntryNote("");
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
      {editingId !== null && (
        <div className="editbar">
          <b>Правка записи</b> — сохранённая запись открыта в форме. «Сохранить
          изменения» перепишет её; «Отменить» оставит как была.
          <button type="button" className="toggle" onClick={cancelEdit}>Отменить</button>
        </div>
      )}
      <section>
        <div className="row tight">
          <NumberField label="Год" value={year} onChange={setYear} min={1700} max={1930} />
          <PageField label="Стр." value={page} onChange={setPage} />
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
            setEntryNote((n) => staleDocNotes(n, text, childDocFor.current));
          }}
          placeholder="имя"
          onResolved={(text, note) => {
            markDocNotes(text, note, childDocFor.current);
            setChild(text);
            setChildParsed(null);
            setEntryNote((n) => appendNote(n, note));
          }}
        />
        {entryNote && (
          <div className="field">
            <label>Прим.</label>
            <div className="fieldbody">
              <input data-field value={entryNote} onChange={(e) => setEntryNote(e.target.value)}
                     autoComplete="off" spellCheck={false}
                     onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); focusNextField(e.currentTarget, e.shiftKey ? -1 : 1); } }} />
            </div>
          </div>
        )}
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
        placeDefaults={placeDefaults}
        withConfession
        gender="М"
        onPickPerson={pickFather}
      />
      <PersonBlock
        title="Мать"
        person={mother}
        onChange={setMother}
        rankKind="rank"
        placeDefaults={placeDefaults}
        withConfession
        gender="Ж"
        onPickPerson={pickInto(setMother)}
      />
      <PersonBlock
        title="Восприемник 1"
        person={god1}
        onChange={setGod1}
        rankKind="rank"
        placeDefaults={placeDefaults}
        onPickPerson={pickInto(setGod1)}
      />
      <PersonBlock
        title="Восприемник 2"
        person={god2}
        onChange={setGod2}
        rankKind="rank"
        placeDefaults={placeDefaults}
        onPickPerson={pickInto(setGod2)}
      />
      {godCount >= 3 && (
        <PersonBlock
          title="Восприемник 3"
          person={god3}
          onChange={setGod3}
          rankKind="rank"
        placeDefaults={placeDefaults}
          onPickPerson={pickInto(setGod3)}
        />
      )}
      {godCount >= 4 && (
        <PersonBlock
          title="Восприемник 4"
          person={god4}
          onChange={setGod4}
          rankKind="rank"
        placeDefaults={placeDefaults}
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
          {busy ? "Сохраняю…" : editingId !== null ? "Сохранить изменения" : "Сохранить и следующая"}
          <span className="kbd">Ctrl+Enter</span>
        </button>
      </div>

      {saved.length > 0 && (
        <section>
          <h2>Набрано: {saved.length}</h2>
          <p className="hint">Нажмите «Открыть», чтобы поправить запись. Форма при этом должна быть пустой.</p>
          <table className="facts saved">
            <tbody>
              {saved.map((e) => (
                <tr key={e.id} className={e.id === editingId ? "editing" : ""}>
                  <td>
                    {e.event_day ?? "?"}.{e.event_month ?? "?"} · {e.child || "без имени"}
                    {/* Отец в строке: правка отца иначе в списке не видна (Роман 23.09.2026). */}
                    {e.father && <span className="sub"> · отец {e.father}</span>}
                  </td>
                  {/* Номер с колонкой — единственное место, где видно, что счёт
                      лёг по полу ребёнка (инцидент 13.09.2026). */}
                  <td>
                    {e.no_male !== null && `№ м. ${e.no_male}`}
                    {e.no_female !== null && `№ ж. ${e.no_female}`}
                    {e.no_male === null && e.no_female === null && "без №"}
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
