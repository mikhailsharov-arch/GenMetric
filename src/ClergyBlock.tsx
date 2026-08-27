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
  const [open, setOpen] = useState(false);
  const [known, setKnown] = useState<ClergyHint[]>([]);
  const [pickerFor, setPickerFor] = useState<0 | 1 | 2 | null>(null);

  useEffect(() => {
    invoke<ClergyHint[]>("list_clergy", { limit: 20 })
      .then(setKnown)
      .catch((e) => report("Не удалось прочитать список церковнослужителей", e));
  }, [reloadKey]);

  const filled = people.filter((p) => p.iof.trim().length > 0);
  const summary = filled
    .map((p) => [p.iof, p.rank].filter(Boolean).join(", "))
    .join(" · ");

  function choose(index: 0 | 1 | 2, hint: ClergyHint) {
    onChange(index, {
      ...people[index],
      iof: hint.iof,
      rank: hint.rank ?? people[index].rank,
    });
    setPickerFor(null);
  }

  // Свёрнутый вид: одна строка вместо девяти полей. Разворачивается щелчком,
  // а пока причт пуст — открыт сам, иначе его не заполнить.
  if (!open && filled.length > 0) {
    return (
      <section className="person">
        <div className="clergyline">
          <div>
            <h2 className="inline">Церковнослужители</h2>
            <span className="clergysummary">{summary}</span>
          </div>
          <button type="button" className="linkish" onClick={() => setOpen(true)}>
            Изменить
          </button>
        </div>
      </section>
    );
  }

  return (
    <section className="person">
      <div className="clergyline">
        <h2 className="inline">Церковнослужители</h2>
        {filled.length > 0 && (
          <button type="button" className="linkish" onClick={() => setOpen(false)}>
            Свернуть
          </button>
        )}
      </div>
      <p className="hint">
        Набранное здесь переходит в следующую запись само и запоминается —
        в следующий раз причт можно выбрать из списка, а не набирать.
      </p>

      {([0, 1, 2] as const).map((i) => (
        <div key={i} className="clergyslot">
          <div className="clergyhead">
            <span className="clergynum">{TITLES[i]}</span>
            {known.length > 0 && (
              <button
                type="button"
                className="linkish"
                onClick={() => setPickerFor(pickerFor === i ? null : i)}
              >
                {pickerFor === i ? "Закрыть список" : "Выбрать из списка"}
              </button>
            )}
          </div>
          {pickerFor === i && (
            <ul className="suggest static">
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
