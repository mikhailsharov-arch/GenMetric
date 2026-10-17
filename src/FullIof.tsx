import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { report } from "./errors";

/**
 * Полное ИОФ ребёнка под полем «Ребёнок» и «Умерший»: имя + отчество по
 * имени отца + фамилия отца (спека 2026-10-15-semya-celikom, п. 1.5).
 *
 * Роман 16.10.2026, ответ 2а: только показывать — в базе и выгрузках у
 * ребёнка имя, как в книге. Считает программа (`child_full_iof`) по тому же
 * правилу, что связи в поиске; нечего дописать — строки нет.
 */
export default function FullIof({ child, gender, father, fatherNameSurname }: {
  child: string;
  gender: string | null;
  father: string;
  fatherNameSurname?: boolean;
}) {
  const [full, setFull] = useState<string | null>(null);
  // Об ошибке — один раз, а не на каждую паузу набора (ревьюер 17.10.2026).
  const told = useRef(false);
  useEffect(() => {
    const kid = child.trim(), dad = father.trim();
    // Пол не известен (имени нет в словаре, «мальчик/девочка» не нажато) —
    // не показываем: отчество и фамилия зависят от пола (проверяющий 17.10.2026).
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
  if (!full) return null;
  return (
    <p className="hint fulliof" data-full-iof>
      Полное ИОФ: <b>{full}</b> <span className="sub">— только для показа, в запись идёт «{child.trim()}»</span>
    </p>
  );
}
