import type { Case } from "./CaseHeader";

/** Общее для форм рождений, браков и смертей — что им сообщает окно. */
export type FormProps = {
  mkCase: Case;
  /** Запись сохранена — дело, к которому она привязана. */
  onSaved: (caseId: number) => void;
  /** Экран «Дело» сохранён с этим годом: форма встаёт на него. */
  workYear: { year: number; n: number } | null;
  /** «Открыть запись» из списка на сверку: раздел и номер записи. */
  openReq: { section: number; id: number; n: number } | null;
};
