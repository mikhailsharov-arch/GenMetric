import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Suggest from "./Suggest";
import IofField from "./IofField";

type DbInfo = {
  names: number;
  name_forms: number;
  lookups: number;
  roles: number;
  db_path: string;
  app_version: string;
  schema_version: number;
  seed_stamp: string;
};

/**
 * Экран проверки сборки.
 *
 * Это ещё не рабочая форма ввода — она появится на следующем этапе.
 * Здесь проверяется то, что дальше ломать нельзя: форма окна, доступ
 * к базе справочников и поиск по кириллице с ранжированием подсказок.
 */
export default function App() {
  const [info, setInfo] = useState<DbInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [onTop, setOnTop] = useState(false);
  const [rank, setRank] = useState("");
  const [place, setPlace] = useState("");
  const firstField = useRef<HTMLInputElement>(null);

  useEffect(() => {
    invoke<DbInfo>("db_info").then(setInfo).catch((e) => setError(String(e)));
    firstField.current?.focus();
  }, []);

  async function toggleOnTop() {
    const next = !onTop;
    await invoke("set_always_on_top", { value: next });
    setOnTop(next);
  }

  return (
    <div className="app">
      <header>
        <h1>GenMetric</h1>
        <p className="sub">
          Индексатор метрических книг
          {info ? ` · версия ${info.app_version}` : ""}
        </p>
      </header>

      {error && (
        <div className="error">
          <b>База не открылась.</b>
          <div className="mono">{error}</div>
        </div>
      )}

      <section>
        <h2>Проверка ввода</h2>
        <p className="hint">
          Наберите несколько букв. Подсказки идут по частоте использования,
          а не по алфавиту. Выбор — стрелками и клавишей Enter, мышь не нужна.
        </p>

        <Suggest
          ref={firstField}
          label="Звание"
          kind="rank_m"
          value={rank}
          onChange={setRank}
          placeholder="например, кр"
        />
        <Suggest
          label="Причина смерти"
          kind="death_cause"
          value={place}
          onChange={setPlace}
          placeholder="например, ста"
        />
      </section>

      <section>
        <h2>Единое поле ИОФ</h2>
        <p className="hint">
          Одно поле вместо трёх. Подсказки идут пословно: первое слово —
          по именам, второе — по отчествам, третье — по фамилиям. Программа
          сама разбирает набранное на части и предлагает современное написание.
        </p>
        <IofField label="ИОФ персоны" />
      </section>

      <section>
        <h2>Окно</h2>
        <p className="hint">
          Окно должно быть узким и помещаться по высоте экрана: слева оно,
          справа скан книги.
        </p>
        <button className="toggle" onClick={toggleOnTop} aria-pressed={onTop}>
          {onTop ? "✓ Поверх других окон" : "Поверх других окон"}
        </button>
      </section>

      {info && (
        <section>
          <h2>Что внутри сборки</h2>
          <p className="hint">
            Если эти числа не изменились после установки новой версии — значит
            обновление до базы не доехало, и об этом надо сказать.
          </p>
          <table className="facts">
            <tbody>
              <tr>
                <td>Имён в словаре</td>
                <td>{info.names.toLocaleString("ru-RU")}</td>
              </tr>
              <tr>
                <td>Значений в перечнях</td>
                <td>{info.lookups.toLocaleString("ru-RU")}</td>
              </tr>
              <tr>
                <td>Написаний имён и отчеств</td>
                <td>{info.name_forms.toLocaleString("ru-RU")}</td>
              </tr>
              <tr>
                <td>Ролей персон</td>
                <td>{info.roles}</td>
              </tr>
              <tr>
                <td>Версия базы</td>
                <td>{info.schema_version}</td>
              </tr>
              <tr>
                <td>Отпечаток справочников</td>
                <td>{info.seed_stamp}</td>
              </tr>
            </tbody>
          </table>
          <p className="path mono">{info.db_path}</p>
        </section>
      )}

      <footer>
        Это сборка для проверки, а не рабочая версия. Форма ввода записей
        появится на следующем этапе.
      </footer>
    </div>
  );
}
