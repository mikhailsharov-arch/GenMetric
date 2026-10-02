import { createContext, useContext, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { EMPTY_PERSON, type Person } from "./PersonBlock";
import { report } from "./errors";

/**
 * Причт — один на все разделы (27.09.2026).
 *
 * Роман: «Церковнослужители должны быть одни и те же и для рождений, и для
 * браков, и для смертей». Раньше у каждой формы был свой причт, и сменив
 * священника в рождениях, в браках его приходилось выбирать заново.
 *
 * Теперь причт живёт над формами. Правка сохранённой записи — исключение:
 * у открытой записи свой причт (может быть другим), он держится в форме,
 * а общий не трогается (см. useFormClergy).
 */

export type Trio = [Person, Person, Person];
type Upd = Person | ((p: Person) => Person);

type Shared = {
  people: Trio;
  setAt: (i: number, p: Upd) => void;
  /** Растёт после каждого сохранения любой записи: список причта
   *  перечитывается, заполненный причт сворачивается. */
  savedTimes: number;
  bump: () => void;
};

const EMPTY_TRIO: Trio = [EMPTY_PERSON, EMPTY_PERSON, EMPTY_PERSON];
const Ctx = createContext<Shared | null>(null);

type LastClergy = { role_code: string; iof: string; rank: string | null; note: string | null };

export function ClergyProvider({ children }: { children: React.ReactNode }) {
  const [people, setPeople] = useState<Trio>(EMPTY_TRIO);
  const [savedTimes, setSavedTimes] = useState(0);

  function setAt(i: number, p: Upd) {
    setPeople((all) => {
      const next = [...all] as Trio;
      next[i] = typeof p === "function" ? p(all[i]) : p;
      return next;
    });
  }

  // Продолжить с места: причт последней записи прихода в любом разделе
  // (заказчик 21.09.2026 — причт после перезапуска; 27.09 — общий). Один раз:
  // дело теперь на год книги, а причт от смены года не зависит (02.10.2026).
  useEffect(() => {
    invoke<LastClergy[]>("last_clergy", { section: 0 })
      .then((rows) => {
        const idx = { clergy1: 0, clergy2: 1, clergy3: 2 } as Record<string, number>;
        for (const r of rows) {
          const i = idx[r.role_code];
          if (i !== undefined && r.iof.trim())
            setAt(i, (p) => (p.iof.trim() ? p : { ...p, iof: r.iof, rank: r.rank ?? "", note: r.note ?? "" }));
        }
        if (rows.length > 0) setSavedTimes((n) => n + 1); // свернуть заполненный причт
      })
      .catch((e) => report("Не удалось восстановить причт последней записи", e));
  }, []);

  return (
    <Ctx.Provider value={{ people, setAt, savedTimes, bump: () => setSavedTimes((n) => n + 1) }}>
      {children}
    </Ctx.Provider>
  );
}

/**
 * Причт для формы: общий, а на время правки сохранённой записи — её
 * собственный. `open(trio)` начинает правку, `close()` возвращает общий.
 */
export function useFormClergy() {
  const shared = useContext(Ctx);
  if (!shared) throw new Error("useFormClergy вне ClergyProvider");
  const [own, setOwn] = useState<Trio | null>(null);
  // Режим — из ссылки, а не из замыкания рендера: ответ разбора ИОФ может
  // прийти с обработчиком, снятым до open(), и ушёл бы в общий причт
  // (ревьюер 27.09.2026).
  const ownRef = useRef(false);
  const people = own ?? shared.people;
  function setAt(i: number, p: Upd) {
    if (ownRef.current) {
      setOwn((all) => {
        if (!all) return all;
        const next = [...all] as Trio;
        next[i] = typeof p === "function" ? p(all[i]) : p;
        return next;
      });
    } else shared!.setAt(i, p);
  }
  return {
    people,
    setAt,
    savedTimes: shared.savedTimes,
    bump: shared.bump,
    open: (trio: Trio) => { ownRef.current = true; setOwn(trio); },
    close: () => { ownRef.current = false; setOwn(null); },
  };
}
