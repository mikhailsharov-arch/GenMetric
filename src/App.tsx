import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Suggest from "./Suggest";
import IofField from "./IofField";
import ErrorBar from "./ErrorBar";
import FontScale from "./FontScale";
import { report } from "./errors";

type Startup = {
  error: string | null;
  db_path: string;
  log_path: string;
};

type LookupSize = { kind: string; title: string; count: number };

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
  const [startup, setStartup] = useState<Startup | null>(null);
  const [onTop, setOnTop] = useState(false);
  const [rankM, setRankM] = useState("");
  const [rankF, setRankF] = useState("");
  const [place, setPlace] = useState("");
  const [lookups, setLookups] = useState<LookupSize[]>([]);
  const firstField = useRef<HTMLInputElement>(null);

  useEffect(() => {
    invoke<Startup>("startup_state")
      .then((state) => {
        setStartup(state);
        if (state.error) return; // база не открылась, остальное бессмысленно
        invoke<DbInfo>("db_info")
          .then(setInfo)
          .catch((e) => report("Не удалось прочитать сведения о базе", e));
        invoke<LookupSize[]>("lookup_summary")
          .then(setLookups)
          .catch((e) => report("Не удалось прочитать состав справочников", e));
      })
      .catch((e) => report("Программа не смогла сообщить своё состояние", e));
    firstField.current?.focus();
  }, []);

  async function toggleOnTop() {
    const next = !onTop;
    try {
      await invoke("set_always_on_top", { value: next });
      setOnTop(next);
    } catch (e) {
      report("Не удалось закрепить окно поверх других", e);
    }
  }

  // База не открылась — показываем объяснение вместо формы: работать всё
  // равно нельзя, а человек должен понимать, что произошло и что делать.
  if (startup?.error) {
    return (
      <div className="app">
        <header>
          <h1>GenMetric</h1>
          <p className="sub">Индексатор метрических книг</p>
        </header>
        <section className="fatal">
          <h2>База не открылась</h2>
          <p>
            Программа запустилась, но не смогла открыть файл со справочниками
            и вашими записями. Работать в таком виде нельзя.
          </p>
          <p className="hint">Что произошло:</p>
          <pre className="errorbar-detail">{startup.error}</pre>
          <p className="hint">Что можно сделать:</p>
          <p>
            Рядом с базой лежат резервные копии — файлы, в имени которых есть
            «до-обновления». Закройте программу, переименуйте самый свежий из
            них в <b>genmetric.sqlite</b>, заменив испорченный файл, и запустите
            программу снова.
          </p>
          <p>
            Если это не помогло — удалите <b>genmetric.sqlite</b> совсем.
            Программа создаст его заново из поставки. Набранные записи при этом
            пропадут, поэтому сначала сохраните копию файла куда-нибудь ещё.
          </p>
          <p className="hint">Где лежат файлы:</p>
          <p className="path mono">{startup.db_path}</p>
          <p className="hint">Журнал ошибок:</p>
          <p className="path mono">{startup.log_path}</p>
          <p>Пришлите Михаилу текст выше — по нему видно причину.</p>
        </section>
      </div>
    );
  }

  return (
    <div className="app">
      <ErrorBar />
      <header className="apphead">
        <div>
          <h1>GenMetric</h1>
          <p className="sub">
            Индексатор метрических книг
            {info ? ` · версия ${info.app_version}` : ""}
          </p>
        </div>
        <FontScale />
      </header>


      <section>
        <h2>Проверка ввода</h2>
        <p className="hint">
          Наберите несколько букв. Подсказки идут по частоте использования,
          а не по алфавиту. Выбор — стрелками и клавишей Enter, мышь не нужна.
        </p>

        <Suggest
          ref={firstField}
          label="Звание мужское"
          kind="rank_m"
          value={rankM}
          onChange={setRankM}
          placeholder="например, кр"
          hint="Здесь только мужские звания: крестьянин, солдат, мещанин."
        />
        <Suggest
          label="Звание женское"
          kind="rank_f"
          value={rankF}
          onChange={setRankF}
          placeholder="например, кресть"
          hint="А здесь женские: крестьянская жена, крестьянская вдова, крестьянка."
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

          {lookups.length > 0 && (
            <>
              <h2 className="sub-h2">Что в справочниках</h2>
              <p className="hint">
                Если звания, которым вы пользуетесь, здесь не хватает —
                скажите, каких именно.
              </p>
              <table className="facts">
                <tbody>
                  {lookups.map((l) => (
                    <tr key={l.kind}>
                      <td>{l.title}</td>
                      <td>{l.count}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </>
          )}
        </section>
      )}

      <footer>
        Это сборка для проверки, а не рабочая версия. Форма ввода записей
        появится на следующем этапе.
      </footer>
    </div>
  );
}
