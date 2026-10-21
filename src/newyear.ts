import { useEffect, useRef } from "react";

/**
 * Год на форме сменили руками — все формы встают на него (спека
 * 2026-10-19-oshibki-svyazej-i-dovodka, Ж2.7; 2026-10-20-dosye-i-dovodka, п. 2).
 *
 * До 20.10.2026 это срабатывало только при уходе из поля «Год». У Романа
 * год 1901 → 1902 на «Рождениях» сбросил счёт и месяцы (это делает сама
 * форма), а страница, скан и другие вкладки остались: кнопки «+/−», щелчок
 * и листание в скане из поля не уводят. Теперь срабатывает сама смена года
 * руками — через полсекунды после последней цифры (год набирают по цифре) и
 * сразу при уходе из поля. Год, поставленный программой (продолжить с места,
 * другая форма, экран «Дело»), сюда не идёт.
 */
export function useNewYear(year: number | null, editing: boolean,
                           onNewYear: ((year: number, from: number | null) => Promise<boolean>) | undefined,
                           askDir: (year: number) => void) {
  const pending = useRef(false);
  const done = useRef<number | null>(null);
  /** Год формы до смены руками: «следующий год» и возврат считаются от него. */
  const from = useRef<number | null>(null);
  function fire(y: number | null) {
    pending.current = false;
    if (editing || y === null || y < 1700 || !onNewYear || done.current === y) return;
    done.current = y;
    const was = from.current;
    from.current = null;
    void onNewYear(y, was).then((fresh) => { if (fresh) askDir(y); });
  }
  useEffect(() => {
    if (!pending.current) return;
    const t = window.setTimeout(() => fire(year), 500);
    return () => window.clearTimeout(t);
  }, [year]);
  return {
    /** Обработчик `onChange` поля «Год». */
    byHand(v: number | null, set: (v: number | null) => void) {
      // То же значение — не смена (ревьюер 20.10.2026: иначе следующая смена
      // года программой срабатывала как ручная).
      if (v === year) return;
      if (!pending.current) from.current = year;
      pending.current = true;
      if (v !== done.current) done.current = null;
      set(v);
    },
    /** Обработчик ухода из поля «Год»: не ждать полсекунды. */
    leave() {
      if (pending.current) fire(year);
    },
  };
}
