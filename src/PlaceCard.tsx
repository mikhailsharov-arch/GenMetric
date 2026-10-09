import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Modal from "./Modal";
import Suggest from "./Suggest";
import { dismissWarn, report, warn } from "./errors";
import { focusNextField } from "./focus";
import { capFirst, placeLabel, splitPlaceLabel } from "./names";

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

type Props = {
  name: string;
  similar: Similar[];
  defaults: { guberniya: string; uyezd: string };
  /** Есть — правка известного пункта: поля из базы, похожих нет, place_update. */
  existing?: PlaceInfo;
  /** Волость последнего заведённого пункта — по умолчанию у нового (Роман
   *  06.10.2026): пункты одной волости заводят подряд. */
  lastVolost?: string;
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
  onPick: (name: string) => void;
  onSaved: (name: string) => void;
  onCancel: () => void;
};

export default function PlaceCard({ name, similar, defaults, existing, lastVolost, defaultType, fillBare, twin, onTwin, onPick, onSaved, onCancel }: Props) {
  // Название правится только у известного пункта (Роман 25.09.2026: «вдруг
  // пользователь допустил ошибку в названии»); у нового оно уже в заголовке.
  // Название в карточке — чистое; комментарий деревни-тёзки — отдельным
  // полем (Роман 06.10.2026). В программе пункт называется «Название
  // (комментарий)», в выгрузки идёт чистое название.
  // Новый пункт набрали сразу с комментарием — «Хмельничное (Нежитино)», по
  // образцу подсказки: хвост в скобках раскладывается в поле комментария,
  // иначе скобки ушли бы в выгрузку как часть названия (ревьюер 08.10.2026).
  // Скобки — часть названия? Человек сотрёт комментарий и допишет название.
  const typed = existing ? { name: "", comment: "" } : twin ? { name, comment: "" } : splitPlaceLabel(name);
  const [title, setTitle] = useState(existing?.clean ?? existing?.name ?? typed.name);
  const [commentRaw, setComment] = useState(existing?.comment ?? typed.comment);
  // Название нового пункта правится в карточке, только если его разложили.
  const titleEditable = !!existing || typed.comment !== "";
  // Известный пункт без единой подробности (заведён одним названием: село
  // прихода, пункт из архива Excel) заполняется как новый — из дела (Роман
  // 06.10.2026: карточка села «остаётся пустой»). Заполненное не трогается.
  const bare = !!fillBare && !!existing
    && ![existing.np_type, existing.guberniya, existing.uyezd, existing.volost, existing.familio_url]
      .some((v) => (v ?? "").trim() !== "");
  const fresh = !existing || bare;
  const [npType, setNpType] = useState(fresh ? defaultType ?? "д." : existing.np_type ?? "");
  const [guberniya, setGuberniya] = useState(fresh ? defaults.guberniya : existing.guberniya ?? "");
  const [uyezd, setUyezd] = useState(fresh ? defaults.uyezd : existing.uyezd ?? "");
  const [volost, setVolost] = useState(existing && !bare ? existing.volost ?? "" : existing ? "" : lastVolost ?? "");
  const [url, setUrl] = useState(existing?.familio_url ?? "");
  const [busy, setBusy] = useState(false);
  const [active, setActive] = useState(0);
  const urlField = useRef<HTMLInputElement>(null);
  const commentField = useRef<HTMLInputElement>(null);

  async function save() {
    if (busy) return;
    // Тёзка без комментария — тот же пункт, что уже есть: различать нечем.
    // Комментарий набрали сразу со скобками — «(Горки)»: скобки программа
    // ставит сама, с лишними вышел бы пункт «Бухарино ((Горки))».
    const comment = commentRaw.trim().replace(/^\((.*)\)$/, "$1").trim();
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
            <input data-field value={title} onChange={(e) => setTitle(e.target.value)}
                   autoComplete="off" spellCheck={false}
                   onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); focusNextField(e.currentTarget, e.shiftKey ? -1 : 1); } }} />
          </div>
        </div>
      )}
      <Suggest label="Тип" kind="np_type" value={npType} onChange={setNpType} browse />
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
        <button type="button" className="toggle" onClick={onCancel}>{existing || twin ? "Отменить (Esc)" : "Исправить название (Esc)"}</button>
      </div>
    </Modal>
  );
}
