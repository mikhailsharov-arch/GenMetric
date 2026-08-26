import Suggest from "./Suggest";
import IofField, { type Parsed, type PersonHint } from "./IofField";

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
  /** Мужской, женский или причтовый перечень званий — разные справочники. */
  rankKind: "rank_m" | "rank_f" | "rank_clergy";
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
};

export default function PersonBlock({
  title, person, onChange, rankKind, withConfession, withMaiden, onPickPerson,
  inputRef, gender, compact,
}: Props) {
  const set = (patch: Partial<Person>) => onChange({ ...person, ...patch });
  const Frame = compact ? "div" : "section";

  return (
    <Frame className={compact ? "person flat" : "person"}>
      {compact ? <h3>{title}</h3> : <h2>{title}</h2>}
      <IofField
        label="ИОФ"
        value={person.iof}
        onChange={(iof, parsed) => set({ iof, parsed })}
        onPickPerson={onPickPerson}
        inputRef={inputRef}
        gender={gender}
      />
      {!compact && (
        <Suggest
          label="НП"
          kind="place"
          value={person.place}
          onChange={(place) => set({ place })}
        />
      )}
      <Suggest
        label="Звание"
        kind={rankKind}
        value={person.rank}
        onChange={(rank) => set({ rank })}
      />
      {withConfession && (
        <Suggest
          label="Вероисповедания"
          kind="confession"
          value={person.confession}
          onChange={(confession) => set({ confession })}
        />
      )}
      <div className="field">
        <label>Прим.</label>
        <div className="fieldbody">
          <input
            data-field
            value={person.note}
            onChange={(e) => set({ note: e.target.value })}
            autoComplete="off"
            spellCheck={false}
          />
        </div>
      </div>
      {withMaiden && (
        <div className="field">
          <label>Девичья фамилия</label>
          <div className="fieldbody">
            <input
              data-field
              value={person.maiden}
              onChange={(e) => set({ maiden: e.target.value })}
              autoComplete="off"
              spellCheck={false}
            />
          </div>
        </div>
      )}
    </Frame>
  );
}
