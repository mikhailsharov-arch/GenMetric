import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { report } from "./errors";

/**
 * Полное ИОФ ребёнка для строки разбора под полем «Ребёнок» и «Умерший»: имя +
 * отчество по имени отца + фамилия отца (спека 2026-10-15-semya-celikom, п.
 * 1.5). Роман 16.10.2026, ответ 2а: только показывать — в базе и выгрузках у
 * ребёнка имя, как в книге. С 19.10.2026 — не отдельной строкой, а вместо
 * современного написания в строке разбора («Федор Андреевич Голяков М»):
 * вторая строка «перегружает интерфейс».
 *
 * Считает программа (`child_full_iof`) по тому же правилу, что связи в
 * поиске; нечего дописать или пол не известен — null.
 */
export function useFullIof(child: string, gender: string | null, father: string, fatherNameSurname?: boolean): string | null {
  const [full, setFull] = useState<string | null>(null);
  // Об ошибке — один раз, а не на каждую паузу набора (ревьюер 17.10.2026).
  const told = useRef(false);
  useEffect(() => {
    const kid = child.trim(), dad = father.trim();
    // Пол не известен — не показываем: отчество и фамилия зависят от пола.
    if (!kid || !dad || /\s/.test(kid) || !gender) {
      setFull(null);
      return;
    }
    let live = true;
    // Пауза — чтобы не спрашивать на каждую букву.
    const timer = window.setTimeout(() => {
      invoke<string | null>("child_full_iof", { child: kid, gender, father: dad, fatherNameSurname: fatherNameSurname ?? false })
        .then((v) => { if (live) setFull(v); })
        .catch((e) => {
          if (!live || told.current) return;
          told.current = true;
          report("Не удалось собрать полное ИОФ ребёнка", e);
        });
    }, 250);
    return () => { live = false; window.clearTimeout(timer); };
  }, [child, gender, father, fatherNameSurname]);
  return full;
}
