/**
 * Несохранённое в формах — для смены прихода (спека 2026-10-02, п. 1.4).
 *
 * Смена прихода перечитывает окно целиком, и набранная, но не сохранённая
 * запись пропала бы молча. Формы отмечают здесь, что в них что-то набрано
 * или открыта запись на правку; окно «Приходы» перед сменой спрашивает.
 */
const dirty = new Map<string, boolean>();

export function setDirty(form: string, value: boolean): void {
  dirty.set(form, value);
}

/** Названия форм, где есть несохранённое. */
export function dirtyForms(): string[] {
  return [...dirty].filter(([, v]) => v).map(([k]) => k);
}

/**
 * «Очистить» прямо в отказе окна «Приходы» (Роман 03.11.2026: «добавить
 * кнопку „Очистить“…, которая одним кликом очистит несохраненные поля в форме
 * и сразу разблокирует смену/создание прихода»). Форма отдаёт сюда, как себя
 * очистить: новая запись — как «следующая», правка — как «Отменить» (запись
 * в базе не тронута).
 */
const clearers = new Map<string, () => void>();

export function setClearer(form: string, clear: (() => void) | null): void {
  if (clear) clearers.set(form, clear);
  else clearers.delete(form);
}

/** Очищает формы с несохранённым. */
export function clearDirtyForms(): void {
  for (const form of dirtyForms()) clearers.get(form)?.();
}
