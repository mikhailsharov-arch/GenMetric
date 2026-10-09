import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { report } from "./errors";

/**
 * Выключатель, который помнится в приходе (не в общих настройках окна):
 * ключа нет в `COMMON_SETTINGS` (parish.rs), значит `get_setting` и
 * `set_setting` читают и пишут таблицу настроек открытого прихода.
 *
 * Изначально включён (если не сказано иначе — `initial`): выключенным
 * считается только явное «0». Так просил
 * Роман 07.10.2026 про подстановку звания — «учитывая разную специфику
 * приходов, эту функцию необходимо сделать настраиваемой»: у него звание
 * восприемникам мешает, у второго тестировщика — «работает идеально».
 */
export function useParishFlag(key: string, title: string, initial = true): [boolean, (on: boolean) => void] {
  const [on, setOn] = useState(initial);
  useEffect(() => {
    invoke<string | null>("get_setting", { key })
      .then((v) => setOn(v === null || v === "" ? initial : v !== "0"))
      .catch((e) => report(`Не удалось прочитать настройку «${title}»`, e));
    // Один выключатель стоит в нескольких местах сразу (замок причта — в трёх
    // формах, замок вероисповедания — у каждой персоны): все слышат друг друга.
    const heard = (e: Event) => {
      const d = (e as CustomEvent<{ key: string; on: boolean }>).detail;
      if (d.key === key) setOn(d.on);
    };
    window.addEventListener(FLAG_EVENT, heard);
    return () => window.removeEventListener(FLAG_EVENT, heard);
  }, [key]);
  function change(next: boolean) {
    setOn(next);
    window.dispatchEvent(new CustomEvent(FLAG_EVENT, { detail: { key, on: next } }));
    invoke("set_setting", { key, value: next ? "1" : "0" })
      .catch((e) => report(`Настройка «${title}» изменена, но не сохранена`, e));
  }
  return [on, change];
}
const FLAG_EVENT = "genmetric:parish-flag";

/** Ключи выключателей подстановки звания. */
export const AUTO_RANK_GODPARENT = "auto_rank_godparent";
export const AUTO_RANK_WITNESS = "auto_rank_witness";

/** Замок причта (Роман и второй тестировщик 09.10.2026): закреплённый причт
 *  клавиши обходят и стрелки не меняют. Изначально снят. */
export const CLERGY_LOCK = "clergy_lock";
/** Замок вероисповедания (Роман 03.10.2026): «только чтение и вне Tab,
 *  состояние помнится». Изначально открыт. */
export const CONFESSION_LOCK = "confession_lock";
/** Свёрнут ли блок формы (Роман 03.10.2026: «сворачиваемые блоки для всех
 *  групп полей с памятью»). Ключ настройки прихода — `fold_<имя блока>`. */
export const foldKey = (block: string) => `fold_${block}`;
