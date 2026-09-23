import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import PersonBlock, { type Person } from "./PersonBlock";
import { report } from "./errors";

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

export default function ClergyBlock({ people, onChange, reloadKey }: Props) {
  // Открыт, пока человек не свернул сам или не сохранил запись. Раньше блок
  // сворачивался от первой же набранной буквы — условие «есть заполненное»
  // срабатывало на каждое нажатие, и поле исчезало под руками. Нашёл стенд
  // 13.09.2026, когда попытался заполнить причт как человек.
  const [open, setOpen] = useState(true);
  const [known, setKnown] = useState<ClergyHint[]>([]);
  const [pickerFor, setPickerFor] = useState<0 | 1 | 2 | null>(null);

  useEffect(() => {
    invoke<ClergyHint[]>("list_clergy", { limit: 20 })
      .then(setKnown)
      .catch((e) => report("Не удалось прочитать список церковнослужителей", e));
  }, [reloadKey]);

  // После сохранения записи заполненный причт сворачивается: на следующей
  // записи он тот же, и девять полей ему ни к чему. Сменить можно из строки.
  useEffect(() => {
    if (reloadKey > 0 && people.some((p) => p.iof.trim())) setOpen(false);
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
  const picker = (i: 0 | 1 | 2) => (
    <>
      <button
        type="button"
        className="linkish"
        onClick={() => setPickerFor(pickerFor === i ? null : i)}
      >
        {pickerFor === i ? "Закрыть список" : "Выбрать из списка"}
      </button>
      {pickerFor === i && (
        <ul className="suggest static">
          {known.length === 0 && (
            <li className="empty">пока никого — наберите руками, при сохранении запомнится</li>
          )}
          {known.map((h, k) => (
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
  if (!open && filled.length === 0) {
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
  if (!open && filled.length > 0) {
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
            <span className="clergysummary">
              {people[i].iof.trim()
                ? [people[i].iof, people[i].rank].filter(Boolean).join(", ")
                : "—"}
            </span>
            {picker(i)}
          </div>
        ))}
      </section>
    );
  }

  return (
    <section className="person">
      <div className="clergyline">
        <h2 className="inline">Церковнослужители</h2>
        <button type="button" className="linkish" onClick={() => setOpen(false)}>
          Свернуть
        </button>
      </div>
      <p className="hint">
        Набранное здесь переходит в следующую запись само и запоминается —
        в следующий раз причт можно выбрать из списка, а не набирать.
      </p>

      {([0, 1, 2] as const).map((i) => (
        <div key={i} className="clergyslot">
          <div className="clergyhead">
            <span className="clergynum">{TITLES[i]}</span>
            {picker(i)}
          </div>
          <PersonBlock
            title=""
            person={people[i]}
            onChange={(p) => onChange(i, p)}
            rankKind="rank_clergy"
            gender="М"
            compact
          />
        </div>
      ))}
    </section>
  );
}
