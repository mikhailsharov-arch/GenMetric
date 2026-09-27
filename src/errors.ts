import { useEffect, useState } from "react";

/**
 * Сбор ошибок интерфейса.
 *
 * 13.08.2026 половина сборки молча не работала: в коде стоял пустой перехват
 * вида .catch(() => setItems([])), и поломка выглядела как «просто ничего
 * не происходит». Программа знала точную причину и проглотила её. Это стоило
 * тестировщику целого цикла проверки.
 *
 * Поэтому: перехватывать ошибку без сообщения человеку нельзя. Любая неудача
 * проходит через report() и показывается полосой вверху окна.
 */

export type AppError = {
  what: string; // что делали, по-русски
  detail: string; // техническое сообщение, его можно скопировать и прислать
  at: number;
  /** Предупреждение: человек что-то не заполнил. Это не поломка — присылать
   *  нечего, и полоса не говорит «это не ваша ошибка, пришлите текст»
   *  (техдолг В7, 27.09.2026). */
  warn?: boolean;
};

let current: AppError | null = null;
const listeners = new Set<(e: AppError | null) => void>();

function emit() {
  for (const listener of listeners) listener(current);
}

export function report(what: string, error: unknown): void {
  current = { what, detail: String(error), at: Date.now() };
  emit();
}

/** Подсказка человеку: чего не хватает, чтобы сохранить. Не поломка. */
export function warn(what: string, hint: string): void {
  current = { what, detail: hint, at: Date.now(), warn: true };
  emit();
}

/** Снять предупреждение (не ошибку) — после удачного сохранения оно устарело
 *  (проверяющий 27.09.2026: «Не указан год» висело после сохранения). */
export function dismissWarn(): void {
  if (current?.warn) dismiss();
}

export function dismiss(): void {
  current = null;
  emit();
}

export function useAppError(): AppError | null {
  const [value, setValue] = useState<AppError | null>(current);
  useEffect(() => {
    listeners.add(setValue);
    return () => {
      listeners.delete(setValue);
    };
  }, []);
  return value;
}
