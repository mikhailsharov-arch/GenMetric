import type { Case } from "./CaseHeader";

/** Общее для форм рождений, браков и смертей — что им сообщает окно. */
export type FormProps = {
  mkCase: Case;
  /** Запись сохранена — дело, к которому она привязана. */
  onSaved: (caseId: number) => void;
  /** Экран «Дело» сохранён с этим годом: форма встаёт на него. */
  workYear: { year: number; n: number; fresh?: boolean } | null;
  /** «Открыть запись» из списка на сверку: раздел и номер записи. */
  openReq: { section: number; id: number; n: number } | null;
  /** Год на форме сменили: если в нём ещё нет записей, все формы встают на
   *  него с чистого листа (Роман 19.10.2026). Ответ — новый ли это год. */
  onNewYear?: (year: number) => Promise<boolean>;
};
