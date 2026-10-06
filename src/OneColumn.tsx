import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { report } from "./errors";

/**
 * «Всегда в столбик» (Роман 30.09.2026): при включённом строки формы не
 * перестраиваются в ряд, как бы широко ни было окно.
 *
 * Плотная раскладка (27.09.2026) ставит персон в две колонки, а «НП | Звание»
 * — одной строкой, когда хватает ширины. Кому-то привычнее одна колонка
 * сверху вниз, как в узком окне: глаз идёт только по вертикали.
 *
 * Класс `onecol` на корне документа отключает контейнеры, от которых считается
 * плотная раскладка (styles.css). Настройка `ui_one_column` — общая для
 * приходов и переживает перезапуск.
 *
 * Переключатель живёт на экране «О программе», а не в верхней строке: в ней
 * при ширине окна по умолчанию места ровно на вкладки, «ⓘ» и размер шрифта —
 * ещё одна кнопка переносила бы строку и отнимала высоту у формы (стенд
 * 06.10.2026). Настройку ставят один раз.
 */
const KEY = "ui_one_column";

function apply(on: boolean): void {
  document.documentElement.classList.toggle("onecol", on);
}

/** Состояние переключателя: читается при запуске, применяется сразу. */
export function useOneColumn(): [boolean, () => void] {
  const [on, setOn] = useState(false);

  useEffect(() => {
    invoke<string | null>("get_setting", { key: KEY })
      .then((value) => {
        setOn(value === "1");
        apply(value === "1");
      })
      .catch((e) => report("Не удалось прочитать настройку «всегда в столбик»", e));
  }, []);

  function toggle() {
    const next = !on;
    setOn(next);
    apply(next);
    invoke("set_setting", { key: KEY, value: next ? "1" : "0" }).catch((e) =>
      report("Раскладка изменена, но не сохранена", e),
    );
  }

  return [on, toggle];
}

export default function OneColumn({ on, onToggle }: { on: boolean; onToggle: () => void }) {
  return (
    <>
      <h2>Вид формы</h2>
      <div className="parishrow">
        <span>Всегда в столбик: <b>{on ? "включено" : "выключено"}</b></span>
        <button type="button" className="toggle small" data-onecol aria-pressed={on} onClick={onToggle}>
          {on ? "Выключить" : "Включить"}
        </button>
      </div>
      <p className="hint">
        Включено — поля формы идут друг под другом, как бы широко ни было окно. Выключено —
        в широком окне персоны встают в две колонки, а «НП» и «Звание» — в одну строку.
      </p>
    </>
  );
}
