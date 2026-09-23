import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Modal from "./Modal";
import Suggest from "./Suggest";
import { report } from "./errors";
import { focusNextField } from "./focus";

/**
 * Карточка населённого пункта при первом вводе (Роман, приоритет 2 от
 * 23.09.2026; задумано ещё 28.08: «губерния, уезд, волость, тип, чистое
 * название, ссылка на Familio»).
 *
 * Сверху — похожие из справочника: «Букарина» против «Бухарино» —
 * расхождение на третьей букве, поиск по началу молчал, и автопополнение
 * завело деревню, которой нет (заказчик 13.09.2026). Выбор похожего
 * подставляет его в поле, карточки не будет.
 *
 * Ниже — карточка: губерния и уезд по умолчанию из шапки дела, тип по
 * умолчанию «д.». Волость и ссылка — необязательны.
 */
export type Similar = { value: string; distance: number };

type Props = {
  name: string;
  similar: Similar[];
  defaults: { guberniya: string; uyezd: string };
  onPick: (name: string) => void;
  onSaved: (name: string) => void;
  onCancel: () => void;
};

export default function PlaceCard({ name, similar, defaults, onPick, onSaved, onCancel }: Props) {
  const [npType, setNpType] = useState("д.");
  const [guberniya, setGuberniya] = useState(defaults.guberniya);
  const [uyezd, setUyezd] = useState(defaults.uyezd);
  const [volost, setVolost] = useState("");
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [active, setActive] = useState(0);

  async function save() {
    if (busy) return;
    setBusy(true);
    try {
      await invoke<number>("place_save", {
        card: { name: name.trim(), np_type: npType, guberniya, uyezd, volost, familio_url: url },
      });
      onSaved(name.trim());
    } catch (e) {
      report(`Не удалось сохранить населённый пункт «${name}»`, e);
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal title={`Новый населённый пункт: «${name}»`} kind="place" onClose={onCancel}>
      {similar.length > 0 && (
        <>
          <p className="hint">Похожие названия уже есть в справочнике. Если это одно из них — Enter подставит его, карточка не понадобится. Tab — заполнить карточку нового.</p>
          {/* Список с фокусом: ↑/↓ выбирают, Enter подставляет, Tab уводит в
              карточку. Список первый в окне и получает фокус при открытии:
              «Букарина» → «Бухарино» решается одним Enter. */}
          <ul
            className="suggest static resolve"
            tabIndex={0}
            data-similar
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") { e.preventDefault(); setActive((i) => (i + 1) % similar.length); }
              else if (e.key === "ArrowUp") { e.preventDefault(); setActive((i) => (i - 1 + similar.length) % similar.length); }
              else if (e.key === "Enter") { e.preventDefault(); onPick(similar[active].value); }
            }}
          >
            {similar.map((s, i) => (
              <li
                key={s.value}
                className={i === active ? "active" : ""}
                onMouseDown={(e) => { e.preventDefault(); onPick(s.value); }}
              >
                <span className="val">{s.value}</span>
                <span className="tier t4">похоже</span>
              </li>
            ))}
          </ul>
        </>
      )}
      <p className="hint">
        {similar.length ? "Или заполните карточку нового:" : "Такого названия в справочнике нет — заполните карточку:"}
        {" "}губерния и уезд подставлены из дела, волость и ссылку можно оставить пустыми.
      </p>
      <Suggest label="Тип" kind="np_type" value={npType} onChange={setNpType} browse />
      <Suggest label="Губерния" kind="guberniya" value={guberniya} onChange={setGuberniya} browse />
      <Suggest label="Уезд" kind="uyezd" value={uyezd} onChange={setUyezd} browse />
      <div className="field">
        <label>Волость</label>
        <div className="fieldbody">
          <input data-field value={volost} onChange={(e) => setVolost(e.target.value)}
                 autoComplete="off" spellCheck={false}
                 onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); focusNextField(e.currentTarget, e.shiftKey ? -1 : 1); } }} />
        </div>
      </div>
      <div className="field">
        <label>Familio</label>
        <div className="fieldbody">
          <input data-field value={url} onChange={(e) => setUrl(e.target.value)}
                 placeholder="ссылка, если есть" autoComplete="off" spellCheck={false}
                 onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); void save(); } }} />
        </div>
      </div>
      <div className="modalbar">
        <button type="button" className="primary" disabled={busy} onClick={() => void save()}>
          {busy ? "Сохраняю…" : "Сохранить населённый пункт"}
        </button>
        <button type="button" className="toggle" onClick={onCancel}>Исправить название (Esc)</button>
      </div>
    </Modal>
  );
}
