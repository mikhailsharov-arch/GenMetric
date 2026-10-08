import { invoke } from "@tauri-apps/api/core";
import { report } from "./errors";
import { NP_TYPES } from "./names";

/**
 * Типы населённых пунктов — для правила заглавной буквы в поле НП: название,
 * начинающееся с типа («д.Балахонка…», «починок Смыгарев»), остаётся как
 * набрано. Перечень `np_type` человек может пополнить, поэтому он читается из
 * базы — один раз на окно (смена прихода перечитывает окно целиком). Пока
 * ответа нет, действуют типы поставки.
 */
let types: string[] = NP_TYPES;
let asked = false;

export function placeTypes(): string[] {
  if (!asked) {
    asked = true;
    invoke<{ value: string }[]>("suggest", { kind: "np_type", prefix: "", limit: 200 })
      .then((rows) => {
        if (rows.length) types = Array.from(new Set([...NP_TYPES, ...rows.map((r) => r.value)]));
      })
      .catch((e) => {
        asked = false;
        report("Не удалось прочитать типы населённых пунктов", e);
      });
  }
  return types;
}
