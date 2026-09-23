/**
 * Перевод фокуса между полями ввода.
 *
 * Заказчик просил, чтобы фокус переходил не только по Tab, но и по Enter
 * и стрелке вниз: в Excel-Индексаторе переход шёл именно клавишей «вниз»,
 * и рука к этому привыкла.
 *
 * Поля помечаются атрибутом data-field, порядок берётся из порядка в DOM —
 * то есть из порядка на экране, а он повторяет порядок чтения записи в книге.
 */
/**
 * Поля, между которыми ходит фокус. Внутри модального окна (сверка имени,
 * карточка НП) — только его поля: Enter не должен уводить из окна в форму
 * под ним. Окно помечается атрибутом data-focus-scope.
 */
function fieldsAround(current: HTMLElement): HTMLInputElement[] {
  const root: ParentNode = current.closest("[data-focus-scope]") ?? document;
  return Array.from(
    root.querySelectorAll<HTMLInputElement>("input[data-field]"),
  ).filter((el) => !el.disabled && el.offsetParent !== null);
}

export function focusNextField(current: HTMLElement, step: 1 | -1 = 1): void {
  const fields = fieldsAround(current);

  const index = fields.indexOf(current as HTMLInputElement);
  if (index === -1) return;

  const next = fields[index + step];
  if (next) {
    next.focus();
    next.select();
  }
}

/**
 * Переход к ближайшему следующему ПУСТОМУ полю.
 *
 * Нужен после выбора целой персоны из базы: он заполняет ИОФ, НП и звание
 * разом, и вести человека через уже заполненные поля по одному — три лишних
 * Enter на каждую подсказанную персону. Поправить заполненное всё равно можно:
 * стрелка вверх ведёт назад.
 *
 * Вызывать после того, как React применил новые значения (setTimeout 0):
 * пустота проверяется по DOM.
 */
export function focusNextEmptyField(current: HTMLElement): void {
  const fields = fieldsAround(current);
  const index = fields.indexOf(current as HTMLInputElement);
  if (index === -1) return;
  const next = fields.slice(index + 1).find((el) => el.value.trim() === "") ?? fields[index + 1];
  if (next) {
    next.focus();
    next.select();
  }
}
