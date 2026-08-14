import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { report } from "./errors";

/**
 * Размер шрифта.
 *
 * Требование У-3: человек должен уметь менять размер сам. Пока такой
 * возможности не было, приходилось угадывать — и мы не угадали: Роман сказал,
 * что шрифт и межстрочный интервал крупноваты.
 *
 * Меняется одна переменная --ui-scale, от неё пересчитывается весь интерфейс:
 * все размеры в стилях заданы в rem и em, абсолютных пикселей не осталось
 * нигде, кроме толщины рамок.
 *
 * Значение хранится в базе (настройка ui_font_scale) и переживает перезапуск.
 */

const STEPS = [0.85, 0.925, 1, 1.1, 1.2, 1.35];
const DEFAULT = 1;
const KEY = "ui_font_scale";

function apply(scale: number): void {
  document.documentElement.style.setProperty("--ui-scale", String(scale));
}

function nearestStep(value: number): number {
  return STEPS.reduce((best, s) =>
    Math.abs(s - value) < Math.abs(best - value) ? s : best, STEPS[0]);
}

export default function FontScale() {
  const [index, setIndex] = useState(STEPS.indexOf(DEFAULT));
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    invoke<string | null>("get_setting", { key: KEY })
      .then((value) => {
        const parsed = value === null ? DEFAULT : Number(value);
        const scale = Number.isFinite(parsed) && parsed > 0 ? nearestStep(parsed) : DEFAULT;
        setIndex(STEPS.indexOf(scale));
        apply(scale);
        setLoaded(true);
      })
      .catch((e) => {
        // Не смогли прочитать — работаем со значением по умолчанию, но молчать
        // об этом нельзя: молчание уже стоило нам цикла тестирования.
        apply(DEFAULT);
        setLoaded(true);
        report("Не удалось прочитать сохранённый размер шрифта", e);
      });
  }, []);

  function change(step: -1 | 1) {
    const next = Math.min(STEPS.length - 1, Math.max(0, index + step));
    if (next === index) return;
    setIndex(next);
    apply(STEPS[next]);
    invoke("set_setting", { key: KEY, value: String(STEPS[next]) }).catch((e) =>
      report("Размер шрифта изменён, но не сохранён", e),
    );
  }

  return (
    <div className="scale" title="Размер шрифта">
      <button onClick={() => change(-1)} disabled={!loaded || index === 0} aria-label="Мельче">
        А−
      </button>
      <span className="scale-value">{Math.round(STEPS[index] * 100)}%</span>
      <button
        onClick={() => change(1)}
        disabled={!loaded || index === STEPS.length - 1}
        aria-label="Крупнее"
      >
        А+
      </button>
    </div>
  );
}
