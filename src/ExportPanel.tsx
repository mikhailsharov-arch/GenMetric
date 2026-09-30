import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Modal from "./Modal";
import { focusNextField } from "./focus";
import { report, warn } from "./errors";
import type { Case } from "./CaseHeader";

/**
 * Выгрузка (сборка #39, Роман 30.09.2026).
 *
 * В FAMILIO — в образец Familio (db/export/familio_template.xlsx): листы
 * about, location, РОЖДЕНИЕ, БРАК, СМЕРТЬ. Перед выгрузкой окно: «выгрузить
 * весь приход целиком или выбрать конкретные годы» и карточка «Для Familio»,
 * которая «автоматически переносится на вкладку about». Карточка
 * запоминается — в следующий раз заполнять заново не надо.
 *
 * В EXCEL — для себя, «всегда для прихода целиком (без диалоговых окон)»:
 * четыре листа МК, Рождения, Браки, Смерти, колонки как в индексаторе.
 *
 * Файл кладётся в папку «Документы/GenMetric» — без окна выбора места:
 * так выгрузку проверяет и сквозная проверка на Windows. Рядом с путём —
 * «Показать в папке».
 */

/** year = null — записи без года (год стал обязательным 22.09). */
type YearCount = { year: number | null; births: number; marriages: number; deaths: number };
type Exported = { path: string; births: number; marriages: number; deaths: number; places: number; persons: number };

/** Карточка «Для Familio» — строки листа about образца. */
export type About = {
  title: string;        // B4 Название справочника
  description: string;  // B7 Описание под названием на сайте
  author: string;       // B9 Кем проведена индексация
  profile: string;      // B10 Ссылка на профиль автора на Familio
  telegram: string;     // B11 Никнейм в Telegram
  isNew: boolean;       // B19 новый справочник / обновление начатого ранее
  previous: string;     // B20 Ссылка на загруженный ранее справочник
};

const ABOUT_KEY = "familio_about";

function yearsText(years: number[]): string {
  if (!years.length) return "";
  const lo = Math.min(...years), hi = Math.max(...years);
  return lo === hi ? String(lo) : `${lo}-${hi}`;
}

/** Название по умолчанию — как у Романа в «Для familio»: «Метрическая книга
 *  Борисоглебское за 1898 год». */
function defaultTitle(village: string, years: number[]): string {
  const y = yearsText(years);
  const where = village ? ` ${village}` : "";
  if (!y) return `Метрические книги${where}`;
  return years.length > 1 && y.includes("-")
    ? `Метрические книги${where} за ${y} годы`
    : `Метрическая книга${where} за ${y} год`;
}

export default function ExportPanel({ mkCase }: { mkCase: Case }) {
  const [years, setYears] = useState<YearCount[]>([]);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<"familio" | "excel" | null>(null);
  const [done, setDone] = useState<(Exported & { kind: string }) | null>(null);

  async function loadYears(): Promise<YearCount[]> {
    const ys = await invoke<YearCount[]>("export_years");
    setYears(ys);
    return ys;
  }

  async function openFamilio() {
    try {
      const ys = await loadYears();
      if (!ys.some((y) => y.births + y.marriages + y.deaths > 0)) {
        warn("Выгружать нечего", "в базе ещё нет ни одной записи");
        return;
      }
      setOpen(true);
    } catch (e) {
      report("Не удалось узнать, какие годы набраны", e);
    }
  }

  async function exportExcel() {
    setBusy("excel");
    setDone(null);
    try {
      const r = await invoke<Exported>("export_excel");
      setDone({ ...r, kind: "Excel" });
    } catch (e) {
      report("Выгрузка в Excel не удалась", e);
    } finally {
      setBusy(null);
    }
  }

  async function exportFamilio(chosen: number[], about: About) {
    setBusy("familio");
    setDone(null);
    try {
      await invoke("set_setting", { key: ABOUT_KEY, value: JSON.stringify(about) })
        .catch((e) => report("Не удалось запомнить карточку «Для Familio»", e));
      // Все годы — «[]», весь приход: так попадают и записи без года
      // (проверяющий #39: по списку годов они пропадали молча).
      const allYears = years.filter((y) => y.year !== null).length === chosen.length;
      const r = await invoke<Exported>("export_familio", {
        years: allYears ? [] : chosen,
        about: {
          title: about.title, years: yearsText(chosen), min_year: chosen.length ? String(Math.min(...chosen)) : "",
          description: about.description, author: about.author, profile: about.profile,
          telegram: about.telegram, is_new: about.isNew, previous: about.previous,
        },
      });
      setOpen(false);
      setDone({ ...r, kind: "Familio" });
    } catch (e) {
      report("Выгрузка в Familio не удалась", e);
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="exportpanel">
      <h2 className="sub-h2">Выгрузка</h2>
      <p className="hint">
        Файл сохраняется в папку «Документы/GenMetric». В Familio — по их образцу,
        весь приход или выбранные годы; в Excel — весь приход, как листы индексатора.
      </p>
      <div className="exportbuttons">
        <button type="button" className="toggle" disabled={busy !== null} onClick={() => void openFamilio()}>
          {busy === "familio" ? "Выгружаю…" : "Выгрузить в Familio…"}
        </button>
        <button type="button" className="toggle" disabled={busy !== null} onClick={() => void exportExcel()}>
          {busy === "excel" ? "Выгружаю…" : "Выгрузить в Excel"}
        </button>
      </div>
      {done && (
        <p className="hint exportdone" data-export-path={done.path}>
          Выгружено в {done.kind}: рождений {done.births}, браков {done.marriages}, смертей {done.deaths}
          {done.kind === "Familio" ? `, населённых пунктов ${done.places}` : `, персон ${done.persons}`}.
          {" "}Файл: <span className="mono">{done.path}</span>{" "}
          <button type="button" className="linkish"
                  onClick={() => invoke("reveal_path", { path: done.path })
                    .catch((e) => report("Не удалось открыть папку", e))}>
            Показать в папке
          </button>
        </p>
      )}
      {open && (
        <FamilioDialog mkCase={mkCase} years={years} busy={busy === "familio"}
                       onClose={() => setOpen(false)} onExport={(ys, a) => void exportFamilio(ys, a)} />
      )}
    </div>
  );
}

function FamilioDialog({ mkCase, years, busy, onClose, onExport }: {
  mkCase: Case; years: YearCount[]; busy: boolean;
  onClose: () => void; onExport: (years: number[], about: About) => void;
}) {
  const dated = years.filter((y): y is YearCount & { year: number } => y.year !== null);
  const undated = years.find((y) => y.year === null);
  const all = dated.map((y) => y.year);
  const [chosen, setChosen] = useState<number[]>(all);
  const village = mkCase.village ?? "";
  const [about, setAbout] = useState<About>({
    title: defaultTitle(village, all), description: "", author: mkCase.indexer ?? "",
    profile: "", telegram: "", isNew: true, previous: "",
  });
  // Название по годам — пока его не правили руками.
  const [titleTouched, setTitleTouched] = useState(false);

  useEffect(() => {
    invoke<string | null>("get_setting", { key: ABOUT_KEY })
      .then((v) => {
        if (!v) return;
        const saved = JSON.parse(v) as Partial<About>;
        // Название и описание зависят от годов — из памяти не берутся;
        // автор и ссылки — берутся.
        setAbout((a) => ({
          ...a,
          author: saved.author ?? a.author, profile: saved.profile ?? "",
          telegram: saved.telegram ?? "", isNew: saved.isNew ?? true, previous: saved.previous ?? "",
        }));
      })
      .catch((e) => report("Не удалось прочитать карточку «Для Familio»", e));
  }, []);

  useEffect(() => {
    if (!titleTouched) setAbout((a) => ({ ...a, title: defaultTitle(village, chosen) }));
  }, [chosen.join(","), titleTouched]);

  const toggle = (y: number) =>
    setChosen((c) => (c.includes(y) ? c.filter((v) => v !== y) : [...c, y].sort((a, b) => a - b)));
  const whole = chosen.length === all.length;
  const counts = years.filter((y) => (y.year === null ? whole : chosen.includes(y.year)))
    .reduce((s, y) => ({ b: s.b + y.births, m: s.m + y.marriages, d: s.d + y.deaths }), { b: 0, m: 0, d: 0 });

  function keys(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter" || e.key === "ArrowDown") {
      e.preventDefault();
      focusNextField(e.currentTarget, e.shiftKey && e.key === "Enter" ? -1 : 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      focusNextField(e.currentTarget, -1);
    }
  }

  const field = (label: string, k: "title" | "description" | "author" | "profile" | "telegram" | "previous",
                 hint?: string, touch?: () => void) => (
    <div className="field">
      <label>{label}</label>
      <div className="fieldbody">
        <input data-field value={about[k]} autoComplete="off" spellCheck={false} onKeyDown={keys}
               onChange={(e) => { touch?.(); setAbout((a) => ({ ...a, [k]: e.target.value })); }} />
        {hint && <div className="fieldhint">{hint}</div>}
      </div>
    </div>
  );

  function submit() {
    if (!chosen.length && all.length) {
      warn("Не выбран ни один год", "отметьте годы или нажмите «Весь приход»");
      return;
    }
    if (!about.title.trim()) {
      warn("Нет названия справочника", "оно пойдёт на лист about — без него Familio файл не примет");
      return;
    }
    onExport(chosen, { ...about, description: about.description.trim() || about.title });
  }

  return (
    <Modal title="Выгрузка в Familio" kind="familio" onClose={onClose}>
      <div className="exportyears">
        <span className="exportlabel">Годы:</span>
        {dated.map((y) => (
          <label key={y.year} className="yearbox" title={`рождений ${y.births}, браков ${y.marriages}, смертей ${y.deaths}`}>
            <input type="checkbox" checked={chosen.includes(y.year)} onChange={() => toggle(y.year)} />
            {y.year}
          </label>
        ))}
        {chosen.length !== all.length && (
          <button type="button" className="linkish" onClick={() => setChosen(all)}>Весь приход</button>
        )}
      </div>
      {undated && (
        <p className="hint">
          Записей без года: {undated.births + undated.marriages + undated.deaths} — выгружаются
          только со всем приходом{whole ? "" : ", сейчас не попадут"}.
        </p>
      )}
      <p className="hint">
        Будет выгружено: рождений {counts.b}, браков {counts.m}, смертей {counts.d}.
      </p>
      <h3>Для Familio</h3>
      {field("Название", "title", "кратко, но информативно", () => setTitleTouched(true))}
      {field("Описание", "description", "под названием на сайте; пусто — как название")}
      {field("Автор", "author", "кем проведена индексация")}
      {field("Профиль", "profile", "ссылка на вашу страницу на familio.org")}
      {field("Telegram", "telegram", "никнейм со знаком @")}
      <div className="field">
        <label>Справочник</label>
        <div className="fieldbody sexpick">
          <button type="button" className={about.isNew ? "on" : ""}
                  onClick={() => setAbout((a) => ({ ...a, isNew: true }))}>новый</button>
          <button type="button" className={!about.isNew ? "on" : ""}
                  onClick={() => setAbout((a) => ({ ...a, isNew: false }))}>обновление начатого ранее</button>
        </div>
      </div>
      {!about.isNew && field("Прежний справочник", "previous", "ссылка на уже загруженный справочник на familio.org")}
      <p className="hint">Отчества — в современном написании; как в книге — в авторском комментарии.</p>
      <div className="modalbar">
        <button type="button" className="primary" disabled={busy} onClick={submit}>
          {busy ? "Выгружаю…" : "Выгрузить"}
        </button>
        <button type="button" className="toggle" onClick={onClose}>Отмена</button>
      </div>
    </Modal>
  );
}
