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
