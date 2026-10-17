import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Suggest from "./Suggest";
import { report } from "./errors";
import { ScanLink } from "./scan";
import { focusNextField } from "./focus";
import { dateOf, describe, diedMark, family, marriageLine, nameless, personTail, roleKind, type DossierEvent, type Mention, type Part } from "./dossier";

/**
 * Экран «Поиск»: персона по приходу и её досье (спека
 * 2026-10-09-okno-poiska-persony).
 *
 * Роман 09.10.2026: «Главная цель… тотальной индексации прихода — сделать так,
 * чтобы больше никогда не листать сканы и не искать информацию глазами. Я хочу
 * просто вбить интересующего человека и мгновенно увидеть всё о нем». В Excel
 * это три фильтра подряд: ИОФ → НП → галочки ролей. Здесь — те же три отбора,
 * а результат — не таблица строк, а досье: события жизни по порядку лет.
 *
 * Программа не связывает записи в одну личность навсегда: персона здесь —
 * совпадение ИОФ и НП, к которому прикреплены собственное рождение и смерть
 * младенцем, когда связь однозначна (спека 2026-10-15-semya-celikom). Это
 * сказано в заголовке досье, а не только в инструкции.
 *
 * Экран только читает. Из строки — одно действие, «Открыть» (его ответ 6А).
 */

type PersonHit = { key: string; iof: string; place: string; mentions: number; year_from: number | null; year_to: number | null;
                  role?: string; born?: number | null; died?: number | null };
type Found = { persons: PersonHit[]; total: number };
type Dossier = { iof: string; place: string; ranks: string[]; mentions: number; events: DossierEvent[];
                 born?: number | null; died?: number | null; possible?: PersonHit[] };

/** С чем экран открыли из формы: ИОФ и НП персоны под курсором. */
export type SearchRequest = { iof: string; place: string; clergy: boolean; n: number };

type Props = {
  /** Экран на виду: скрытый не ищет и не перехватывает клавиши. */
  active: boolean;
  request: SearchRequest | null;
  onOpenEntry: (section: number, id: number) => void;
  /** Вернуться туда, откуда пришли (Esc, «К набору»). */
  onBack: () => void;
  backTitle: string;
};

type Snapshot = { query: string; place: string; picked: { key: string; place: string } | null };

const SECTION = ["", "рождение", "брак", "смерть"];
/** Роли по группам переключателей — имена как у `role_class` в search.rs. */
type RoleGroup = "own" | "part" | "clergy";
const ROLES: Record<RoleGroup, [string, string][]> = {
  own: [["child", "своё рождение"], ["groom", "жених"], ["bride", "невеста"], ["father", "отец"], ["mother", "мать"], ["deceased", "умерший"]],
  part: [["godparent", "восприемник"], ["witness", "поручитель"], ["groom_relative", "родственник жениха"],
         ["bride_relative", "родственник невесты"], ["deceased_relative", "родственник умершего"]],
  clergy: [["clergy", "причт"]],
};
const years = (a: number | null, b: number | null) => (a === null ? "" : a === b || b === null ? String(a) : `${a}–${b}`);

export default function SearchScreen({ active, request, onOpenEntry, onBack, backTitle }: Props) {
  const [query, setQuery] = useState("");
  const [place, setPlace] = useState("");
  // Его деление ролей: «отсеять „свои“ события человека от событий, где он
  // был просто поручителем/восприемником». Причт стоит в каждой записи.
  const [own, setOwn] = useState(true);
  const [part, setPart] = useState(true);
  const [clergy, setClergy] = useState(false);
  // Роли по отдельности (Роман 12.10.2026: «трех общих переключателей ролей
  // недостаточно… отдельные галочки по каждой конкретной роли (как это
  // делается в фильтрах Excel)»). Три переключателя остаются группами; здесь
  // — роли, выключенные внутри включённой группы.
  const [skip, setSkip] = useState<string[]>([]);
  const [rolesOpen, setRolesOpen] = useState(false);
  const groupOn = { own, part, clergy };
  const setGroup = { own: setOwn, part: setPart, clergy: setClergy };
  function toggleRole(group: RoleGroup, role: string) {
    const mates = ROLES[group].map(([r]) => r);
    if (!groupOn[group]) {
      // Группа выключена — включаем её с одной этой ролью.
      setGroup[group](true);
      setSkip((x) => [...x.filter((r) => !mates.includes(r)), ...mates.filter((r) => r !== role)]);
      return;
    }
    const next = skip.includes(role) ? skip.filter((r) => r !== role) : [...skip, role];
    // Выключена последняя роль группы — это выключенная группа.
    if (mates.every((r) => next.includes(r))) {
      setGroup[group](false);
      setSkip(next.filter((r) => !mates.includes(r)));
    } else setSkip(next);
  }
  /** Переключатель группы включает её целиком. */
  function toggleGroup(group: RoleGroup, on: boolean) {
    setGroup[group](on);
    setSkip((x) => x.filter((r) => !ROLES[group].some(([m]) => m === r)));
  }
  /** Человек нужен в такой-то роли (ссылка из досье, Ctrl+F из причта) — она включается. */
  function needRole(role: string) {
    const bare = role.replace(/\d+$/, "");
    // Как role_class в search.rs: родители и супруги из импорта — с родственниками.
    const cls = ({ groom_parent: "groom_relative", bride_parent: "bride_relative",
                   deceased_parent: "deceased_relative", deceased_spouse: "deceased_relative" } as Record<string, string>)[bare] ?? bare;
    const group = (Object.keys(ROLES) as RoleGroup[]).find((g) => ROLES[g].some(([r]) => r === cls));
    if (!group) return;
    if (!groupOn[group]) toggleGroup(group, true);
    else setSkip((x) => x.filter((r) => r !== cls));
  }
  const [yearFrom, setYearFrom] = useState("");
  const [yearTo, setYearTo] = useState("");

  const [found, setFound] = useState<Found | null>(null);
  const [picked, setPicked] = useState<{ key: string; place: string } | null>(null);
  const [dossier, setDossier] = useState<Dossier | null>(null);
  const [history, setHistory] = useState<Snapshot[]>([]);
  const queryField = useRef<HTMLInputElement>(null);
  /** Кого выбрать, когда придёт ответ поиска. По ссылке из досье — именно
   *  этого человека (ключ и НП): его ИОФ находит и тёзок с фамилией, и первым
   *  шёл бы самый частый из них — открывалось чужое досье (ревьюер и
   *  проверяющий 09.10.2026). Из формы (Ctrl+F) ключа нет — тот, чей ИОФ
   *  слово в слово похож на набранный. */
  const wanted = useRef<{ key?: string; place: string; typed?: string } | null>(null);

  const year = (v: string) => (/^\d{4}$/.test(v.trim()) ? Number(v.trim()) : null);
  const filter = { query, place: place.trim() || null, own, part, clergy, skip, year_from: year(yearFrom), year_to: year(yearTo) };
  const filterKey = JSON.stringify(filter);
  // Год не из четырёх цифр в отбор не идёт — и об этом сказано, а не молча
  // (проверяющий 09.10.2026); «от» больше «до» — никого не найдётся.
  const badYear = (v: string) => v.trim() !== "" && year(v) === null;
  const yearNote = badYear(yearFrom) || badYear(yearTo) ? "Год — четыре цифры: пока их меньше, отбор по этому году не действует."
    : filter.year_from !== null && filter.year_to !== null && filter.year_from > filter.year_to
      ? "Год «от» больше года «до» — так никого не найдётся." : "";

  // Открыли из формы с персоной под курсором — её ИОФ и НП в поля.
  useEffect(() => {
    if (!request) return;
    if (request.iof.trim()) {
      setHistory([]);
      setQuery(request.iof.trim());
      setPlace(request.place.trim());
      if (request.clergy) needRole("clergy");
      wanted.current = { place: request.place.trim(), typed: request.iof.trim() };
    }
    setTimeout(() => { queryField.current?.focus(); queryField.current?.select(); }, 0);
  }, [request?.n]);

  // Поиск при наборе. Ответ на прежний запрос отбрасывается; пауза — чтобы
  // не спрашивать базу на каждую букву быстрого набора.
  const seq = useRef(0);
  useEffect(() => {
    if (!active) return;
    const mine = ++seq.current;
    const timer = window.setTimeout(() => {
      invoke<Found>("search_persons", { filter })
        .then((res) => {
          if (mine !== seq.current) return;
          setFound(res);
          const want = wanted.current;
          wanted.current = null;
          const words = (t: string) => t.toLowerCase().replace(/ё/g, "е").replace(/ъ(?=\s|$)/g, "").split(/\s+/).filter(Boolean);
          // Тот же человек, что набран в форме: столько же слов, и слова
          // совпадают с точностью до окончания — в форме книжное «Петрова» и
          // «Капитонов», в списке современное «Петровна» и «Капитонович».
          const same = (x: string, y: string) => {
            let n = 0;
            while (n < x.length && n < y.length && x[n] === y[n]) n++;
            return n >= Math.max(3, Math.min(x.length, y.length) - 2);
          };
          const like = (p: PersonHit, typed: string) => {
            const a = words(p.iof), b = words(typed);
            return a.length === b.length && a.every((w, i) => same(w, b[i]));
          };
          const target = !want ? undefined
            : want.key !== undefined ? res.persons.find((p) => p.key === want.key && p.place === want.place)
            : res.persons.find((p) => (!want.place || p.place === want.place) && like(p, want.typed ?? ""));
          setPicked((now) => {
            if (target) return { key: target.key, place: target.place };
            if (now && res.persons.some((p) => p.key === now.key && p.place === now.place)) return now;
            return res.persons[0] ? { key: res.persons[0].key, place: res.persons[0].place } : null;
          });
        })
        .catch((e) => { if (mine === seq.current) report("Не удалось выполнить поиск", e); });
    }, 180);
    return () => window.clearTimeout(timer);
  }, [filterKey, active]);

  // Досье выбранной персоны — с теми же отборами по ролям и годам.
  const dseq = useRef(0);
  useEffect(() => {
    if (!active) return;
    const mine = ++dseq.current;
    if (!picked) {
      setDossier(null);
      return;
    }
    invoke<Dossier>("person_dossier", { key: picked.key, place: picked.place, filter })
      .then((d) => { if (mine === dseq.current) setDossier(d); })
      .catch((e) => { if (mine === dseq.current) report("Не удалось собрать досье", e); });
  }, [picked?.key, picked?.place, own, part, clergy, skip.join(","), yearFrom, yearTo, active]);

  // Выбранная строка — на виду в своём списке. Только при смене выбора: на
  // каждую перерисовку это дёргало бы страницу к списку, пока читают досье.
  const listRef = useRef<HTMLUListElement>(null);
  useEffect(() => {
    const el = listRef.current?.querySelector<HTMLElement>("li.on");
    const box = listRef.current;
    if (!el || !box) return;
    const a = el.getBoundingClientRect(), b = box.getBoundingClientRect();
    if (a.top < b.top) box.scrollTop -= b.top - a.top;
    else if (a.bottom > b.bottom) box.scrollTop += a.bottom - b.bottom;
  }, [picked?.key, picked?.place]);

  /** Стрелки в поле ИОФ ходят по списку найденных — рука остаётся в поле. */
  function move(step: 1 | -1) {
    const list = found?.persons ?? [];
    if (!list.length) return;
    const at = list.findIndex((p) => picked && p.key === picked.key && p.place === picked.place);
    const next = list[Math.min(list.length - 1, Math.max(0, at + step))];
    setPicked({ key: next.key, place: next.place });
  }

  /** Перейти к досье другого человека из строки — «нет ли у него брата Ивана?». */
  function go(m: Mention) {
    const nextQuery = m.iof_modern || m.iof;
    // Ссылка на того, чьё досье уже открыто, — делать нечего.
    if (picked && picked.key === m.key && picked.place === m.place) return;
    setHistory((h) => [...h, { query, place, picked }]);
    if (nextQuery === query && m.place === place) {
      // Запрос тот же (тёзка с фамилией в том же списке) — поиск не уйдёт,
      // выбираем сразу.
      setPicked({ key: m.key, place: m.place });
      return;
    }
    wanted.current = { key: m.key, place: m.place };
    setQuery(nextQuery);
    setPlace(m.place);
    // Причт в досье не показывается, но роль человека может быть и такой;
    // и роль, выключенная по отдельности, — тоже: иначе ссылка никого не найдёт.
    // Ребёнок — с 17.10.2026 тоже персона (своё рождение).
    needRole(m.role_code);
    window.scrollTo({ top: 0 });
  }
  function goBack() {
    const last = history[history.length - 1];
    if (!last) return;
    setHistory((h) => h.slice(0, -1));
    setQuery(last.query);
    setPlace(last.place);
    setPicked(last.picked);
  }

  // Esc — туда, откуда пришли. Открытый список подсказок НП первым Esc только
  // закрывается: слушаем на перехвате, пока список ещё на экране, — иначе поле
  // успевало его убрать, и тот же Esc уводил с экрана (стенд 09.10.2026).
  const onBackRef = useRef(onBack);
  onBackRef.current = onBack;
  useEffect(() => {
    if (!active) return;
    const on = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || document.querySelector(".modal, .search .suggest")) return;
      e.preventDefault();
      onBackRef.current();
    };
    document.addEventListener("keydown", on, true);
    return () => document.removeEventListener("keydown", on, true);
  }, [active]);

  const who = (m: Mention, here: string) => (
    <>
      {/* Ребёнок — тоже персона: его досье — рождение, смерть, взрослая жизнь. */}
      {nameless(m) ? <span className="sub">имя не записано</span>
        : <button type="button" className="linkish who" data-person={m.iof} title="Показать досье этого человека"
                  onClick={() => go(m)}>{m.iof}</button>}
      {personTail(m, here)}
    </>
  );
  const parts = (list: Part[], here: string) => list.map((p, i) => (
    <span key={i}>{typeof p === "string" ? p : who(p.person, here)}</span>
  ));
  const when = (e: DossierEvent) => [e.year ?? "год не указан", dateOf(e)].filter(Boolean).join(", ");

  // Как человек чаще всего записан в книге: заголовок досье — в современном
  // написании, а в записях стоит «Иван Капитонов». В строке события написание
  // показывается, только если оно отличается от этого, самого частого.
  const usual = (() => {
    const count = new Map<string, number>();
    for (const e of dossier?.events ?? []) count.set(e.me.iof, (count.get(e.me.iof) ?? 0) + 1);
    return [...count.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? "";
  })();
  const fam = dossier ? family(dossier.events) : null;
  const hasFamily = !!fam && (!!fam.parents || fam.marriages.length > 0 || fam.spouses.length > 0);
  const short = query.trim().replace(/\s+/g, "").length < 2;

  return (
    <div className="search" data-search>
      <section className="searchbar">
        <div className="searchhead">
          <h2>Поиск персоны</h2>
          {history.length > 0 && (
            <button type="button" className="linkish" data-search-back onClick={goBack}>← назад, к прежнему досье</button>
          )}
          <button type="button" className="toggle small" data-search-close onClick={onBack} title="Esc">
            {backTitle}
          </button>
        </div>
        <div className="searchfields">
          <div className="field">
            <label>ИОФ</label>
            <div className="fieldbody">
              <input ref={queryField} data-field data-search-query value={query} autoComplete="off" spellCheck={false}
                     placeholder="слова в любом порядке, можно не целиком: «кап ив»"
                     onChange={(e) => { wanted.current = null; setQuery(e.target.value); }}
                     onKeyDown={(e) => {
                       if (e.key === "ArrowDown") { e.preventDefault(); move(1); }
                       else if (e.key === "ArrowUp") { e.preventDefault(); move(-1); }
                       else if (e.key === "Enter") { e.preventDefault(); focusNextField(e.currentTarget, e.shiftKey ? -1 : 1); }
                     }} />
            </div>
          </div>
          <Suggest label="НП" kind="place" value={place} onChange={setPlace} placeholder="пусто — все пункты" />
        </div>
        <div className="searchopts">
          <label className="unknownbox" title="Он жених или невеста, отец или мать, умерший">
            <input type="checkbox" checked={own} data-role-own onChange={(e) => toggleGroup("own", e.target.checked)} />
            свои события
          </label>
          <label className="unknownbox" title="Восприемник, поручитель, родственник жениха, невесты или умершего">
            <input type="checkbox" checked={part} data-role-part onChange={(e) => toggleGroup("part", e.target.checked)} />
            участие
          </label>
          <label className="unknownbox" title="Церковнослужители стоят в каждой записи — с ними список длиннее">
            <input type="checkbox" checked={clergy} data-role-clergy onChange={(e) => toggleGroup("clergy", e.target.checked)} />
            причт
          </label>
          <button type="button" className="linkish" data-roles-toggle onClick={() => setRolesOpen((v) => !v)}>
            {rolesOpen ? "роли — свернуть" : skip.length > 0 ? `роли по отдельности (выключено: ${skip.length})` : "роли по отдельности"}
          </button>
          <span className="searchyears">
            годы
            <input data-year-from value={yearFrom} inputMode="numeric" maxLength={4} placeholder="от" aria-label="Год от"
                   className={badYear(yearFrom) ? "rejected" : undefined}
                   onChange={(e) => setYearFrom(e.target.value.replace(/[^0-9]/g, ""))} />
            —
            <input data-year-to value={yearTo} inputMode="numeric" maxLength={4} placeholder="до" aria-label="Год до"
                   className={badYear(yearTo) ? "rejected" : undefined}
                   onChange={(e) => setYearTo(e.target.value.replace(/[^0-9]/g, ""))} />
          </span>
        </div>
        {rolesOpen && (
          <div className="searchroles" data-roles>
            {(Object.keys(ROLES) as RoleGroup[]).map((group) => (
              <span key={group} className="rolegroup">
                {ROLES[group].map(([role, title]) => (
                  <label key={role} className="unknownbox">
                    <input type="checkbox" data-role={role} checked={groupOn[group] && !skip.includes(role)}
                           onChange={() => toggleRole(group, role)} />
                    {title}
                  </label>
                ))}
              </span>
            ))}
          </div>
        )}
        {yearNote && <p className="hint yearnote" data-year-note>{yearNote}</p>}
      </section>

      <div className="searchbody">
        <section className="searchlist" data-search-list>
          {short && <p className="hint">Наберите хотя бы две буквы имени, отчества или фамилии. Ищем по этому приходу, по всем его годам.</p>}
          {!short && found && found.persons.length === 0 && (
            <p className="hint" data-search-empty>
              Никого не нашлось{!own && !part && !clergy ? ": не отмечена ни одна группа ролей" : place.trim() ? " в этом пункте — попробуйте стереть НП" : ""}.
              {skip.length > 0 && ` Выключены роли: ${(Object.keys(ROLES) as RoleGroup[]).flatMap((g) => ROLES[g])
                .filter(([r]) => skip.includes(r)).map(([, t]) => t).join(", ")} — см. «роли по отдельности».`}
              {!clergy && (own || part) && " Церковнослужителей ищет флажок «причт»."}
            </p>
          )}
          {!short && found && found.persons.length > 0 && (
            <>
              <p className="hint">
                Найдено: {found.total}{found.total > found.persons.length && ` — показаны первые ${found.persons.length}, уточните запрос`}.
                {" "}Одна строка — один ИОФ в одном пункте. Стрелки ↑ ↓ в поле ИОФ ходят по списку.
              </p>
              <ul className="searchpersons" ref={listRef}>
                {found.persons.map((p) => {
                  const on = !!picked && picked.key === p.key && picked.place === p.place;
                  return (
                    <li key={`${p.key}|${p.place}`} className={on ? "on" : ""} data-hit={p.iof}
                        onMouseDown={(e) => { e.preventDefault(); setPicked({ key: p.key, place: p.place }); }}>
                      <span className="val">{p.iof}<span className="sub">{[p.place || "без НП", p.born ? `р. ${p.born}` : "", diedMark(p.died)].filter(Boolean).join(" · ")}</span></span>
                      <span className="tier">{years(p.year_from, p.year_to)} · {p.mentions}</span>
                    </li>
                  );
                })}
              </ul>
            </>
          )}
        </section>

        {dossier && dossier.mentions > 0 && (
          <section className="dossier" data-dossier>
            <h2>
              {dossier.iof}
              <span className="sub">
                {[dossier.place || "без НП",
                  years(dossier.events.find((e) => e.year !== null)?.year ?? null,
                        [...dossier.events].reverse().find((e) => e.year !== null)?.year ?? null)].filter(Boolean).join(" · ")}
              </span>
            </h2>
            <p className="hint">
              {usual && usual !== dossier.iof && <>В записях: {usual}. </>}
              {dossier.ranks.length > 0 && <>{dossier.ranks.join("; ")}. </>}
              Собрано по совпадению ИОФ и НП: упоминаний {dossier.mentions}.{" "}
              {dossier.born ? `Своё рождение (${dossier.born}) прикреплено: имя, отчество по отцу и пункт сходятся, возраст не противоречит. `
                : (dossier.possible ?? []).length > 0 ? "Своё рождение не прикреплено: подходящих несколько — см. «Возможно» ниже. " : ""}
              {dossier.died ? `Смерть — ${dossier.died}. ` : ""}
              Тёзки с тем же ИОФ в одном пункте пока собираются вместе.
            </p>

            {hasFamily && fam && (
              <div className="dossierfamily" data-family>
                <h3>Семья</h3>
                {fam.parents && (
                  <p data-parents>
                    <span className="when">{fam.parents.event.year ?? "?"}</span> родители:{" "}
                    {fam.parents.father ? <>отец — {who(fam.parents.father, dossier.place)}</> : "отец не записан"}
                    {fam.parents.mother && <>; мать — {who(fam.parents.mother, dossier.place)}</>}
                  </p>
                )}
                {fam.marriages.map(({ event, spouse }) => {
                  const line = marriageLine(event);
                  return (
                    <p key={`m${event.entry_id}`} data-marriage>
                      <span className="when">{when(event)}</span> брак{line.mine && ` (${line.mine})`}:{" "}
                      {spouse ? who(spouse, dossier.place) : line.missing}
                      {spouse && line.spouseNote && <span className="sub"> · {line.spouseNote}</span>}
                      {line.relatives.length > 0 && <span className="sub"> · {parts(line.relatives, dossier.place)}</span>}
                    </p>
                  );
                })}
                {fam.spouses.map((g, i) => (
                  <div key={i} className="children">
                    <p>
                      {g.children[0]?.event.me.role_code === "mother" ? "Отец детей" : "Мать детей"} — {g.spouse ? who(g.spouse, dossier.place)
                                        : g.children[0]?.event.me.role_code === "mother" ? "не записан" : "не записана"}. Детей: {g.children.length}.
                    </p>
                    <ul>
                      {g.children.map((c) => (
                        <li key={c.event.entry_id}>
                          <span className="when">{c.event.year ?? "?"}</span> {c.child ? who(c.child, dossier.place) : "имя не записано"}
                          {c.child?.died ? <span className="died" data-died> {diedMark(c.child.died)}</span> : null}
                          {c.godparents.length > 0 && (
                            <span className="sub"> · восприемники: {c.godparents.map((gp, k) => (
                              <span key={k}>{k > 0 && ", "}{who(gp, dossier.place)}</span>
                            ))}</span>
                          )}
                        </li>
                      ))}
                    </ul>
                  </div>
                ))}
              </div>
            )}

            <div className="dossierevents" data-events>
            <h3 className="eventshead" data-events-head>События</h3>
            <table className="events">
              <tbody>
                {dossier.events.map((e, i) => {
                  const d = describe(e);
                  const kind = roleKind(e.me.role_code);
                  return (
                    <tr key={`${e.entry_id}-${i}`} className={kind} data-event={e.me.role_code}>
                      <td className="when">{when(e)}</td>
                      <td>
                        <b className="what">{d.what}</b>{d.who.length > 0 && ". "}{parts(d.who, dossier.place)}
                        <span className="sub">
                          {" "}· {SECTION[e.section] ?? "запись"}
                          {e.me.rank && dossier.ranks[0] !== e.me.rank && ` · ${e.me.rank}`}
                          {e.me.iof !== usual && ` · в записи: ${e.me.iof}`}
                          {e.note && ` · ${e.note}`}
                        </span>
                      </td>
                      <td className="page">{e.page ? `стр. ${e.page}` : ""}</td>
                      <td>
                        <button type="button" className="linkish" data-open={e.entry_id}
                                onClick={() => onOpenEntry(e.section, e.entry_id)}>Открыть</button>
                        {/* Разворот записи — не открывая её саму. */}
                        <ScanLink file={e.scan_file} year={e.book_year ?? null} />
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            </div>

            {(dossier.possible ?? []).length > 0 && (
              <div className="dossierpossible" data-possible>
                <h3>Возможно, тот же человек</h3>
                <p className="hint">Совпадают имя, отчество и пункт, но однозначно связать нельзя — кандидатов несколько. Программа их не сливает.</p>
                <ul>
                  {(dossier.possible ?? []).map((p) => (
                    <li key={`${p.key}|${p.place}`}>
                      <button type="button" className="linkish who" data-possible-person={p.iof}
                              onClick={() => go({ key: p.key, role_code: p.role || "child", iof: p.iof, iof_modern: p.iof, place: p.place,
                                                  rank: null, gender: null, age: null, death_cause: null, marriage_order: null, kinship: null, note: null })}>
                        {p.iof}
                      </button>
                      <span className="sub"> · {[p.place || "без НП", p.born ? `р. ${p.born}` : "",
                                                  // Годы — если они не тот же год рождения или смерти.
                                                  [p.born, p.died].includes(p.year_from) && p.year_from === p.year_to ? "" : years(p.year_from, p.year_to), diedMark(p.died),
                                                  `упоминаний ${p.mentions}`].filter(Boolean).join(" · ")}</span>
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </section>
        )}
      </div>
    </div>
  );
}
