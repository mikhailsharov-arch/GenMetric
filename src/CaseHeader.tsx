import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Suggest from "./Suggest";
import { focusNextField } from "./focus";
import NumberField from "./NumberField";
import { report } from "./errors";

/**
 * Шапка дела: архив, фонд, опись, дело, приход, год, кто индексирует.
 *
 * Требование 3.4: вводится один раз и держится неизменной, пока человек сам
 * её не поменяет. В Excel это была та же идея, и Роман назвал её среди того,
 * что нужно обязательно сохранить.
 *
 * Церковь, село, уезд и губерния вместе образуют ключ прихода. По нему
 * переносится накопленная статистика подсказок между годами: индексируют
 * приходами, год за годом, поэтому на второй год подсказки почти всегда
 * попадают с первой буквы.
 */

export type Case = {
  id: number;
  archive: string | null;
  fond: string | null;
  opis: string | null;
  delo: string | null;
  church: string | null;
  village: string | null;
  uyezd: string | null;
  guberniya: string | null;
  year: number | null;
  indexer: string | null;
};

const EMPTY: Case = {
  id: 0, archive: null, fond: null, opis: null, delo: null, church: null,
  village: null, uyezd: null, guberniya: null, year: null, indexer: null,
};

export default function CaseHeader({ onSaved }: { onSaved: (c: Case) => void }) {
  const [c, setC] = useState<Case>(EMPTY);
  const [busy, setBusy] = useState(false);
  const [ok, setOk] = useState(false);

  useEffect(() => {
    invoke<Case | null>("case_load")
      .then((loaded) => {
        if (loaded) {
          setC(loaded);
          onSaved(loaded);
        }
      })
      .catch((e) => report("Не удалось прочитать дело", e));
  }, []);

  const set = (k: keyof Case) => (v: string) =>
    setC((prev) => ({ ...prev, [k]: v === "" ? null : v }));

  /** Enter и стрелки ведут по форме, как в остальных полях программы.
   *  Шапка заполняется редко, но спотыкаться на ней всё равно незачем. */
  function plainKeys(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter" || e.key === "ArrowDown") {
      e.preventDefault();
      focusNextField(e.currentTarget);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      focusNextField(e.currentTarget, -1);
    }
  }

  // Год — число, а не строка. Текстовое поле отдавало сюда строку, и
  // сохранение дела падало на разборе: программа ждёт число. Ошибка нашлась
  // на скриншоте до того, как её увидел тестировщик.
  const text = (label: string, k: "archive" | "fond" | "opis" | "delo" | "church"
                | "village" | "uyezd" | "guberniya" | "indexer", hint?: string) => (
    <div className="field">
      <label>{label}</label>
      <input
        data-field
        value={(c[k] as string | null) ?? ""}
        onChange={(e) => set(k)(e.target.value)}
        onKeyDown={plainKeys}
        autoComplete="off"
        spellCheck={false}
      />
      {hint && <div className="fieldhint">{hint}</div>}
    </div>
  );

  async function save() {
    setBusy(true);
    setOk(false);
    try {
      const id = await invoke<number>("case_save", { case: { ...c, id: c.id || 1 } });
      const next = { ...c, id };
      setC(next);
      onSaved(next);
      setOk(true);
    } catch (e) {
      report("Не удалось сохранить дело", e);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section>
      <h2>Дело</h2>
      <p className="hint">
        Заполняется один раз и держится, пока вы сами не измените. Церковь, село,
        уезд и губерния вместе задают приход — по нему подсказки переносятся
        между годами.
      </p>

      <Suggest label="Архив" kind="archive" value={c.archive ?? ""} onChange={set("archive")} />
      <div className="row">
        {text("Фонд", "fond")}
        {text("Опись", "opis")}
        {text("Дело", "delo")}
      </div>
      <Suggest label="Церковь" kind="church" value={c.church ?? ""} onChange={set("church")} />
      <Suggest label="Село" kind="place" value={c.village ?? ""} onChange={set("village")} />
      <Suggest label="Уезд" kind="uyezd" value={c.uyezd ?? ""} onChange={set("uyezd")} />
      <Suggest label="Губерния" kind="guberniya" value={c.guberniya ?? ""} onChange={set("guberniya")} />
      <div className="row">
        <NumberField
          label="Год"
          value={c.year}
          onChange={(v) => setC((prev) => ({ ...prev, year: v }))}
          min={1700}
          max={1930}
        />
        {text("Кто индексирует", "indexer")}
      </div>

      <button className="primary" onClick={save} disabled={busy}>
        {busy ? "Сохраняю…" : "Сохранить дело"}
      </button>
      {ok && <p className="hint">Сохранено. Можно переходить к записям.</p>}
    </section>
  );
}
