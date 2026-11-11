import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Modal from "./Modal";
import Suggest from "./Suggest";
import { dismissWarn, report, warn } from "./errors";
import { focusNextField } from "./focus";
import { capFirst, placeLabel, placeTypeOf, splitPlaceLabel } from "./names";
import { placeTypes } from "./placetypes";

/**
 * Карточка населённого пункта при первом вводе (Роман, приоритет 2 от
 * 23.09.2026; задумано ещё 28.08: «губерния, уезд, волость, тип, чистое
 * название, ссылка на Familio»).
 *
 * Сверху — похожие из справочника: «Букарина» против «Бухарино» —
 * расхождение на третьей букве, поиск по началу молчал, и автопополнение
 * завело деревню, которой нет (заказчик 13.09.2026). Выбор похожего
 * подставляет его в поле, карточки не будет.
 *
 * Ниже — карточка: губерния и уезд по умолчанию из шапки дела, тип по
 * умолчанию «д.». Волость и ссылка — необязательны.
 */
export type Similar = { value: string; distance: number };

/** Карточка известного пункта из базы — режим правки (Роман 24.09.2026). */
export type PlaceInfo = {
  id: number; name: string; np_type: string | null; guberniya: string | null;
  uyezd: string | null; volost: string | null; familio_url: string | null; origin: string;
  /** Комментарий деревни-тёзки и название без него (08.10.2026). */
  comment: string; clean: string;
};

/** Последний заведённый или исправленный пункт: его тип, губерния, уезд и
 *  волость — в карточку нового (Роман 20.10.2026: «Чаще всего пункты идут из
 *  одной волости/уезда»). Пустое поле — как раньше, из дела. */
export type LastPlace = { np_type: string; guberniya: string; uyezd: string; volost: string };
export type LastPlaceReply = { last_volost: string; last_type?: string; last_guberniya?: string; last_uyezd?: string };
export function lastPlace(r: LastPlaceReply): LastPlace {
  return { np_type: r.last_type ?? "", guberniya: r.last_guberniya ?? "", uyezd: r.last_uyezd ?? "", volost: r.last_volost ?? "" };
}

type Props = {
  name: string;
  similar: Similar[];
  defaults: { guberniya: string; uyezd: string };
  /** Есть — правка известного пункта: поля из базы, похожих нет, place_update. */
  existing?: PlaceInfo;
  /** Волость последнего заведённого пункта — по умолчанию у нового (Роман
   *  06.10.2026): пункты одной волости заводят подряд. */
  lastVolost?: string;
  /** Последний пункт целиком — тип, губерния, уезд (у нового пункта из формы). */
  last?: LastPlace;
  /** Тип нового пункта по умолчанию: «с.» у села прихода, иначе «д.». */
  defaultType?: string;
  /** Известный пункт без единой подробности заполнять из дела, как новый, —
   *  только у карточки села прихода (её открывает экран «Дело»). У карточки
   *  из формы записи так нельзя: пункт чужого прихода получил бы уезд дела. */
  fillBare?: boolean;
  /** Новый пункт с тем же названием, что у известного (Роман 09.10.2026:
   *  «нужен понятный механизм принудительного создания новой карточки для
   *  тёзки»): `name` — чистое название, комментарий обязателен и идёт первым. */
  twin?: boolean;
  /** Кнопка «Другой пункт с тем же названием» в карточке известного пункта. */
  onTwin?: () => void;
  /** Пункты объединены (кнопка «Объединить с другим пунктом» у известного
   *  пункта): `kept` остался, `dropped` ушёл из справочника. */
  onMerged?: (kept: string, dropped: string) => void;
  onPick: (name: string) => void;
  onSaved: (name: string) => void;
  onCancel: () => void;
};

export default function PlaceCard(props: Props) {
  const [merging, setMerging] = useState(false);
  if (merging && props.existing && props.onMerged)
    return <MergePlaces mine={props.existing} onMerged={props.onMerged} onBack={() => setMerging(false)} onCancel={props.onCancel} />;
  return <PlaceForm {...props} onMerge={props.existing && props.onMerged ? () => setMerging(true) : undefined} />;
}

function PlaceForm({ name, similar, defaults, existing, lastVolost, last, defaultType, fillBare, twin, onTwin, onMerge, onPick, onSaved, onCancel }: Props & { onMerge?: () => void }) {
  // Название правится только у известного пункта (Роман 25.09.2026: «вдруг
  // пользователь допустил ошибку в названии»); у нового оно уже в заголовке.
  // Название в карточке — чистое; комментарий деревни-тёзки — отдельным
  // полем (Роман 06.10.2026). В программе пункт называется «Название
  // (комментарий)», в выгрузки идёт чистое название.
  // Новый пункт набрали сразу с комментарием — «Хмельничное (Нежитино)», по
  // образцу подсказки: хвост в скобках раскладывается в поле комментария,
  // иначе скобки ушли бы в выгрузку как часть названия (ревьюер 08.10.2026).
  // Скобки — часть названия? Человек сотрёт комментарий и допишет название.
  const label = existing ? { name: "", comment: "" } : twin ? { name, comment: "" } : splitPlaceLabel(name);
  // Тип, набранный приставкой («с.Шишкино»), — в поле «Тип» (Роман
  // 02.11.2026); название остаётся как набрано (Роман 03.11.2026: «не должна
  // обрезать исходную строку в поле „Название“… должно оставаться полное
  // введенное значение — с.Шишкино»). В выгрузку и в поиск Familio приставка
  // не идёт — её срезает `strip_place_type`. Тёзку не трогаем.
  const typedType = !existing && !twin ? placeTypeOf(label.name, placeTypes()) : null;
  const typed = label;
  const [title, setTitle] = useState(existing?.clean ?? existing?.name ?? typed.name);
  const [commentRaw, setComment] = useState(existing?.comment ?? typed.comment);
  // Название — поле и у нового пункта (Роман 16.10.2026: «при первом
  // заведении НП… отсутствует самое главное поле — само название»). Кроме
  // тёзки: его название обязано совпадать с уже заведённым.
  const titleEditable = !!existing || !twin;
  // Известный пункт без единой подробности (заведён одним названием: село
  // прихода, пункт из архива Excel) заполняется как новый — из дела (Роман
  // 06.10.2026: карточка села «остаётся пустой»). Заполненное не трогается.
  const bare = !!fillBare && !!existing
    && ![existing.np_type, existing.guberniya, existing.uyezd, existing.volost, existing.familio_url]
      .some((v) => (v ?? "").trim() !== "");
  const fresh = !existing || bare;
  // Новый пункт из формы — по последнему пункту, пустое — из дела. Село
  // прихода (карточка с экрана «Дело») — по-прежнему из дела и «с.».
  const prev = !existing && !defaultType ? last : undefined;
  const [npType, setNpType] = useState(fresh ? typedType?.type ?? defaultType ?? (prev?.np_type || "д.") : existing.np_type ?? "");
  const [guberniya, setGuberniya] = useState(fresh ? prev?.guberniya || defaults.guberniya : existing.guberniya ?? "");
  const [uyezd, setUyezd] = useState(fresh ? prev?.uyezd || defaults.uyezd : existing.uyezd ?? "");
  const [volost, setVolost] = useState(existing && !bare ? existing.volost ?? "" : existing ? "" : lastVolost ?? "");
  const [url, setUrl] = useState(existing?.familio_url ?? "");
  const [busy, setBusy] = useState(false);
  const [active, setActive] = useState(0);
  const urlField = useRef<HTMLInputElement>(null);
  const commentField = useRef<HTMLInputElement>(null);
  const titleField = useRef<HTMLInputElement>(null);

  async function save() {
    if (busy) return;
    // Тёзка без комментария — тот же пункт, что уже есть: различать нечем.
    // Комментарий набрали сразу со скобками — «(Горки)»: скобки программа
    // ставит сама, с лишними вышел бы пункт «Бухарино ((Горки))».
    const comment = commentRaw.trim().replace(/^\((.*)\)$/, "$1").trim();
    // Стёртое название — ошибка набора, а не поломка (проверяющий 16.10.2026).
    if (titleEditable && !title.trim()) {
      warn("Впишите название пункта", "без названия пункт не заводится");
      titleField.current?.focus();
      return;
    }
    if (twin && !comment) {
      warn("Впишите комментарий", "по нему программа отличит этот пункт от одноимённого — например, чей приход или волость");
      commentField.current?.focus();
      return;
    }
    setBusy(true);
    try {
      const cleanName = (titleEditable ? title : name).trim();
      const card = { name: cleanName, comment, np_type: npType, guberniya, uyezd, volost, familio_url: url };
      // Тёзка с комментарием, который уже занят: place_save молча вернул бы
      // прежний пункт, а набранные тип, уезд и волость пропали бы без слова
      // (ревьюер 09.10.2026).
      if (twin && await invoke<PlaceInfo | null>("place_get", { name: placeLabel(cleanName, comment) })) {
        warn("Такой пункт уже есть", `«${placeLabel(cleanName, comment)}» уже заведён — впишите другой комментарий`);
        commentField.current?.focus();
        return;
      }
      // Название поправили на уже заведённое («Шишкино» другого типа):
      // place_save молча вернул бы тот пункт, и запись ушла бы к деревне
      // вместо села (ревьюер 02.11.2026). Тот же тип — это он и есть; другой
      // тип — человек решает сам. С 03.11.2026 название с приставкой
      // («с.Шишкино») — своё, и уже заведённое «Шишкино» здесь не находится:
      // так хотел Роман, двойник «Шишкино» / «с.Шишкино» сливается
      // объединением пунктов.
      if (typedType && !twin && !existing) {
        // Тот же пункт тем же типом под другим написанием приставки:
        // «д.Лысое» при «Лысое» (д.), «с. Шишкино» при «с.Шишкино» — это он,
        // а не двойник (проверяющий 03.11.2026: Роман пишет то с приставкой,
        // то без).
        const bare = typedType.name;
        for (const spelled of [bare, `${typedType.type}${bare}`, `${typedType.type} ${bare}`]) {
          if (spelled === cleanName) continue;
          const same = await invoke<PlaceInfo | null>("place_get", { name: placeLabel(spelled, comment) });
          if (same && (same.np_type ?? "") === npType) {
            dismissWarn();
            onPick(same.name);
            return;
          }
        }
      }
      if (typedType && !twin) {
        const known = await invoke<PlaceInfo | null>("place_get", { name: placeLabel(cleanName, comment) });
        if (known && (known.np_type ?? "") !== npType) {
          warn("Такой пункт уже есть", `«${placeLabel(cleanName, comment)}» уже заведён с типом «${known.np_type ?? "—"}». `
            + "Если это он — поправьте тип; если другой пункт — впишите комментарий (чей приход или волость).");
          commentField.current?.focus();
          return;
        }
      }
      if (existing) await invoke("place_update", { id: existing.id, card });
      else await invoke<number>("place_save", { card });
      // Подсказка «впишите комментарий» или «такой пункт уже есть» своё отслужила.
      dismissWarn();
      onSaved(placeLabel(cleanName, comment));
    } catch (e) {
      // «Такой пункт уже есть» — не поломка, а подсказка: присылать нечего
      // (проверяющий 08.10.2026: стёрли комментарий у тёзки — вышло название
      // другого пункта, а полоса говорила «это не ваша ошибка»).
      if (String(e).includes("уже есть в справочнике")) warn("Такой пункт уже есть", String(e));
      else report(`Не удалось сохранить населённый пункт «${name}»`, e);
    } finally {
      setBusy(false);
    }
  }

  // Комментарий — для деревень с одинаковым названием: «Хмельничное» из
  // Столпина и «Хмельничное» из Нежитина. Нужен редко, поэтому вне обхода
  // Enter (без data-field): обычная карточка заполняется теми же нажатиями,
  // что и раньше. Tab и мышь в поле ведут. У карточки тёзки он обязателен и
  // стоит первым: с него карточка и начинается.
  const commentRow = (
    <div className="field">
      <label title="Для деревень с одинаковым названием. Виден в подсказке и в поле НП, в выгрузки не идёт.">Коммент.</label>
      <div className="fieldbody">
        <input data-place-comment ref={commentField} value={commentRaw} onChange={(e) => setComment(e.target.value)}
               {...(twin ? { "data-field": "", "data-autofocus": "" } : {})}
               placeholder="для тёзок: чей приход или волость" autoComplete="off" spellCheck={false}
               onKeyDown={(e) => {
                 if (e.key !== "Enter") return;
                 e.preventDefault();
                 if (twin) focusNextField(e.currentTarget, e.shiftKey ? -1 : 1);
                 else urlField.current?.focus();
               }} />
      </div>
    </div>
  );

  return (
    <Modal title={existing ? `Населённый пункт: «${name}»` : twin ? `Ещё один пункт с названием «${name}»` : `Новый населённый пункт: «${name}»`}
           kind={existing ? "place-edit" : twin ? "place-twin" : "place"} onClose={onCancel}>
      {twin && (
        <>
          <p className="hint" data-twin-hint>
            Пункт с таким названием уже есть. Впишите комментарий, по которому вы их различите, — в программе
            новый пункт будет называться «{name} (комментарий)», в выгрузки пойдёт чистое «{name}».
          </p>
          {commentRow}
        </>
      )}
      {existing && (
        <p className="hint">
          Поправьте, что нужно, включая название, — Enter ведёт по полям, на последнем
          сохраняет. Записи с этим пунктом получат новое название сами.
          {bare && " У пункта не было подробностей — тип, губерния и уезд подставлены из дела, проверьте их."}
        </p>
      )}
      {!existing && similar.length > 0 && (
        <>
          <p className="hint">Похожие названия уже есть в справочнике. Если это одно из них — Enter подставит его, карточка не понадобится. Tab — заполнить карточку нового.</p>
          {/* Список с фокусом: ↑/↓ выбирают, Enter подставляет, Tab уводит в
              карточку. Список первый в окне и получает фокус при открытии:
              «Букарина» → «Бухарино» решается одним Enter. */}
          <ul
            className="suggest static resolve"
            tabIndex={0}
            data-similar
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") { e.preventDefault(); setActive((i) => (i + 1) % similar.length); }
              else if (e.key === "ArrowUp") { e.preventDefault(); setActive((i) => (i - 1 + similar.length) % similar.length); }
              else if (e.key === "Enter") { e.preventDefault(); onPick(similar[active].value); }
            }}
          >
            {similar.map((s, i) => (
              <li
                key={s.value}
                className={i === active ? "active" : ""}
                onMouseDown={(e) => { e.preventDefault(); onPick(s.value); }}
              >
                <span className="val">{s.value}</span>
                <span className="tier t4">похоже</span>
              </li>
            ))}
          </ul>
        </>
      )}
      {!existing && !twin && (
        <p className="hint">
          {similar.length ? "Или заполните карточку нового:" : "Такого названия в справочнике нет — заполните карточку:"}
          {" "}губерния и уезд подставлены из дела, волость — от последнего заведённого пункта; ссылку можно оставить пустой.
        </p>
      )}
      {/* Скобки разложены не молча: у заказчика есть пункты, где скобки —
          часть названия («Загнетино (Поздеевка)»), и в выгрузку они идут
          (проверяющий 08.10.2026). */}
      {!existing && typed.comment !== "" && (
        <p className="hint" data-split-hint>
          <b>Скобки вынесены в комментарий:</b> в выгрузку пойдёт «{title.trim()}», а «{commentRaw.trim() || typed.comment}» останется
          пометкой для деревень-тёзок. Если скобки — часть названия, сотрите комментарий и допишите их в название.
        </p>
      )}
      {titleEditable && (
        <div className="field">
          <label>Название</label>
          <div className="fieldbody">
            <input data-field ref={titleField} value={title} onChange={(e) => setTitle(e.target.value)}
                   autoComplete="off" spellCheck={false}
                   onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); focusNextField(e.currentTarget, e.shiftKey ? -1 : 1); } }} />
          </div>
        </div>
      )}
      {/* Курсор нового пункта — в «Тип», как до поля названия: название уже
          набрано, а выделенное поле затёрлось бы первым же «д.» (ревьюер
          16.10.2026). */}
      <Suggest label="Тип" kind="np_type" value={npType} onChange={setNpType} browse autofocus={!existing && !twin && similar.length === 0} />
      {/* Первая буква — заглавная (Роман 07.10.2026). */}
      <Suggest label="Губерния" kind="guberniya" value={guberniya} onChange={setGuberniya} browse fix={capFirst} />
      <Suggest label="Уезд" kind="uyezd" value={uyezd} onChange={setUyezd} browse fix={capFirst} />
      {/* Волость — со списком уже известных; набранная в карточке губерния,
          уезд и волость запоминаются (Роман 02.10.2026). */}
      <Suggest label="Волость" kind="volost" value={volost} onChange={setVolost} browse fix={capFirst} />
      {!twin && commentRow}
      <div className="field">
        <label>Familio</label>
        <div className="fieldbody">
          <input data-field ref={urlField} value={url} onChange={(e) => setUrl(e.target.value)}
                 placeholder="ссылка, если есть" autoComplete="off" spellCheck={false}
                 onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); if (!e.ctrlKey && !e.metaKey) void save(); } }} />
        </div>
      </div>
      {/* Роман 03.10.2026, задача 3: найти пункт на Familio, чтобы взять оттуда
          ссылку. Ищем по тому, что сейчас набрано в карточке. Вне обхода
          клавишами: Enter по полям ведёт к сохранению, как раньше. */}
      <div className="familiofind">
        <button type="button" className="toggle small" tabIndex={-1} data-familio-find
                title="Откроет в браузере поиск Familio по названию, губернии, уезду и волости из карточки"
                onClick={() => {
                  invoke<string>("open_familio", {
                    name: (titleEditable ? title : name).trim(), guberniya, uyezd, volost,
                  }).catch((e) => report("Не удалось открыть поиск на Familio", e));
                  // Найденную ссылку вставляют сюда — фокус ждёт в поле.
                  urlField.current?.focus();
                }}>
          Найти на Familio ↗
        </button>
      </div>
      <div className="modalbar">
        <button type="button" className="primary" disabled={busy} onClick={() => void save()}>
          {busy ? "Сохраняю…" : existing ? "Сохранить изменения" : "Сохранить населённый пункт"}
        </button>
        {existing && onTwin && (
          <button type="button" className="toggle" data-place-twin onClick={onTwin}
                  title="Завести ещё один пункт с этим же названием — например, деревню другого прихода">
            Другой пункт с тем же названием
          </button>
        )}
        {existing && onMerge && (
          <button type="button" className="toggle" data-place-merge onClick={onMerge}
                  title="Этот пункт и другой — одно и то же (опечатка, приставка «с.»): записи обоих станут записями одного">
            Объединить с другим пунктом
          </button>
        )}
        <button type="button" className="toggle" onClick={onCancel}>{existing || twin ? "Отменить (Esc)" : "Исправить название (Esc)"}</button>
      </div>
    </Modal>
  );
}

type Merged = { kept: string; dropped: string; entries: number; cases: number; filled: string[]; differs: string[] };

const CARD_FIELDS = [["np_type", "тип"], ["guberniya", "губерния"], ["uyezd", "уезд"], ["volost", "волость"],
                     ["familio_url", "ссылка Familio"]] as const;

/** «2 строки», «5 строк». */
function rowsWord(n: number): string {
  const tail = n % 100;
  return n % 10 >= 2 && n % 10 <= 4 && (tail < 10 || tail >= 20) ? "строки" : "строк";
}

/** Подробности карточки одной строкой: «д., Макарьевский уезд, Котельская волость». */
function cardLine(p: PlaceInfo): string {
  const parts = [p.np_type, p.uyezd && `${p.uyezd} уезд`, p.volost && `${p.volost} волость`,
                 p.guberniya && `${p.guberniya} губ.`].map((v) => (v ?? "").trim()).filter(Boolean);
  return parts.length ? parts.join(", ") : "без подробностей";
}

/**
 * Объединение пунктов (спека 2026-10-20-obedinenie-punktov.md; Роман
 * 12.10.2026: «Глиница» и «Глинница» — один пункт; ответ 20.10 — только
 * кнопкой в карточке, во всех приходах). Шаги: выбрать второй пункт
 * (похожие — списком), решить, какой остаётся, объединить, прочитать отчёт.
 */
function MergePlaces({ mine, onMerged, onBack, onCancel }: {
  mine: PlaceInfo; onMerged: (kept: string, dropped: string) => void; onBack: () => void; onCancel: () => void;
}) {
  const [text, setText] = useState("");
  const [like, setLike] = useState<Similar[] | null>(null);
  const [other, setOther] = useState<{ info: PlaceInfo; usesMine: number; usesOther: number;
                                       rowsMine: number; rowsOther: number } | null>(null);
  const [keepMine, setKeepMine] = useState(true);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<Merged | null>(null);
  const asked = useRef(false);
  if (!asked.current) {
    asked.current = true;
    invoke<Similar[]>("place_merge_candidates", { name: mine.name })
      .then(setLike)
      .catch((e) => { setLike([]); report(`Не удалось найти пункты, похожие на «${mine.name}»`, e); });
  }

  async function choose(value: string) {
    const typed = value.trim();
    if (!typed || busy) return;
    setBusy(true);
    try {
      const info = await invoke<PlaceInfo | null>("place_get", { name: typed });
      if (!info) {
        warn("Такого пункта нет", `«${typed}» в справочнике нет — выберите пункт из подсказки или из списка похожих`);
        return;
      }
      if (info.id === mine.id || info.name === mine.name) {
        warn("Это тот же пункт", "выберите другой пункт — тот, с которым объединить этот");
        return;
      }
      const [usesMine, usesOther] = await invoke<[number, number]>("place_merge_uses", { a: mine.name, b: info.name });
      const [rowsMine, rowsOther] = await Promise.all([invoke<number>("place_rows", { name: mine.name }),
                                                       invoke<number>("place_rows", { name: info.name })]);
      dismissWarn();
      // По умолчанию остаётся пункт, у которого больше записей; поровну —
      // тот, чью карточку открыли.
      setKeepMine(usesMine >= usesOther);
      setOther({ info, usesMine, usesOther, rowsMine, rowsOther });
    } catch (e) {
      report(`Не удалось открыть пункт «${typed}»`, e);
    } finally {
      setBusy(false);
    }
  }

  async function merge() {
    if (!other || busy) return;
    const keep = keepMine ? mine : other.info;
    const gone = keepMine ? other.info : mine;
    setBusy(true);
    try {
      setDone(await invoke<Merged>("place_merge", { keepId: keep.id, dropId: gone.id }));
    } catch (e) {
      report(`Не удалось объединить «${gone.name}» с «${keep.name}»`, e);
    } finally {
      setBusy(false);
    }
  }

  if (done) {
    return (
      // Свой key у каждого шага: иначе React берёт тот же экземпляр окна, фокус
      // на [data-autofocus] не ставится, и Esc/Enter не работают (проверяющий
      // 03.11.2026).
      <Modal key="done" title="Пункты объединены" kind="place-merge" onClose={() => onMerged(done.kept, done.dropped)}>
        <div data-merge-done>
          <p className="hint">
            <b>«{done.dropped}» объединён с «{done.kept}».</b> Записей перешло: {done.entries}
            {done.cases > 0 && `, дел с этим селом: ${done.cases}`}. В подсказках и выгрузках теперь только «{done.kept}»;
            другие приходы переведут свои записи при открытии. Копии прихода и общего файла до объединения лежат в папке данных.
          </p>
          {done.filled.length > 0 && <p className="hint">Взято у «{done.dropped}» в пустые поля: {done.filled.join("; ")}.</p>}
          {done.differs.length > 0 && (
            <p className="hint" data-merge-differs>Расходились — оставлено своё, проверьте карточку: {done.differs.join("; ")}.</p>
          )}
        </div>
        <div className="modalbar">
          <button type="button" className="primary" data-autofocus onClick={() => onMerged(done.kept, done.dropped)}>Готово</button>
        </div>
      </Modal>
    );
  }

  if (other) {
    const keep = keepMine ? mine : other.info;
    const gone = keepMine ? other.info : mine;
    const filled: string[] = [];
    const differs: string[] = [];
    for (const [k, label] of CARD_FIELDS) {
      const a = (keep[k] ?? "").trim(), b = (gone[k] ?? "").trim();
      if (!b || a === b) continue;
      if (!a) filled.push(`${label}: ${b}`);
      else differs.push(`${label}: «${a}» против «${b}»`);
    }
    // Комментарий — часть названия, и название остаётся у оставшегося.
    if (gone.comment.trim() && gone.comment.trim() !== keep.comment.trim())
      differs.push(`комментарий «${gone.comment.trim()}» у «${gone.name}» не перейдёт`);
    // п. 121: у ушедшего несколько строк одного названия (разные уезды) —
    // уйдут все. п. 122: тёзки — комментарий обычно отличает разные деревни.
    const goneRows = keepMine ? other.rowsOther : other.rowsMine;
    const key = (v: string) => v.trim().toLowerCase().replace(/ё/g, "е");
    const twins = key(keep.clean) !== "" && key(keep.clean) === key(gone.clean)
      && keep.comment.trim() !== gone.comment.trim();
    const row = (p: PlaceInfo, uses: number, isMine: boolean) => (
      <label className="mergeopt" data-merge-keep={p.name}>
        {/* Курсор — на выборе, а не на «Объединить»: двойной Enter после
            выбора второго пункта объединял, не дав прочитать (ревьюер 03.11.2026). */}
        <input type="radio" name="merge-keep" checked={keepMine === isMine} onChange={() => setKeepMine(isMine)}
               {...(keepMine === isMine ? { "data-autofocus": "" } : {})} />
        <span><b>{p.name}</b> — записей: {uses}; {cardLine(p)}</span>
      </label>
    );
    return (
      <Modal key="choose" title={`Объединить «${mine.name}» и «${other.info.name}»`} kind="place-merge" onClose={onCancel}>
        <p className="hint">Какой пункт остаётся — его название и карточка? Второй уйдёт из справочника, его записи,
          подсказки и село в деле перейдут к оставшемуся — в этом приходе сразу, в других — при их открытии.</p>
        <div className="mergechoice">
          {row(mine, other.usesMine, true)}
          {row(other.info, other.usesOther, false)}
        </div>
        {filled.length > 0 && <p className="hint">Пустое у «{keep.name}» возьмётся у «{gone.name}»: {filled.join("; ")}.</p>}
        {differs.length > 0 && <p className="hint">Расходится — у оставшегося останется своё: {differs.join("; ")}.</p>}
        {goneRows > 1 && (
          <p className="refused" data-merge-rows>
            У «{gone.name}» в справочнике {goneRows} {rowsWord(goneRows)} с этим названием (разные уезды или типы). Уйдут все,
            их записи перейдут к «{keep.name}». Если это разные пункты — отмените и поправьте название у лишней.
          </p>
        )}
        {twins && (
          <p className="refused" data-merge-twin>
            «{keep.name}» и «{gone.name}» — тёзки: комментарий обычно отличает разные деревни. Объединяйте, только
            если это одна и та же деревня.
          </p>
        )}
        <p className="hint">Перед объединением программа сделает копии прихода и общего файла.</p>
        <div className="modalbar">
          <button type="button" className="primary" data-merge-go disabled={busy} onClick={() => void merge()}>
            {busy ? "Объединяю…" : `Объединить: остаётся «${keep.name}»`}
          </button>
          <button type="button" className="toggle" onClick={() => setOther(null)}>Назад</button>
          <button type="button" className="toggle" onClick={onCancel}>Отменить (Esc)</button>
        </div>
      </Modal>
    );
  }

  return (
    <Modal key="pick" title={`Объединить «${mine.name}» с другим пунктом`} kind="place-merge" onClose={onCancel}>
      <p className="hint">Выберите пункт, который на самом деле тот же, что «{mine.name}» (опечатка, «с.» в названии).
        Следующим шагом вы решите, какое название остаётся.</p>
      {like && like.length > 0 && (
        <>
          <p className="hint">Похожие в справочнике:</p>
          <ul className="suggest static resolve" data-merge-like>
            {like.map((s) => (
              <li key={s.value} onMouseDown={(e) => { e.preventDefault(); void choose(s.value); }}>
                <span className="val">{s.value}</span>
              </li>
            ))}
          </ul>
        </>
      )}
      {like && like.length === 0 && <p className="hint">Похожих названий нет — найдите пункт по названию.</p>}
      <div data-merge-other onKeyDown={(e) => {
        // Enter при закрытом списке подсказок — дальше; при открытом его
        // забирает список (выбор строки).
        if (e.key === "Enter" && !document.querySelector("[data-merge-other] .suggest:not(.static)")) {
          e.preventDefault();
          void choose(text);
        }
      }}>
        <Suggest label="Пункт" kind="place" value={text} onChange={setText} autofocus />
      </div>
      <div className="modalbar">
        <button type="button" className="primary" disabled={busy || !text.trim()} onClick={() => void choose(text)}>Дальше</button>
        <button type="button" className="toggle" onClick={onBack}>Назад к карточке</button>
        <button type="button" className="toggle" onClick={onCancel}>Отменить (Esc)</button>
      </div>
    </Modal>
  );
}
