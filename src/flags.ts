import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { report } from "./errors";

/**
 * Выключатель, который помнится в приходе (не в общих настройках окна):
 * ключа нет в `COMMON_SETTINGS` (parish.rs), значит `get_setting` и
 * `set_setting` читают и пишут таблицу настроек открытого прихода.
 *
 * Изначально включён: выключенным считается только явное «0». Так просил
 * Роман 07.10.2026 про подстановку звания — «учитывая разную специфику
 * приходов, эту функцию необходимо сделать настраиваемой»: у него звание
 * восприемникам мешает, у второго тестировщика — «работает идеально».
 */
export function useParishFlag(key: string, title: string): [boolean, (on: boolean) => void] {
  const [on, setOn] = useState(true);
  useEffect(() => {
    invoke<string | null>("get_setting", { key })
      .then((v) => setOn(v !== "0"))
      .catch((e) => report(`Не удалось прочитать настройку «${title}»`, e));
  }, [key]);
  function change(next: boolean) {
    setOn(next);
    invoke("set_setting", { key, value: next ? "1" : "0" })
      .catch((e) => report(`Настройка «${title}» изменена, но не сохранена`, e));
  }
  return [on, change];
}

/** Ключи выключателей подстановки звания. */
export const AUTO_RANK_GODPARENT = "auto_rank_godparent";
export const AUTO_RANK_WITNESS = "auto_rank_witness";
