import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Modal from "./Modal";
import { report } from "./errors";

/**
 * Очистка неиспользуемых значений справочников (З3.4, спека
 * 2026-11-04-ochistka-spravochnikov-i-dolg.md). Роман 20.10.2026: опечатки
 * «намертво оседают в справочниках … При этом в самой базе (в реальных
 * записях) их больше нет»; кнопку «можно скрыть куда-то в настройки» — она
 * на экране «ⓘ О программе», рядом с архивом подсказок.
 *
 * Список считает Rust (`cleanup_find`): занятое — в записях, делах и
 * карточках пунктов прихода; память из архива Excel не предлагается. Ничего
 * не удаляется без «Убрать отмеченные»; перед удалением — копия прихода.
 */

type Unused = { key: string; group: string; value: string; detail: string; uses: number };
type Cleaned = { removed: number; copy: string };

/** Столько раз и больше подсказка помнит значение — скорее настоящее, чем
 *  опечатка (проверяющий 04.11.2026: «Ефим» ×49 и «Ефимов» ×91 из архива
 *  Excel, тронутые позже архива): изначально не отмечено. Опечатки — единицы. */
const OFTEN = 5;

/** Порядок групп в окне. */
const GROUPS = ["Имена", "Отчества", "Фамилии", "Звания", "Причт", "Персоны в подсказке",
                "Пары «муж — жена»", "Населённые пункты", "Прочие перечни"];

export default function CleanupBlock() {
  const [found, setFound] = useState<Unused[] | null>(null);
  const [off, setOff] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<Cleaned | null>(null);

  async function look() {
    setBusy(true);
    try {
      const list = await invoke<Unused[]>("cleanup_find");
      setFound(list);
      setOff(new Set(list.filter((u) => u.uses >= OFTEN).map((u) => u.key)));
      setDone(null);
    } catch (e) {
      report("Не удалось найти неиспользуемые значения", e);
    } finally {
      setBusy(false);
    }
  }

  async function clean() {
    if (!found || busy) return;
    const keys = found.filter((u) => !off.has(u.key)).map((u) => u.key);
    if (keys.length === 0) return;
    setBusy(true);
    try {
      setDone(await invoke<Cleaned>("cleanup_remove", { keys }));
    } catch (e) {
      report("Не удалось убрать неиспользуемые значения", e);
    } finally {
      setBusy(false);
    }
  }

  function close() {
    setFound(null);
    setDone(null);
  }

  const toggle = (keys: string[], on: boolean) =>
    setOff((was) => {
      const next = new Set(was);
      for (const k of keys) {
        if (on) next.delete(k);
        else next.add(k);
      }
      return next;
    });

  const chosen = found ? found.filter((u) => !off.has(u.key)).length : 0;

  return (
    <div className="cleanupblock" data-cleanup>
      <h2 className="sub-h2">Неиспользуемые значения справочников</h2>
      <p className="hint">
        Опечатки, которых больше нет ни в одной записи этого прихода, могли остаться в подсказках, в списке
        причта и в перечнях. Программа найдёт их и покажет списком — ничего не убирается, пока вы не нажмёте
        «Убрать отмеченные». Перед удалением программа делает копию прихода. Память из архива Excel и значения
        перечней, занятые в других ваших приходах, в список не попадают.
      </p>
      <button type="button" className="toggle" data-cleanup-find disabled={busy} onClick={() => void look()}>
        {busy && !found ? "Ищу…" : "Найти неиспользуемые…"}
      </button>

      {found && done && (
        <Modal key="done" title="Справочники очищены" kind="cleanup" onClose={close}>
          <p className="hint" data-cleanup-done>
            Убрано значений: {done.removed}. Копия прихода до очистки: <span className="mono">{done.copy}</span>
          </p>
          <div className="modalbar">
            <button type="button" className="primary" data-autofocus onClick={close}>Готово</button>
          </div>
        </Modal>
      )}

      {found && !done && (
        <Modal key="list" title="Неиспользуемые значения справочников" kind="cleanup" onClose={busy ? () => {} : close}>
          {found.length === 0 ? (
            <p className="hint" data-cleanup-empty>
              Ничего не нашлось: всё, что помнят подсказки, стоит в записях этого прихода (или пришло из архива Excel).
            </p>
          ) : (
            <>
              <p className="hint">
                Этих значений нет ни в одной записи, деле и карточке пункта этого прихода. «Помнит» — сколько раз
                значение было в подсказке; опечатки обычно единицы. Значения, которые подсказка помнит {OFTEN} раз и
                больше, не отмечены: скорее всего, это не опечатки — отметьте их сами, если уверены. Снимите отметку с
                того, что нужно оставить. Перед удалением программа сделает копию прихода.
              </p>
              <div className="cleanuplist">
                {GROUPS.map((g) => {
                  const items = found.filter((u) => u.group === g);
                  if (items.length === 0) return null;
                  const keys = items.map((u) => u.key);
                  const allOn = keys.every((k) => !off.has(k));
                  return (
                    <div key={g} data-cleanup-group={g}>
                      <h3 className="cleanuphead">
                        {g}: {items.length}{" "}
                        <button type="button" className="linkish" onClick={() => toggle(keys, !allOn)}>
                          {allOn ? "снять все" : "отметить все"}
                        </button>
                      </h3>
                      {items.map((u) => (
                        <label key={u.key} className="cleanupitem" data-cleanup-item={u.value}>
                          <input type="checkbox" checked={!off.has(u.key)} onChange={(e) => toggle([u.key], e.target.checked)} />
                          {" "}<b>{u.value}</b>
                          {u.detail && <span className="fieldhint"> — {u.detail}</span>}
                          {u.uses > 0 && <span className="fieldhint"> (помнит: {u.uses})</span>}
                          {u.uses >= OFTEN && <span className="fieldhint"> — часто, скорее не опечатка</span>}
                        </label>
                      ))}
                    </div>
                  );
                })}
              </div>
            </>
          )}
          <div className="modalbar">
            {found.length > 0 && (
              <button type="button" className="primary" data-cleanup-go disabled={busy || chosen === 0} onClick={() => void clean()}>
                {busy ? "Убираю…" : `Убрать отмеченные (${chosen})`}
              </button>
            )}
            <button type="button" className="toggle" data-autofocus onClick={close}>Закрыть (Esc)</button>
          </div>
        </Modal>
      )}
    </div>
  );
}
