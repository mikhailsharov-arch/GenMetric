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
export function focusNextField(current: HTMLElement, step: 1 | -1 = 1): void {
  const fields = Array.from(
    document.querySelectorAll<HTMLInputElement>("input[data-field]"),
  ).filter((el) => !el.disabled && el.offsetParent !== null);

  const index = fields.indexOf(current as HTMLInputElement);
  if (index === -1) return;

  const next = fields[index + step];
  if (next) {
    next.focus();
    next.select();
  }
}
