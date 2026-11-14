import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { report } from "./errors";
import { dirtyForms } from "./dirty";

/**
 * «Найти в записях» на «ⓘ О программе» (спека 2026-11-13-sprint-139-149,
 * новая функция 2). Роман 12.11.2026: опечатка «головая боль» стоит в
 * какой-то записи — «я физически не могу найти ту самую запись среди сотен
 * других», а удалить значение — потерять причину в той записи. Здесь: где
 * стоит значение, «Открыть» любую запись, «Заменить во всех на…». Замена
 * пересохраняет записи, как правка в форме (`replace.rs`); перед ней —
 * копия прихода. Закрывает и его задачу 11 (поиск фамилии «попов»).
 */

type Spelling = { value: string; entries: number };
type Hit = { entry_id: number; section: number; year: number | null; page: string | null; no: number | null; role_code: string; who: string };

const FIELDS: { code: string; title: string }[] = [
  { code: "death_cause", title: "причина смерти" },
  { code: "rank", title: "звание" },
  { code: "kinship", title: "родство" },
  { code: "surname", title: "фамилия" },
];
const SECTION = ["", "рождение", "брак", "смерть"];
/** Сколько записей показывать списком. */
const SHOWN = 200;

export default function FindReplaceBlock({ onOpenEntry }: { onOpenEntry: (section: number, id: number) => void }) {
  const [field, setField] = useState("death_cause");
  const [query, setQuery] = useState("");
  const [found, setFound] = useState<Spelling[] | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [hits, setHits] = useState<Hit[]>([]);
  const [to, setTo] = useState("");
  const [sure, setSure] = useState(false);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  async function look(q = query, f = field) {
    setPicked(null); setHits([]); setSure(false); setNote(null);
    if (!q.trim()) { setFound(null); return; }
    try {
      setFound(await invoke<Spelling[]>("replace_spellings", { field: f, query: q }));
    } catch (e) {
      report("Не удалось найти в записях", e);
    }
  }

  async function pick(value: string) {
    setPicked(value); setTo(""); setSure(false); setNote(null);
    try {
      setHits(await invoke<Hit[]>("replace_hits", { field, value }));
    } catch (e) {
      report("Не удалось прочитать записи", e);
    }
  }

  async function replace() {
    if (!picked || !to.trim() || busy) return;
    // Набранная форма — сразу, а не после «Да, заменить…» (проверяющий 13.11.2026).
    const forms = dirtyForms();
    if (forms.length) {
      setNote(`Сначала сохраните или очистите набранное: ${forms.join(", ")} — иначе форма вернёт прежнее значение.`);
      setSure(false);
      return;
    }
    if (!sure) { setSure(true); return; }
    setBusy(true);
    try {
      const r = await invoke<{ entries: number; copy: string }>("replace_apply", { field, old: picked, new: to.trim() });
      setNote(`Заменено в записях: ${r.entries}. Копия прихода до замены: ${r.copy}. Если «${picked}» осталось в подсказке с пометкой
        «словарь» — это значение поставки: уберите его кнопкой «Найти неиспользуемые…» выше. Память из архива
        Excel остаётся в подсказке и так.`);
      setSure(false);
      await look(to.trim());
    } catch (e) {
      report("Не удалось заменить в записях", e);
    } finally {
      setBusy(false);
    }
  }

  const title = FIELDS.find((f) => f.code === field)?.title ?? "";
  const entries = new Set(hits.map((h) => h.entry_id)).size;

  return (
    <div className="cleanupblock" data-find-replace>
      <h2 className="sub-h2">Найти в записях</h2>
      <p className="hint">
        Где стоит причина смерти, звание, родство или фамилия — например, опечатка, которая видна в подсказке.
        Регистр и «ё» не важны: «попов» найдёт и «Попов». Запись можно открыть или заменить написание во всех
        записях сразу — перед заменой программа делает копию прихода.
      </p>
      <div className="row findrow">
        <div className="sexpick" role="group" aria-label="Что искать">
          {FIELDS.map((f) => (
            <button key={f.code} type="button" className={field === f.code ? "on" : ""} data-find-field={f.code}
                    onClick={() => { setField(f.code); if (query.trim()) void look(query, f.code); }}>
              {f.title}
            </button>
          ))}
        </div>
        <input data-find-query value={query} placeholder="что искать" autoComplete="off" spellCheck={false}
               onChange={(e) => setQuery(e.target.value)}
               onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); void look(); } }} />
        <button type="button" className="toggle" data-find-go onClick={() => void look()}>Найти</button>
      </div>

      {found && found.length === 0 && <p className="hint" data-find-empty>В записях нет: «{query.trim()}» ({title}).</p>}
      {found && found.length > 0 && (
        <ul className="importnotes" data-find-spellings>
          {found.map((f) => (
            <li key={f.value}>
              <button type="button" className={picked === f.value ? "linkish on" : "linkish"} data-find-spelling={f.value}
                      onClick={() => void pick(f.value)}>
                «{f.value}»
              </button>{" "}
              <span className="fieldhint">— записей: {f.entries}</span>
            </li>
          ))}
        </ul>
      )}

      {picked && (
        <div className="findhits" data-find-hits={entries}>
          <p className="hint">
            «{picked}» ({title}) — записей: {entries}{hits.length > SHOWN ? `, показаны первые ${SHOWN}` : ""}.
          </p>
          <table className="facts">
            <tbody>
              {hits.slice(0, SHOWN).map((h, i) => (
                <tr key={`${h.entry_id}-${i}`}>
                  <td>{SECTION[h.section] ?? "запись"} {h.year ?? "без года"}{h.page ? `, стр. ${h.page}` : ""}{h.no != null ? `, № ${h.no}` : ""}</td>
                  <td>{h.who || "имени нет"}</td>
                  <td>
                    <button type="button" className="linkish" onClick={() => {
                      const forms = dirtyForms();
                      if (forms.length) {
                        setNote(`Чтобы открыть запись, сначала сохраните или очистите набранное: ${forms.join(", ")}.`);
                        return;
                      }
                      onOpenEntry(h.section, h.entry_id);
                    }}>
                      Открыть
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="row findrow">
            <span>Заменить во всех на:</span>
            <input data-find-to value={to} autoComplete="off" spellCheck={false}
                   onChange={(e) => { setTo(e.target.value); setSure(false); }}
                   onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); void replace(); } }} />
            <button type="button" className={sure ? "danger" : "toggle"} data-find-replace-go
                    disabled={busy || !to.trim() || to.trim() === picked} onClick={() => void replace()}>
              {busy ? "Заменяю…" : sure ? `Да, заменить в ${entries} записях` : "Заменить"}
            </button>
            {sure && !busy && <button type="button" className="toggle" onClick={() => setSure(false)}>Отмена</button>}
          </div>
        </div>
      )}
      {note && <p className="hint" data-find-note>{note}</p>}
    </div>
  );
}
