import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Suggest from "./Suggest";
import IofField, { type Parsed, type PersonHint } from "./IofField";
import PlaceCard, { type PlaceInfo, type Similar } from "./PlaceCard";
import { focusNextField } from "./focus";
import { report } from "./errors";

/**
 * Блок одной персоны в записи.
 *
 * ПОРЯДОК ПОЛЕЙ ВЗЯТ ИЗ ФОРМЫ ВВОДА EXCEL, лист «МК Ввод», раздел 1:
 * ИОФ, НП, Звание, Вероисповедание, Прим. Это ключевое требование заказчика —
 * работа не будет принята, если порядок отличается.
 *
 * Порядок не произволен и с точки зрения работы: ИОФ идёт первым потому, что
 * выбор уже занесённой персоны заполняет и населённый пункт, и звание.
 * Заказчик 17.08.2026: «как только ты выбираешь при вводе ИОФ существующую
 * персону, то и НП, и его звание, и все данные его жены тут же должны быть
 * заполнены автоматически».
 */

export type Person = {
  iof: string;
  parsed: Parsed | null;
  place: string;
  rank: string;
  confession: string;
  note: string;
  maiden: string;
};

export const EMPTY_PERSON: Person = {
  iof: "", parsed: null, place: "", rank: "", confession: "", note: "", maiden: "",
};

type Props = {
  title: string;
  person: Person;
  onChange: (p: Person) => void;
  /**
   * Перечень званий. Для причта он свой и от пола не зависит. Для остальных
   * персон выбирается по полу: «rank» означает «взять мужской или женский
   * по тому, кто перед нами».
   */
  rankKind: "rank" | "rank_clergy";
  /** Вероисповедание есть у родителей, у восприемников его в Excel нет. */
  withConfession?: boolean;
  /** Девичья фамилия — только у матери. */
  withMaiden?: boolean;
  /** Выбор персоны из базы: заполняет НП и звание, для отца ещё и мать. */
  onPickPerson?: (hint: PersonHint) => void;
  inputRef?: React.RefObject<HTMLInputElement>;
  /** Пол, заданный ролью: отец — М, мать — Ж. У восприемников роль пола
   *  не задаёт, там он определяется по имени. Нужен, чтобы отцу не предлагали
   *  женское отчество (пункт 6 отчёта от 24.08.2026). */
  gender?: "М" | "Ж";
  /**
   * Причт: только ИОФ, звание и примечание — как в Excel, где у
   * церковнослужителей нет ни населённого пункта, ни вероисповедания.
   *
   * Заодно блок рисуется без своей рамки, чтобы три причта уместились в одну
   * секцию. Высота — самый дефицитный ресурс формы: заказчик подтвердил, что
   * запись помещается на 85%, и терять это ради трёх рамок нельзя.
   */
  compact?: boolean;
  /** Губерния и уезд дела — по умолчанию в карточку нового НП. */
  placeDefaults?: { guberniya: string; uyezd: string };
  /** Пункт переименован в карточке — форма меняет старое название у всех
   *  персон записи, иначе при сохранении старое название завело бы дубль. */
  onPlaceRenamed?: (oldName: string, newName: string) => void;
  /** Рядом с заголовком — сторона поручителя («по жениху» ⇄ «по невесте»):
   *  отдельная строка «Прим.» у четырёх поручителей стоила бы четыре строки
   *  высоты (проверяющий 25.09.2026: форма браков 1908 px). */
  titleExtra?: React.ReactNode;
  /** Справа в строке заголовка, после «+ примечание» — «убрать» у
   *  пятого и шестого поручителя. */
  titleAfter?: React.ReactNode;
  /** Поля роли перед ИОФ — родство у родственника в браке (как в Excel). */
  before?: React.ReactNode;
  /** Поля роли после вероисповедания — «каким браком» и «лет» у жениха
   *  и невесты (лист «2» Excel, 25.09.2026). Стоят в одной строке с
   *  вероисповеданием, когда ширины хватает. */
  extra?: React.ReactNode;
  /** Без поля НП — родственник в браке (в Excel у него НП нет). */
  noPlace?: boolean;
  /** Умерший: младенцы первыми в подсказке ИОФ (Роман 28.09.2026). */
  preferInfant?: boolean;
  /** Подпись поля вероисповедания; в строке с «Брак» и «Лет» — короткая. */
  confessionLabel?: string;
  /** Умерший: дети из записей о рождении прихода — отдельными строками
   *  подсказки ИОФ, каждый с родителем (Роман 30.09.2026). */
  infantRows?: boolean;
  /** Год формы смертей — окно лет для этих строк. */
  infantYear?: number | null;
  /** НП для отбора этих строк по деревне; не задан — НП самой персоны. */
  infantPlace?: string | null;
  /** Без поля ИОФ — умерший, чья личность не установлена (Роман 30.09.2026). */
  noIof?: boolean;
  /** Звание по умолчанию — самое частое в приходе у этой роли и пола (Роман
   *  06.10.2026, вариант А). Не задано — не подставлять: мать, невеста,
   *  родственники, умерший и любая запись, открытая на правку. */
  defaultRank?: "father" | "groom" | "godparent" | "witness";
  /** Enter в ИОФ ведёт к первому пустому полю (мать в рождениях). */
  enterToEmpty?: boolean;
  /** Уход из поля ИОФ. */
  onIofLeave?: () => void;
};

/** Самое частое звание роли и пола; ответ держится минуту — частоты меняются
 *  медленно, а спрашивают его на каждой записи у нескольких персон. */
const rankCache = new Map<string, { at: number; value: Promise<string | null> }>();
function rankDefault(role: string, gender: string): Promise<string | null> {
  const key = `${role}|${gender}`;
  const hit = rankCache.get(key);
  if (hit && Date.now() - hit.at < 60_000) return hit.value;
  const value = invoke<string | null>("rank_default", { role, gender });
  rankCache.set(key, { at: Date.now(), value });
  // Ошибку и «ещё нет таких записей» не держим: первая же сохранённая запись
  // должна дать звание следующей.
  // Саму ошибку покажет тот, кто спрашивал (report в эффекте блока).
  value.then((v) => { if (v === null) rankCache.delete(key); }, () => { rankCache.delete(key); });
  return value;
}

/** Событие окна: пункт переименован в карточке; слушают обе формы. */
export const PLACE_RENAMED = "genmetric:place-renamed";

/** Подписка формы на переименование пункта где угодно в программе. */
export function usePlaceRenamed(apply: (oldName: string, newName: string) => void) {
  const ref = useRef(apply);
  ref.current = apply;
  useEffect(() => {
    const on = (e: Event) => {
      const d = (e as CustomEvent<{ oldName: string; newName: string }>).detail;
      ref.current(d.oldName, d.newName);
    };
    window.addEventListener(PLACE_RENAMED, on);
    return () => window.removeEventListener(PLACE_RENAMED, on);
  }, []);
}

/** Дописать пометку в примечание, не повторяя её. */
export function appendNote(note: string, add: string): string {
  if (!add || note.includes(add)) return note;
  return note.trim() ? `${note.trim()}; ${add}` : add;
}

const DOC_PREFIX = { name: "Имя в документе:", patr: "Отчество в документе:" } as const;

/** Какую пометку сверки и к какому слову поля она относится. */
export type DocFor = { name?: { word: string; note: string }; patr?: { word: string; note: string } };

/** Убрать из примечания ровно одну пометку. */
function removePart(note: string, part: string): string {
  return note.split(";").map((s) => s.trim()).filter((s) => s && s !== part).join("; ");
}

/**
 * Пометка сверки держится, пока в поле стоит то слово, ради которого она
 * написана. Сменили человека («Кесарь …» → «Иван …») — пометка «Имя в
 * документе: Пискарь» ему не принадлежит и уходит (техдолг 23.09.2026).
 * Недописанное слово — префикс прежнего — пометку не трогает.
 *
 * Трогается только та самая пометка, что дописана в этом блоке, и только
 * пока она есть в примечании: при открытии другой записи память о прежней
 * пометке сбрасывается, чужое примечание не правится (проверяющий
 * 24.09.2026 — первая версия стирала пометку записи, открытой на правку).
 */
export function staleDocNotes(note: string, iof: string, docFor: DocFor): string {
  const toks = iof.trim().split(/\s+/);
  let out = note;
  (["name", "patr"] as const).forEach((kind, i) => {
    const want = docFor[kind];
    if (!want) return;
    const parts = out.split(";").map((s) => s.trim());
    if (!parts.includes(want.note)) {
      delete docFor[kind];
      return;
    }
    const have = toks[i] ?? "";
    if (have !== want.word && !want.word.startsWith(have)) {
      out = removePart(out, want.note);
      delete docFor[kind];
    }
  });
  return out;
}

/** Запомнить, к каким словам поля относятся только что дописанные пометки. */
export function markDocNotes(iof: string, note: string, docFor: DocFor) {
  const toks = iof.trim().split(/\s+/);
  for (const part of note.split(";").map((s) => s.trim())) {
    if (part.startsWith(DOC_PREFIX.name)) docFor.name = { word: toks[0], note: part };
    if (part.startsWith(DOC_PREFIX.patr)) docFor.patr = { word: toks[1], note: part };
  }
}

export default function PersonBlock({
  title, person, onChange, rankKind, withConfession, withMaiden, onPickPerson,
  inputRef, gender, compact, placeDefaults, onPlaceRenamed, before, extra, titleExtra, noPlace, titleAfter,
  confessionLabel, preferInfant, noIof, infantRows, infantYear, infantPlace,
  defaultRank, enterToEmpty, onIofLeave,
}: Props) {
  const latest = useRef(person);
  latest.current = person;
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  /** Отдать форме новое состояние блока. Оно же сразу становится «последним
   *  известным»: две правки подряд до перерисовки (ответ разбора имени и
   *  подстановка звания) иначе брали бы за основу одно и то же старое
   *  состояние, и вторая затирала первую (стенд 06.10.2026). */
  function push(next: Person) {
    latest.current = next;
    onChangeRef.current(next);
  }
  const set = (patch: Partial<Person>) => push({ ...latest.current, ...patch });
  // Слова поля, к которым относятся пометки сверки в примечании.
  const docFor = useRef<DocFor>({});
  const Frame = compact ? "div" : "section";

  // Карточка населённого пункта при первом вводе — по уходу из поля НП.
  // После «Исправить название» (Esc) не открывается снова, пока название
  // не изменится.
  const [placeCard, setPlaceCard] = useState<{ name: string; similar: Similar[]; existing?: PlaceInfo;
                                                lastVolost?: string } | null>(null);
  const placeSkip = useRef(false);
  useEffect(() => { placeSkip.current = false; }, [person.place]);
  const placeRef = useRef<HTMLInputElement | null>(null);
  const placeNow = useRef(person.place);
  placeNow.current = person.place;

  async function checkPlace(name: string, related: EventTarget | null) {
    const text = name.trim();
    // Не при потере фокуса окном и не поверх другого окна — см. IofField.
    if ((related === null && !document.hasFocus()) || document.querySelector(".modal")) return;
    if (!text || placeSkip.current || placeCard) return;
    try {
      const r = await invoke<{ known: boolean; similar: Similar[]; last_volost: string }>("place_check", { name: text });
      // Пока ждали ответ, поле могло измениться — карточка на прежнее не нужна.
      if (r.known || placeNow.current.trim() !== text || document.querySelector(".modal")) return;
      // Форма уже скрыта (ушли на другую вкладку) — карточку не показывать.
      if (placeRef.current?.offsetParent == null) return;
      setPlaceCard({ name: text, similar: r.similar, lastVolost: r.last_volost });
    } catch (e) {
      report(`Не удалось проверить населённый пункт «${text}»`, e);
    }
  }

  /** «Карточка» у поля НП: известный пункт — на правку, иначе обычный путь. */
  async function openPlaceCard() {
    const text = person.place.trim();
    if (!text || document.querySelector(".modal")) return;
    try {
      const existing = await invoke<PlaceInfo | null>("place_get", { name: text });
      if (existing) {
        setPlaceCard({ name: existing.name, similar: [], existing });
        return;
      }
      const r = await invoke<{ known: boolean; similar: Similar[]; last_volost: string }>("place_check", { name: text });
      setPlaceCard({ name: text, similar: r.similar, lastVolost: r.last_volost });
    } catch (e) {
      report(`Не удалось открыть карточку «${text}»`, e);
    }
  }

  function placeDone(name: string) {
    setPlaceCard(null);
    set({ place: name });
    const el = placeRef.current;
    if (el) setTimeout(() => focusNextField(el), 0);
  }

  function placeCancel() {
    placeSkip.current = true;
    setPlaceCard(null);
    const el = placeRef.current;
    if (el) setTimeout(() => { el.focus(); el.setSelectionRange(el.value.length, el.value.length); }, 0);
  }

  /**
   * Пол персоны. У отца и матери его задаёт роль, у ребёнка и восприемников —
   * только набранное имя.
   *
   * От него зависят две вещи сразу: какие отчества предлагать и какой перечень
   * званий открывать. Заказчик 27.08.2026: «при вводе женщины восприемника
   * предлагает мужские звание, чего не должно быть». Раньше пол доходил только
   * до отчеств, да и то у родителей, а перечень званий у восприемников был
   * жёстко мужским.
   */
  const [noteOpen, setNoteOpen] = useState(false);
  // Новая запись — примечание снова свёрнуто (проверяющий 22.09.2026).
  useEffect(() => {
    if (!person.iof && !person.note) setNoteOpen(false);
  }, [person.iof, person.note]);
  const sex = gender ?? (person.parsed?.gender as "М" | "Ж" | undefined) ?? undefined;
  const ranks = rankKind === "rank_clergy"
    ? "rank_clergy"
    : sex === "Ж" ? "rank_f" : "rank_m";

  // --- Звание по умолчанию ---
  // Появилось имя, а звание пусто — вписываем самое частое у этой роли и
  // пола. Стёрли или набрали своё — больше не трогаем, пока блок не очистят.
  // Сменился пол (восприемник «Иван» → «Мария») — подставленное меняется.
  const autoRank = useRef<{ rank: string; sex: string } | null>(null);
  const rankTouched = useRef(false);
  const hasIof = person.iof.trim() !== "";
  useEffect(() => {
    if (!defaultRank) {
      autoRank.current = null;
      return;
    }
    const now = latest.current;
    if (!hasIof) {
      rankTouched.current = false;
      const was = autoRank.current;
      autoRank.current = null;
      if (was && now.rank === was.rank) push({ ...now, rank: "" });
      return;
    }
    const ours = autoRank.current;
    const replaceable = !now.rank || (ours !== null && now.rank === ours.rank && ours.sex !== sex);
    if (!sex || rankTouched.current || !replaceable) return;
    let alive = true;
    rankDefault(defaultRank, sex)
      .then((rank) => {
        const p = latest.current;
        const mine = autoRank.current;
        if (!alive || !p.iof.trim() || rankTouched.current) return;
        if (p.rank && !(mine && p.rank === mine.rank)) return;
        if (!rank) {
          // Для этого пола частого звания ещё нет — чужое не оставляем.
          if (mine && p.rank === mine.rank) push({ ...p, rank: "" });
          autoRank.current = null;
          return;
        }
        autoRank.current = { rank, sex };
        if (p.rank !== rank) push({ ...p, rank });
      })
      .catch((e) => report("Не удалось узнать самое частое звание", e));
    return () => { alive = false; };
  }, [defaultRank, hasIof, sex]);

  /** Простое поле без подсказок: Enter и стрелки ведут по форме дальше.
   *  Без этого блок персоны заканчивался тупиком — «Прим.» никуда не вело,
   *  и приходилось браться за мышь. */
  function plainKeys(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter" || e.key === "ArrowDown") {
      e.preventDefault();
      // Shift+Enter — назад: заказчик 15.09.2026 возвращался в пропущенное
      // поле мышью. Стрелка вверх делала это и раньше, но её не нашли.
      focusNextField(e.currentTarget, e.shiftKey && e.key === "Enter" ? -1 : 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      focusNextField(e.currentTarget, -1);
    }
  }

  /** Ссылка «+ примечание»: в заголовке, а у персон без заголовка (причт) —
   *  отдельной строкой. Пока примечание открыто или заполнено — не нужна. */
  function noteLink(inHead: boolean) {
    if (noteOpen || person.note) return null;
    return (
      <button type="button" className={inHead ? "linkish noteadd" : "linkish"} tabIndex={inHead ? -1 : undefined}
              onClick={() => setNoteOpen(true)}>
        + примечание
      </button>
    );
  }

  const iofField = (
    <IofField
        label="ИОФ"
        value={person.iof}
        onChange={(iof, parsed) => set({ iof, parsed, note: staleDocNotes(person.note, iof, docFor.current) })}
        onResolved={(iof, note) => {
          markDocNotes(iof, note, docFor.current);
          set({ iof, parsed: null, note: appendNote(person.note, note) });
        }}
        onPickPerson={onPickPerson}
        inputRef={inputRef}
        gender={sex}
        preferInfant={preferInfant}
        enterToEmpty={enterToEmpty}
        onLeave={onIofLeave}
        noSurnameMark={compact}
        infantRows={infantRows}
        infantYear={infantYear}
        infantPlace={infantPlace === undefined ? person.place : infantPlace}
      />
  );

  return (
    <Frame className={compact ? "person flat" : "person"}>
      {/* «+ примечание» — в строке заголовка, а не отдельной строкой под
          персоной: минус строка высоты у каждой персоны (Роман 27.09.2026:
          «сэкономить место за счёт дизайна»). */}
      {/* Ссылка — рядом с заголовком, но не внутри него: текст заголовка
          остаётся «Отец», по нему его находят e2e и стенды (ревьюер 27.09). */}
      {title && (
        <div className="personhead">
          {compact ? <h3>{title}{titleExtra}</h3> : <h2>{title}{titleExtra}</h2>}
          {noteLink(true)}
          {titleAfter}
        </div>
      )}
      {before}
      {!compact && !noIof && iofField}
      {/* НП и звание — парой в одну строку, когда блок шире 28em (styles.css,
          @container person). У причта (compact) пара — ИОФ | Звание: раскрытый
          причт был высоким (проверяющий 27.09.2026). */}
      <div className={!compact && !noPlace ? "pair npair" : "pair"}>
      {compact && iofField}
      {!compact && !noPlace && (
        <Suggest
          ref={placeRef}
          label="НП"
          kind="place"
          value={person.place}
          onChange={(place) => set({ place })}
          onLeave={(v, related) => void checkPlace(v, related)}
          action={{ label: "карточка", onClick: () => void openPlaceCard() }}
        />
      )}
      {placeCard && (
        <PlaceCard
          name={placeCard.name}
          similar={placeCard.similar}
          existing={placeCard.existing}
          lastVolost={placeCard.lastVolost}
          defaults={placeDefaults ?? { guberniya: "", uyezd: "" }}
          onPick={placeDone}
          onSaved={(saved) => {
            const old = placeCard.existing?.name;
            if (old && old !== saved) {
              onPlaceRenamed?.(old, saved);
              // И во все формы разом: рождения и браки живут рядом, и
              // недонабранная запись в другой форме со старым названием
              // при сохранении завела бы пункт заново (проверяющий 25.09.2026).
              window.dispatchEvent(new CustomEvent(PLACE_RENAMED, { detail: { oldName: old, newName: saved } }));
            }
            placeDone(saved);
          }}
          onCancel={placeCancel}
        />
      )}
      <Suggest
        label="Звание"
        kind={ranks}
        value={person.rank}
        onChange={(rank) => {
          // Звание тронуто руками — подстановка по умолчанию больше не вмешивается.
          if (rank !== latest.current.rank) rankTouched.current = true;
          set({ rank });
        }}
      />
      </div>
      {(withConfession || extra) && (
        <div className={withConfession && extra ? "pair trio" : "pair"}>
          {withConfession && (
            <Suggest
              label={confessionLabel ?? "Вероисповедания"}
              kind="confession"
              value={person.confession}
              onChange={(confession) => set({ confession })}
            />
          )}
          {extra}
        </div>
      )}
      {/* «Прим.» свёрнуто, пока пусто: заказчик 22.09.2026 — «строку спрятать,
          чтобы если требуется ввести примечание, строку можно было развернуть
          кнопкой/значком». Пустая строка у каждой персоны — минус высота. */}
      {(noteOpen || person.note) ? (
        <div className="field">
          <label>Прим.</label>
          <div className="fieldbody">
            <input
              data-field
              autoFocus={noteOpen && !person.note}
              value={person.note}
              onChange={(e) => set({ note: e.target.value })}
              onKeyDown={plainKeys}
              autoComplete="off"
              spellCheck={false}
            />
          </div>
        </div>
      ) : (!title && <div className="noterow">{noteLink(false)}</div>)}
      {withMaiden && (
        <div className="field">
          <label>Девичья фамилия</label>
          <div className="fieldbody">
            <input
              data-field
              value={person.maiden}
              onChange={(e) => set({ maiden: e.target.value })}
              onKeyDown={plainKeys}
              autoComplete="off"
              spellCheck={false}
            />
          </div>
        </div>
      )}
    </Frame>
  );
}
