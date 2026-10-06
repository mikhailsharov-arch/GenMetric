import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Modal from "./Modal";
import { dirtyForms } from "./dirty";
import { report } from "./errors";

/**
 * Окно «Приходы» (спека 2026-10-02, п. 1 и 4).
 *
 * Каждый приход — свой файл; открыт один. Роман 02.10.2026: файлы нужны,
 * чтобы «индексировать сразу несколько приходов одновременно, переключаясь
 * между ними … не смешивая данные в одну кучу». Здесь — список, «Открыть»,
 * «Новый приход…» и «Импортировать из Excel…» (старый индексатор становится
 * новым приходом; в открытый приход ничего не добавляется).
 *
 * После смены прихода окно программы перечитывается целиком: все формы,
 * списки и подсказки принадлежали прежнему приходу. Удаления прихода нет.
 */

type ParishRow = {
  id: number; name: string; file: string; opened_at: string | null;
  entries: number | null; missing: boolean; current: boolean; source_name: string | null;
};

type Seen = {
  village: string; church: string; indexer: string;
  births: number; marriages: number; deaths: number; years: number[];
  /** Годы, похожие на опечатку в колонке «Год». */
  odd_years: number[];
  size: number; already_id: number | null; already_name: string | null;
};

type Done = {
  births: number; marriages: number; deaths: number; persons: number; places: number;
  years: number[]; unknown_names: number; unknown_examples: string[];
  unknown_patronymics: number; unknown_patronymic_examples: string[];
  skipped: string[]; notes: string[]; parish_name: string;
};

/** Файл уходит в программу частями: байты — обычным аргументом (сырое тело
 *  на Windows не дошло, инцидент 13.09.2026), а 7 МБ одним массивом чисел —
 *  слишком тяжёлое сообщение. */
const CHUNK = 512 * 1024;

/** Размер части. Сквозная проверка на Windows ставит `window.__genmetricChunk`
 *  поменьше: файл её фикстуры — 6 КБ и одной частью склейку не проверяет. */
function chunkSize(): number {
  const test = Number((window as unknown as { __genmetricChunk?: number }).__genmetricChunk);
  return Number.isFinite(test) && test >= 1 ? Math.floor(test) : CHUNK;
}

function yearsText(years: number[]): string {
  if (!years.length) return "без года";
  const a = Math.min(...years), b = Math.max(...years);
  return a === b ? String(a) : `${a}–${b}`;
}

export default function ParishDialog({ onClose }: { onClose: () => void }) {
  const [rows, setRows] = useState<ParishRow[]>([]);
  const [mode, setMode] = useState<"list" | "new" | "import" | "done">("list");
  // Окно меняет вид (список → название → итог): фокус — в первое поле нового вида.
  useEffect(() => {
    // Поля нет (список, итог) — на само окно: иначе фокус остаётся на
    // исчезнувшем поле, и Esc до окна не доходит.
    const box = document.querySelector<HTMLElement>('.modal[data-modal="parish"]');
    (box?.querySelector<HTMLInputElement>("input[data-field]") ?? box)?.focus();
  }, [mode]);
  const [busy, setBusy] = useState<string | null>(null);
  /** Сколькими частями ушёл выбранный файл — для сквозной проверки. */
  const [parts, setParts] = useState(0);
  const [name, setName] = useState("");
  const [fileName, setFileName] = useState("");
  const [seen, setSeen] = useState<Seen | null>(null);
  const [replace, setReplace] = useState(false);
  const [done, setDone] = useState<Done | null>(null);
  const [refused, setRefused] = useState<string | null>(null);
  const filePick = useRef<HTMLInputElement>(null);

  useEffect(() => {
    invoke<ParishRow[]>("parish_list")
      .then(setRows)
      .catch((e) => report("Не удалось прочитать перечень приходов", e));
  }, []);

  /** Несохранённая запись при смене прихода не теряется молча. */
  function blocked(): boolean {
    const forms = dirtyForms();
    if (!forms.length) return false;
    setRefused(`Сначала сохраните или очистите набранное: ${forms.join(", ")}. `
      + "При смене прихода несохранённая запись пропала бы.");
    return true;
  }

  /** Приход сменён — окно перечитывает всё заново. */
  function reload() {
    window.location.reload();
  }

  async function open(id: number) {
    if (blocked()) return;
    setBusy("open");
    try {
      await invoke<string>("parish_open", { id });
      reload();
    } catch (e) {
      report("Приход не открылся", e);
      setBusy(null);
    }
  }

  async function create() {
    if (blocked()) return;
    setRefused(null);
    setBusy("create");
    try {
      await invoke<string>("parish_create", { name });
      reload();
    } catch (e) {
      // Чаще всего — занятое название: это не поломка, говорим в окне.
      setRefused(String(e));
      setBusy(null);
    }
  }

  async function pick(file: File) {
    if (blocked()) return;
    setRefused(null);
    setBusy("Читаю файл…");
    setSeen(null);
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const step = chunkSize();
      let sent = 0;
      let got = 0;
      for (let at = 0; at < bytes.length || at === 0; at += step) {
        got = await invoke<number>("import_chunk", { bytes: bytes.subarray(at, at + step), first: at === 0 });
        sent += 1;
        setBusy(`Читаю файл… ${Math.min(100, Math.round(((at + step) / Math.max(1, bytes.length)) * 100))}%`);
        if (bytes.length === 0) break;
      }
      // Программа отвечает, сколько байт у неё собралось: не столько, сколько
      // в файле, — часть потерялась по дороге, и разбирать такой файл нельзя.
      if (got !== bytes.length)
        throw new Error(`файл дошёл до программы не целиком: ${got} из ${bytes.length} байт — выберите его ещё раз`);
      setParts(sent);
      const s = await invoke<Seen>("import_inspect", { fileName: file.name });
      setSeen(s);
      setFileName(file.name);
      setReplace(false);
      setName(`${s.village || "Приход"} (из Excel)`);
      setMode("import");
    } catch (e) {
      // «Это не индексатор» — не поломка программы: говорим в окне.
      setRefused(String(e));
    } finally {
      setBusy(null);
      if (filePick.current) filePick.current.value = "";
    }
  }

  async function runImport() {
    if (!seen || blocked()) return;
    setBusy("import");
    try {
      const d = await invoke<Done>("import_run", {
        name: replace && seen.already_name ? seen.already_name : name,
        fileName, replace: replace ? seen.already_id : null,
      });
      setDone(d);
      setMode("done");
    } catch (e) {
      // Чаще всего — занятое название: файл программа помнит, поправьте и повторите.
      setRefused(`Импорт не прошёл, приход не создан. ${String(e)}`);
    } finally {
      setBusy(null);
    }
  }

  // Итог импорта: приход уже открыт программой, любое закрытие — перечитать окно.
  if (mode === "done" && done) {
    return (
      <Modal title="Импорт завершён" kind="parish" onClose={reload}>
        <p data-import-done>
          Приход «{done.parish_name}»: рождений {done.births}, браков {done.marriages}, смертей {done.deaths};
          персон {done.persons}; новых населённых пунктов {done.places}; годы {yearsText(done.years)}.
        </p>
        {done.unknown_names > 0 && (
          <p className="hint">
            Имён, которых нет в словаре: {done.unknown_names} — перенесены как в Excel
            {done.unknown_examples.length > 0 && <> ({done.unknown_examples.join(", ")})</>}.
            Сверить можно позже: откройте запись и выйдите из поля ИОФ.
          </p>
        )}
        {done.unknown_patronymics > 0 && (
          <p className="hint">
            Вторых слов, похожих на отчество, но не найденных в словаре: {done.unknown_patronymics} — перенесены
            как часть фамилии
            {done.unknown_patronymic_examples.length > 0 && <> ({done.unknown_patronymic_examples.join(", ")})</>}.
          </p>
        )}
        {done.skipped.length > 0 && (
          <>
            <p className="hint">Не перенесено строк: {done.skipped.length}.</p>
            <ul className="importnotes">{done.skipped.slice(0, 30).map((s) => <li key={s}>{s}</li>)}</ul>
          </>
        )}
        {done.notes.length > 0 && (
          <>
            <p className="hint">Перенесено с оговоркой: {done.notes.length}.</p>
            <ul className="importnotes">{done.notes.slice(0, 30).map((s) => <li key={s}>{s}</li>)}</ul>
          </>
        )}
        <p className="hint">
          Фонд, опись и дело каждого года — на экране «Дело» (список годов в заголовке).
          Формы продолжат с последней записи; выгрузки — весь приход.
        </p>
        <div className="modalbar">
          <button type="button" className="primary" onClick={reload}>Перейти в приход</button>
        </div>
      </Modal>
    );
  }

  return (
    <Modal title="Приходы" kind="parish"
           onClose={busy ? () => {} : mode === "list" ? onClose : () => { setRefused(null); setMode("list"); }}>
      {refused && <p className="refused">{refused}</p>}
      {mode === "list" && (
        <>
          <p className="hint">
            Каждый приход — отдельный файл со своими записями. Справочники населённых
            пунктов и званий — общие. Открыт один приход; при смене окно перечитается.
          </p>
          <table className="facts parishes">
            <tbody>
              {rows.map((r) => (
                <tr key={r.id} className={r.current ? "editing" : ""}>
                  <td>
                    <b>{r.name}</b>
                    <div className="fieldhint">
                      {r.missing ? "файл не найден" : `записей: ${r.entries ?? "?"}`}
                      {r.opened_at ? ` · открывали ${r.opened_at.slice(0, 16)}` : ""}
                      {r.source_name ? ` · из ${r.source_name}` : ""}
                    </div>
                  </td>
                  <td>
                    {r.current ? "открыт" : (
                      <button type="button" className="toggle small" disabled={r.missing || busy !== null}
                              onClick={() => void open(r.id)}>
                        Открыть
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <input ref={filePick} type="file" hidden data-import-file
                 accept=".xlsm,.xlsx,application/vnd.ms-excel.sheet.macroEnabled.12,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                 onChange={(e) => { const f = e.target.files?.[0]; if (f) void pick(f); }} />
          <div className="modalbar">
            <button type="button" className="toggle" disabled={busy !== null}
                    onClick={() => { setRefused(null); setName(""); setMode("new"); }}>
              Новый приход…
            </button>
            <button type="button" className="toggle" disabled={busy !== null}
                    onClick={() => { setRefused(null); filePick.current?.click(); }}>
              {busy && busy.startsWith("Читаю") ? busy : "Импортировать из Excel…"}
            </button>
            <button type="button" className="toggle" disabled={busy !== null} onClick={onClose}>Закрыть (Esc)</button>
          </div>
        </>
      )}
      {mode === "new" && (
        <>
          <p className="hint">
            Новый приход — пустой: без записей, персон и причта; справочники общие.
            Дело (архив, церковь, село) заполните в нём на экране «Дело».
          </p>
          <div className="field">
            <label>Название</label>
            <div className="fieldbody">
              <input data-field value={name} onChange={(e) => setName(e.target.value)}
                     placeholder="например, село прихода" autoComplete="off" spellCheck={false}
                     onKeyDown={(e) => { if (e.key === "Enter" && name.trim() && !busy) { e.preventDefault(); void create(); } }} />
            </div>
          </div>
          <div className="modalbar">
            <button type="button" className="primary" disabled={!name.trim() || busy !== null} onClick={() => void create()}>
              {busy === "create" ? "Создаю…" : "Создать и открыть"}
            </button>
            <button type="button" className="toggle" disabled={busy !== null} onClick={() => setMode("list")}>Назад</button>
          </div>
        </>
      )}
      {mode === "import" && seen && (
        <>
          <p data-import-seen data-parts={parts}>
            В файле «{fileName}»: рождений {seen.births}, браков {seen.marriages}, смертей {seen.deaths};
            годы {yearsText(seen.years)}{seen.village ? `; село ${seen.village}` : ""}
            {seen.indexer ? `; индексировал ${seen.indexer}` : ""}.
          </p>
          {seen.odd_years.length > 0 && (
            <p className="reviewbusy" data-import-oddyears>
              {seen.odd_years.length === 1 ? "Год" : "Годы"} {seen.odd_years.join(", ")} —
              {seen.odd_years.length === 1 ? " стоит" : " стоят"} далеко от остальных, и записей там не больше трёх.
              Если это опечатка в колонке «Год», проще поправить её в Excel и выбрать файл заново: иначе
              такому году заведётся отдельное дело. Импортировать можно и так — эти строки попадут в список на сверку.
            </p>
          )}
          <p className="hint">
            Записи лягут в новый приход — в открытый ничего не добавится. Каждая запись
            сохраняется так же, как набранная руками, поэтому подсказки сразу знают людей и места из файла.
          </p>
          {seen.already_id !== null && (
            <div className="importagain">
              <p>Этот файл уже импортирован — приход «{seen.already_name}». Что сделать?</p>
              <label>
                <input type="radio" name="again" checked={!replace} onChange={() => setReplace(false)} />
                {" "}создать ещё один приход
              </label>
              <label>
                <input type="radio" name="again" checked={replace} onChange={() => setReplace(true)} />
                {" "}заменить приход «{seen.already_name}» — набранное в нём после импорта в новый не попадёт
                (прежний файл останется в папке приходов с пометкой «заменён»)
              </label>
            </div>
          )}
          {!replace && (
            <div className="field">
              <label>Название</label>
              <div className="fieldbody">
                <input data-field value={name} onChange={(e) => setName(e.target.value)}
                       autoComplete="off" spellCheck={false}
                       onKeyDown={(e) => { if (e.key === "Enter" && name.trim() && !busy) { e.preventDefault(); void runImport(); } }} />
                <div className="fieldhint">как приход будет называться в перечне и в заголовке окна</div>
              </div>
            </div>
          )}
          <div className="modalbar">
            <button type="button" className="primary" disabled={busy !== null || (!replace && !name.trim())}
                    onClick={() => void runImport()}>
              {busy === "import" ? "Импортирую… это может занять минуту" : "Импортировать"}
            </button>
            <button type="button" className="toggle" disabled={busy !== null} onClick={() => setMode("list")}>Назад</button>
          </div>
        </>
      )}
    </Modal>
  );
}
