import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Suggest from "./Suggest";
import { report } from "./errors";
import { focusNextField } from "./focus";
import { dateOf, describe, family, nameless, personTail, roleKind, type DossierEvent, type Mention, type Part } from "./dossier";

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
 * Программа не связывает записи в одну личность: персона здесь — совпадение
 * ИОФ и НП. Это сказано в заголовке досье, а не только в инструкции.
 *
 * Экран только читает. Из строки — одно действие, «Открыть» (его ответ 6А).
 */

type PersonHit = { key: string; iof: string; place: string; mentions: number; year_from: number | null; year_to: number | null };
type Found = { persons: PersonHit[]; total: number };
type Dossier = { iof: string; place: string; ranks: string[]; mentions: number; events: DossierEvent[] };

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
const years = (a: number | null, b: number | null) => (a === null ? "" : a === b || b === null ? String(a) : `${a}–${b}`);

export default function SearchScreen({ active, request, onOpenEntry, onBack, backTitle }: Props) {
  const [query, setQuery] = useState("");
  const [place, setPlace] = useState("");
  // Его деление ролей: «отсеять „свои“ события человека от событий, где он
  // был просто поручителем/восприемником». Причт стоит в каждой записи.
  const [own, setOwn] = useState(true);
  const [part, setPart] = useState(true);
  const [clergy, setClergy] = useState(false);
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
  const filter = { query, place: place.trim() || null, own, part, clergy, year_from: year(yearFrom), year_to: year(yearTo) };
  const filterKey = JSON.stringify(filter);

  // Открыли из формы с персоной под курсором — её ИОФ и НП в поля.
  useEffect(() => {
    if (!request) return;
    if (request.iof.trim()) {
      setHistory([]);
      setQuery(request.iof.trim());
      setPlace(request.place.trim());
      if (request.clergy) setClergy(true);
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
  }, [picked?.key, picked?.place, own, part, clergy, yearFrom, yearTo, active]);

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
    // Причт в досье не показывается, но роль человека может быть и такой.
    if (roleKind(m.role_code) === "clergy") setClergy(true);
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
      {/* Ребёнок — не персона поиска: у него одно имя. */}
      {nameless(m) ? <span className="sub">имя не записано</span>
        : roleKind(m.role_code) === "child" ? <b>{m.iof.trim()}</b>
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
  const hasFamily = !!fam && (fam.marriages.length > 0 || fam.spouses.length > 0);
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
            <input type="checkbox" checked={own} data-role-own onChange={(e) => setOwn(e.target.checked)} />
            свои события
          </label>
          <label className="unknownbox" title="Восприемник, поручитель, родственник жениха, невесты или умершего">
            <input type="checkbox" checked={part} data-role-part onChange={(e) => setPart(e.target.checked)} />
            участие
          </label>
          <label className="unknownbox" title="Церковнослужители стоят в каждой записи — с ними список длиннее">
            <input type="checkbox" checked={clergy} data-role-clergy onChange={(e) => setClergy(e.target.checked)} />
            причт
          </label>
          <span className="searchyears">
            годы
            <input data-year-from value={yearFrom} inputMode="numeric" maxLength={4} placeholder="от" aria-label="Год от"
                   onChange={(e) => setYearFrom(e.target.value.replace(/[^0-9]/g, ""))} />
            —
            <input data-year-to value={yearTo} inputMode="numeric" maxLength={4} placeholder="до" aria-label="Год до"
                   onChange={(e) => setYearTo(e.target.value.replace(/[^0-9]/g, ""))} />
          </span>
        </div>
      </section>

      <div className="searchbody">
        <section className="searchlist" data-search-list>
          {short && <p className="hint">Наберите хотя бы две буквы имени, отчества или фамилии. Ищем по этому приходу, по всем его годам.</p>}
          {!short && found && found.persons.length === 0 && (
            <p className="hint" data-search-empty>
              Никого не нашлось{!own && !part && !clergy ? ": не отмечена ни одна группа ролей" : place.trim() ? " в этом пункте — попробуйте стереть НП" : ""}.
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
                      <span className="val">{p.iof}<span className="sub">{p.place || "без НП"}</span></span>
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
              Собрано по совпадению ИОФ и НП: упоминаний {dossier.mentions}. Программа не знает, один это человек или
              тёзки. Рождение самого человека и его родители сюда пока не попадают: в записи о рождении у ребёнка
              только имя.
            </p>

            {hasFamily && fam && (
              <div className="dossierfamily" data-family>
                <h3>Семья</h3>
                {fam.marriages.map(({ event, spouse }) => (
                  <p key={`m${event.entry_id}`}>
                    <span className="when">{when(event)}</span> брак: {spouse ? who(spouse, dossier.place) : "супруг не записан"}
                  </p>
                ))}
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

            <h3>События</h3>
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
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </section>
        )}
      </div>
    </div>
  );
}
