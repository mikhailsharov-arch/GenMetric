/**
 * Обряд в следующем году (28.09.2026).
 *
 * Роман на вопрос о годе ответил скриншотом Excel: там один «Год» на обе
 * даты, и в его данных нет ни одной записи, где крещение или погребение
 * пришлось бы на следующий год. Угадывать программа не должна. Поэтому,
 * только когда месяц обряда меньше месяца события (декабрь → январь),
 * у дат появляется жёлтая строка с флажком «событие было в предыдущем
 * году» — решает человек. «Год» формы — год книги, в книгу запись попадает
 * по обряду, поэтому сдвигается год события, а не обряда (ревьюер #38).
 */
import { riteBeforeEvent } from "./count";
import { focusNextField } from "./focus";

type Props = {
  eventMonth: number | null;
  riteMonth: number | null;
  year: number | null;
  /** «крещение» или «погребение». */
  rite: string;
  checked: boolean;
  onChange: (v: boolean) => void;
};

export default function NextYear({ eventMonth, riteMonth, year, rite, checked, onChange }: Props) {
  if (!riteBeforeEvent(eventMonth, riteMonth)) return null;
  const event = rite === "крещение" ? "рождение" : "смерть";
  return (
    <label className="nextyear">
      <input type="checkbox" data-field checked={checked}
             onChange={(e) => onChange(e.target.checked)}
             onKeyDown={(e) => {
               // Enter и стрелки — дальше по форме, как у полей; пробел — флажок.
               if (e.key === "Enter" || e.key === "ArrowDown") {
                 e.preventDefault();
                 focusNextField(e.currentTarget, e.shiftKey && e.key === "Enter" ? -1 : 1);
               } else if (e.key === "ArrowUp") {
                 e.preventDefault();
                 focusNextField(e.currentTarget, -1);
               }
             }} />
      Месяц {rite === "крещение" ? "крещения" : "погребения"} раньше месяца {rite === "крещение" ? "рождения" : "смерти"} —
      {" "}{event} было в предыдущем году{year !== null ? ` (${year - 1})` : ""}
    </label>
  );
}
