/**
 * Отметка «после импорта открыть список на сверку» (Роман 20.10.2026: второй
 * тестировщик не заметил список, и ошибки импорта осели в базе). Импорт
 * кончается перезагрузкой окна, поэтому отметка — в хранилище сеанса окна.
 * Оно может быть недоступно — тогда список просто не откроется сам, кнопка
 * на экране «Дело» остаётся.
 */
const MARK = "genmetric_review_after_import";

export function markReviewAfterImport() {
  try { sessionStorage.setItem(MARK, "1"); } catch { /* кнопка остаётся */ }
}

export function takeReviewMark(): boolean {
  try {
    const v = sessionStorage.getItem(MARK) === "1";
    sessionStorage.removeItem(MARK);
    return v;
  } catch {
    return false;
  }
}
