import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Modal from "./Modal";
import { report } from "./errors";
import { dirtyForms } from "./dirty";

/**
 * Дозаполнение фамилий жён по мужьям (З4.4, спека 2026-10-20-familii-zhyon.md).
 * Роман 12.10.2026: «Я начинал индексировать часть метрик, не вбивая фамилии
 * женам… Нужен инструмент (скрипт) для проверки и массового дозаполнения
 * БД». Только открытый приход (ответ 4.4). Сначала предпросмотр: что
 * дописывается (отмечено) и что «Проверьте вручную» (не правится, «Открыть» —
 * запись на правку). Перед правкой — копия прихода.
 */

type Pair = { wife_id: number; entry_id: number; section: number; year: number | null; page: string | null;
              husband: string; wife: string; proposed: string; reason: string };
type Plan = { fill: Pair[]; check: Pair[]; skipped: number };
type Done = { fixed: number; copy: string };

const where = (p: Pair) =>
  `${p.section === 1 ? "рождение" : "смерть"}${p.year ? `, ${p.year}` : ""}${p.page ? `, стр. ${p.page}` : ""}`;

export default function WivesBlock({ onOpenEntry }: { onOpenEntry: (section: number, id: number) => void }) {
  const [plan, setPlan] = useState<Plan | null>(null);
  const [off, setOff] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<Done | null>(null);
  const [refused, setRefused] = useState<string | null>(null);

  async function look() {
    setBusy(true);
    try {
      setPlan(await invoke<Plan>("wives_plan"));
      setOff(new Set());
      setDone(null);
      setRefused(null);
    } catch (e) {
      report("Не удалось подобрать фамилии жён", e);
    } finally {
      setBusy(false);
    }
  }

  async function apply() {
    if (!plan || busy) return;
    // Запись на правке перезаписала бы дописанную фамилию прежней.
    const forms = dirtyForms();
    if (forms.length) {
      setRefused(`Сначала сохраните или очистите набранное: ${forms.join(", ")}.`);
      return;
    }
    const ids = plan.fill.filter((p) => !off.has(p.wife_id)).map((p) => p.wife_id);
    if (ids.length === 0) return;
    setBusy(true);
    try {
      setDone(await invoke<Done>("wives_apply", { ids }));
    } catch (e) {
      report("Не удалось дописать фамилии жён", e);
    } finally {
      setBusy(false);
    }
  }

  function close() {
    setPlan(null);
    setDone(null);
    setRefused(null);
  }

  const chosen = plan ? plan.fill.filter((p) => !off.has(p.wife_id)).length : 0;
  const allOn = plan ? plan.fill.every((p) => !off.has(p.wife_id)) : true;

  return (
    <div className="cleanupblock" data-wives>
      <h2 className="sub-h2">Фамилии жён по мужьям</h2>
      <p className="hint">
        Для записей, где у матери (у жены умершего, у умершей жены) фамилия не записана, а у мужа есть: программа
        предложит дописать её женской формой фамилии мужа. Другие фамилии не меняются — их она покажет списком
        «Проверьте вручную». Только этот приход; перед правкой — копия.
      </p>
      <button type="button" className="toggle" data-wives-find disabled={busy} onClick={() => void look()}>
        {busy && !plan ? "Ищу…" : "Дозаполнить фамилии жён…"}
      </button>

      {plan && done && (
        <Modal key="done" title="Фамилии жён дописаны" kind="wives" onClose={close}>
          <p className="hint" data-wives-done>
            Дописано фамилий: {done.fixed}. На ручной проверке: {plan.check.length}. Копия прихода до правки:{" "}
            <span className="mono">{done.copy}</span>
          </p>
          <div className="modalbar">
            <button type="button" className="primary" data-autofocus onClick={close}>Готово</button>
          </div>
        </Modal>
      )}

      {plan && !done && (
        <Modal key="plan" title="Фамилии жён по мужьям" kind="wives" onClose={busy ? () => {} : close}>
          {refused && <p className="refused">{refused}</p>}
          {plan.fill.length === 0 && plan.check.length === 0 ? (
            <p className="hint" data-wives-empty>Дописывать нечего: у всех жён фамилия записана или у мужа её нет.</p>
          ) : (
            <div className="cleanuplist">
              <p className="hint" data-wives-summary>
                <b>Дописать: {plan.fill.length}. Проверьте вручную: {plan.check.length}.</b>
              </p>
              {plan.check.length > 0 && (
                <div data-wives-check>
                  <h3 className="cleanuphead">Проверьте вручную: {plan.check.length}</h3>
                  <p className="hint">Программа их не меняет. «Открыть» — запись на правку.</p>
                  {plan.check.map((p) => (
                    <div key={p.wife_id} className="cleanupitem" data-wives-check-item={p.wife}>
                      <b>{p.wife}</b>
                      <span className="fieldhint"> — муж {p.husband}; {p.reason}; {where(p)}</span>{" "}
                      <button type="button" className="linkish" onClick={() => {
                        // Форма с набранным запись не откроет — сказать здесь, не теряя список.
                        const forms = dirtyForms();
                        if (forms.length) {
                          setRefused(`Чтобы открыть запись, сначала сохраните или очистите набранное: ${forms.join(", ")}.`);
                          return;
                        }
                        close();
                        onOpenEntry(p.section, p.entry_id);
                      }}>
                        Открыть
                      </button>
                    </div>
                  ))}
                </div>
              )}
              {plan.fill.length > 0 && (
                <div data-wives-fill>
                  <h3 className="cleanuphead">
                    Дописать: {plan.fill.length}{" "}
                    <button type="button" className="linkish"
                            onClick={() => setOff(allOn ? new Set(plan.fill.map((p) => p.wife_id)) : new Set())}>
                      {allOn ? "снять все" : "отметить все"}
                    </button>
                  </h3>
                  {plan.fill.map((p) => (
                    <label key={p.wife_id} className="cleanupitem" data-wives-item={p.wife}>
                      <input type="checkbox" checked={!off.has(p.wife_id)}
                             onChange={(e) => setOff((was) => {
                               const next = new Set(was);
                               if (e.target.checked) next.delete(p.wife_id);
                               else next.add(p.wife_id);
                               return next;
                             })} />
                      {" "}<b>{p.wife}</b> → <b>{p.wife} {p.proposed}</b>
                      <span className="fieldhint"> — муж {p.husband}; {where(p)}</span>
                    </label>
                  ))}
                </div>
              )}
              {plan.skipped > 0 && (
                <p className="hint">Не рассматривались пары: {plan.skipped} — у мужа фамилия не записана или нечитаема, или у жены нет имени.</p>
              )}
            </div>
          )}
          <div className="modalbar">
            {plan.fill.length > 0 && (
              <button type="button" className="primary" data-wives-go disabled={busy || chosen === 0} onClick={() => void apply()}>
                {busy ? "Дописываю…" : `Дописать отмеченные (${chosen})`}
              </button>
            )}
            <button type="button" className="toggle" data-autofocus onClick={close}>Закрыть (Esc)</button>
          </div>
        </Modal>
      )}
    </div>
  );
}
