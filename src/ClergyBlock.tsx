import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import PersonBlock, { type Person } from "./PersonBlock";
import { report } from "./errors";
import { focusNextField } from "./focus";

/**
 * Церковнослужители записи.
 *
 * Две задачи разом, обе из отчёта заказчика от 27 августа 2026.
 *
 * ПЕРВАЯ — высота. Возврат причта в форму съел ту высоту, которую неделей
 * раньше удалось отыграть подписями слева: «все поля не умещаются по высоте
 * экрана и приходится скролить вниз чтобы нажать кнопку сохранить». Поэтому
 * заполненный причт сворачивается в одну строку. Он меняется раз в дело,
 * а занимал девять строк в каждой записи.
 *
 * ВТОРАЯ — выбор вместо набора: «следует у церковнослужителей сделать кнопку
 * выбора из выпадающего списка нужного церковника. А если в списке его нет,
 * то после ввода его руками он добавляется в базу и появляется в списке».
 * Список приходит из clergy_index и пополняется сам при сохранении записи.
 *
 * ТРЕТЬЯ (Роман 06.10 и 07–08.10.2026) — выбор по званию и с клавиатуры: «у
 * меня два священника, два дьякона и два псаломщика, и они не привязаны друг
 * к другу… Пусть будет выпадающий список, по умолчанию прошлый церковник…
 * выбор кнопками вниз/вверх, подтверждение — энтер». И распределение: «В
 * первом поле должен быть строго только „священник“ — он главный в приходе.
 * Всех остальных (включая „иерей“, „причетник“… и любые другие звания) нужно
 * выводить во втором и третьем полях». В свёрнутом причте у каждого из трёх
 * — настоящий выпадающий список в обходе клавишами; пустая первая строка —
 * «никого» (отдельная кнопка не нужна — его слова).
 */

type ClergyHint = { iof: string; rank: string | null; uses: number };

type Props = {
  people: [Person, Person, Person];
  onChange: (index: 0 | 1 | 2, p: Person) => void;
  /** Считается заново после каждого сохранения: новый причт должен появляться
   *  в списке сразу, а не после перезапуска программы. */
  reloadKey: number;
};

const TITLES = ["Первый", "Второй", "Третий"] as const;

/** Звание первого церковнослужителя — и только оно. */
const PRIEST = "священник";
const isPriest = (rank: string | null | undefined) => (rank ?? "").trim().toLowerCase() === PRIEST;

/** Кто годится на место: первое — священники, второе и третье — все остальные. */
export function clergyFor<T extends { rank: string | null }>(slot: number, known: T[]): T[] {
  return known.filter((h) => (slot === 0 ? isPriest(h.rank) : !isPriest(h.rank)));
}
const hintKey = (iof: string, rank: string | null | undefined) => `${iof.trim()}|${(rank ?? "").trim()}`;
const OPEN_KEY = "clergy_open";
const OPEN_EVENT = "genmetric:clergy-open";

export default function ClergyBlock({ people, onChange, reloadKey }: Props) {
  // Открыт, пока человек не свернул сам или не сохранил запись. Раньше блок
  // сворачивался от первой же набранной буквы — условие «есть заполненное»
  // срабатывало на каждое нажатие, и поле исчезало под руками. Нашёл стенд
  // 13.09.2026, когда попытался заполнить причт как человек.
  const [open, setOpenState] = useState(true);
  const [known, setKnown] = useState<ClergyHint[]>([]);

  // Свёрнут блок или развёрнут — помнится между запусками (Роман 30.09.2026:
  // «программа сбрасывает состояние сворачиваемого блока после перезапуска»).
  // Причт общий для рождений, браков и смертей — и состояние блока общее:
  // три формы слышат друг друга через событие окна.
  useEffect(() => {
    invoke<string | null>("get_setting", { key: OPEN_KEY })
      .then((v) => { if (v === "0" || v === "1") setOpenState(v === "1"); })
      .catch((e) => report("Не удалось узнать, свёрнут ли причт", e));
    const on = (e: Event) => setOpenState((e as CustomEvent<boolean>).detail);
    window.addEventListener(OPEN_EVENT, on);
    return () => window.removeEventListener(OPEN_EVENT, on);
  }, []);
  function setOpen(v: boolean) {
    setOpenState(v);
    window.dispatchEvent(new CustomEvent(OPEN_EVENT, { detail: v }));
    invoke("set_setting", { key: OPEN_KEY, value: v ? "1" : "0" })
      .catch((e) => report("Не удалось запомнить, свёрнут ли причт", e));
  }
  const [pickerFor, setPickerFor] = useState<0 | 1 | 2 | null>(null);

  useEffect(() => {
    invoke<ClergyHint[]>("list_clergy", { limit: 100 })
      .then(setKnown)
      .catch((e) => report("Не удалось прочитать список церковнослужителей", e));
  }, [reloadKey]);

  // После сохранения записи заполненный причт сворачивается: на следующей
  // записи он тот же, и девять полей ему ни к чему. Сменить можно из строки.
  useEffect(() => {
    // Только на экране: в настройку идёт лишь то, что человек выбрал сам
    // («Свернуть» / «Развернуть» / «Изменить»). Иначе автосворачивание
    // записывало бы «свёрнут» навсегда, и в новом сеансе пустой причт был бы
    // спрятан — а он меняется почти в каждой записи (проверяющий, 01.10.2026).
    if (reloadKey > 0 && people.some((p) => p.iof.trim())) setOpenState(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reloadKey]);

  const filled = people.filter((p) => p.iof.trim().length > 0);

  function choose(index: 0 | 1 | 2, hint: ClergyHint) {
    onChange(index, {
      ...people[index],
      iof: hint.iof,
      rank: hint.rank ?? people[index].rank,
    });
    setPickerFor(null);
  }

  /**
   * Список для выбора причта. Кнопка есть ВСЕГДА — заказчик 13.09.2026:
   * «у церковнослужителей такой кнопки нет» (после переустановки память была
   * пуста, и кнопка не показывалась) и «кнопка должна быть активна всегда».
   * Пустой список говорит, что делать, а не молчит.
   */
  const pickButton = (i: 0 | 1 | 2) => (
    <button
      type="button"
      className="linkish"
      onClick={() => setPickerFor(pickerFor === i ? null : i)}
    >
      {pickerFor === i ? "Закрыть список" : "Выбрать из списка"}
    </button>
  );
  /**
   * Выпадающий список свёрнутого причта. Стрелки меняют человека (так ведёт
   * себя список в Windows), Enter ведёт к следующему полю, Shift+Enter —
   * назад. Тот, кто стоит в записи, но в отбор не попал (набран руками, ещё
   * не сохранён; или звание не то), остаётся в списке отдельной строкой —
   * иначе список показывал бы «никого» при заполненном причте.
   */
  const select = (i: 0 | 1 | 2) => {
    const fit = clergyFor(i, known);
    const now = people[i];
    const nowKey = now.iof.trim() ? hintKey(now.iof, now.rank) : "";
    const listed = fit.some((h) => hintKey(h.iof, h.rank) === nowKey);
    return (
      <select
        data-field
        data-clergy={i}
        className="clergyselect"
        aria-label={`${TITLES[i]} церковнослужитель`}
        value={nowKey}
        onChange={(e) => {
          const key = e.target.value;
          if (key === "") {
            onChange(i, { ...now, iof: "", parsed: null, rank: "", note: "" });
            return;
          }
          const hint = fit.find((h) => hintKey(h.iof, h.rank) === key);
          // Примечание («Имя в документе: …») принадлежало прежнему человеку.
          if (hint) onChange(i, { ...now, iof: hint.iof, parsed: null, rank: hint.rank ?? "", note: "" });
        }}
        onKeyDown={(e) => {
          if (e.key !== "Enter") return;
          e.preventDefault();
          // Ctrl+Enter сохраняет запись (обработчик формы) — не мешаем.
          if (e.ctrlKey || e.metaKey) return;
          focusNextField(e.currentTarget, e.shiftKey ? -1 : 1);
        }}
      >
        <option value="">— никого —</option>
        {nowKey && !listed && (
          <option value={nowKey}>{[now.iof.trim(), now.rank.trim()].filter(Boolean).join(", ")}</option>
        )}
        {fit.map((h) => (
          <option key={hintKey(h.iof, h.rank)} value={hintKey(h.iof, h.rank)}>
            {[h.iof, h.rank].filter(Boolean).join(", ")}
          </option>
        ))}
      </select>
    );
  };
  const pickList = (i: 0 | 1 | 2) => (
    <>
      {pickerFor === i && (
        <ul className="suggest static">
          {clergyFor(i, known).length === 0 && (
            <li className="empty">
              {i === 0 ? "священников пока нет" : "пока никого"} — наберите руками, при сохранении запомнится
            </li>
          )}
          {clergyFor(i, known).map((h, k) => (
            <li key={k} onMouseDown={(e) => { e.preventDefault(); choose(i, h); }}>
              <span className="val">
                {h.iof}
                {h.rank && <span className="sub">{h.rank}</span>}
              </span>
              <span className="tier">вводили {h.uses}</span>
            </li>
          ))}
        </ul>
      )}
    </>
  );

  // Свёрнутый вид: по строке на каждого, и у каждого своя кнопка выбора —
  // сменить причт можно, не разворачивая. Заказчик: сворачивание удобно,
  // но кнопка нужна всегда. Пустой причт тоже сворачивается — заказчик
  // 23.09.2026: «бывают пользователи, которые не набирают церковнослужителей,
  // оставляя их поля пустыми, они должны иметь возможность свернуть причт,
  // чтобы он не „мозолил“ глаза». Свёрнутый пустой — одна строка.
  // …но только пока выбирать не из кого: с известным причтом свёрнутый блок —
  // три списка, даже когда все «никого». Иначе выбор «никого» у последнего
  // заполненного убирал список из-под рук вместе с фокусом (ревьюер 08.10.2026).
  if (!open && filled.length === 0 && known.length === 0) {
    return (
      <section className="person">
        <div className="clergyline">
          <h2 className="inline">Церковнослужители</h2>
          <span className="clergysummary">не указаны</span>
          <button type="button" className="linkish" onClick={() => setOpen(true)}>
            Развернуть
          </button>
        </div>
      </section>
    );
  }
  if (!open) {
    return (
      <section className="person">
        <div className="clergyline">
          <h2 className="inline">Церковнослужители</h2>
          <button type="button" className="linkish" onClick={() => setOpen(true)}>
            Изменить
          </button>
        </div>
        {([0, 1, 2] as const).map((i) => (
          <div key={i} className="clergyrow">
            <span className="clergynum">{TITLES[i]}</span>
            {select(i)}
          </div>
        ))}
      </section>
    );
  }

  return (
    <section className="person">
      {/* Подсказка — во всплывающей подписи заголовка, а каждый причт —
          строка заголовка и строка «ИОФ | Звание»: раскрытый причт был
          высоким (проверяющий 27.09.2026). */}
      <div className="clergyline">
        <h2 className="inline" title="Набранное здесь переходит в следующую запись само и запоминается — в следующий раз причт можно выбрать из списка, а не набирать.">
          Церковнослужители
        </h2>
        <button type="button" className="linkish" onClick={() => setOpen(false)}>
          Свернуть
        </button>
      </div>

      {([0, 1, 2] as const).map((i) => (
        <div key={i} className="clergyslot">
          <PersonBlock
            title={TITLES[i]}
            person={people[i]}
            onChange={(p) => onChange(i, p)}
            rankKind="rank_clergy"
            gender="М"
            compact
            titleAfter={pickButton(i)}
          />
          {pickList(i)}
        </div>
      ))}
    </section>
  );
}
