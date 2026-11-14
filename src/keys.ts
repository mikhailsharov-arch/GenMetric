/**
 * Клавиши с учётом Mac (Роман 12.11.2026, пп. 141, 142, функция 5).
 *
 * На Mac подсказки — маковскими значками: ⌘ вместо Ctrl, ⌥ вместо Alt.
 * Листать скан Alt+PageDown на Mac нечем (клавиши PageDown нет, Fn+⌥+↓
 * программа не получала) — там листают ⌘+] и ⌘+[ (по коду клавиши: в
 * русской раскладке это «ъ» и «х»).
 */

export const MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

/** Сохранить запись. */
export const SAVE_KEY = MAC ? "⌘+Enter" : "Ctrl+Enter";
/** Свой поиск персоны. */
export const FIND_KEY = MAC ? "⌘+F" : "Ctrl+F";
/** Следующий и предыдущий разворот скана. */
export const NEXT_SHEET_KEY = MAC ? "⌘+]" : "Alt+PageDown";
export const PREV_SHEET_KEY = MAC ? "⌘+[" : "Alt+PageUp";
/** Двигать лист скана. */
export const PAN_KEY = MAC ? "⌥+стрелки" : "Alt+стрелки";

/** Листание скана с клавиатуры: 1 — дальше, −1 — назад, 0 — не листание.
 *  Alt+PageDown/PageUp — везде; ⌘+] и ⌘+[ — на Mac (и Ctrl+] / Ctrl+[ —
 *  на случай Mac с клавиатурой Windows). */
export function sheetStep(e: KeyboardEvent): -1 | 0 | 1 {
  if (e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey) {
    if (e.key === "PageDown") return 1;
    if (e.key === "PageUp") return -1;
  }
  if (MAC && (e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey) {
    if (e.code === "BracketRight") return 1;
    if (e.code === "BracketLeft") return -1;
  }
  return 0;
}
