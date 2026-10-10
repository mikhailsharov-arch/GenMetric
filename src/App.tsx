import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import ErrorBar from "./ErrorBar";
import FontScale from "./FontScale";
import OneColumn, { useOneColumn } from "./OneColumn";
import CaseHeader, { type Case } from "./CaseHeader";
import BirthForm from "./BirthForm";
import MarriageForm from "./MarriageForm";
import DeathForm from "./DeathForm";
import { ClergyProvider } from "./clergy";
import { report, warn } from "./errors";
import { dirtyForms } from "./dirty";
import { FIELDS } from "./focus";
import ArchiveBlock from "./ArchiveBlock";
import SearchScreen, { type SearchRequest } from "./SearchScreen";

type Startup = {
  error: string | null;
  db_path: string;
  log_path: string;
  parish_id: number;
  parish_name: string;
  warning: string | null;
};

type LookupSize = { kind: string; title: string; count: number };

type DbInfo = {
  names: number;
  name_forms: number;
  lookups: number;
  places: number;
  roles: number;
  db_path: string;
  app_version: string;
  schema_version: number;
  seed_stamp: string;
  repaired_entries: number;
  unknown_sex_entries: number;
  clergy_noname_entries: number;
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
  const [oneColumn, toggleOneColumn] = useOneColumn();
  const [startup, setStartup] = useState<Startup | null>(null);
  const [lookups, setLookups] = useState<LookupSize[]>([]);
  const [mkCase, setMkCase] = useState<Case | null>(null);
  // Дело — на год книги (02.10.2026): запись другого года уводит работу в
  // дело этого года, и экран «Дело» перечитывается.
  const [caseReload, setCaseReload] = useState(0);
  // «Сохранить дело» с годом — формы встают на него (спека 2026-10-03, п. 1.2);
  // «Открыть запись» из списка на сверку — запись в форме своего раздела.
  // n — счётчик: то же значение второй раз тоже должно сработать.
  const [workYear, setWorkYear] = useState<{ year: number; n: number } | null>(null);
  const [openReq, setOpenReq] = useState<{ section: number; id: number; n: number } | null>(null);
  function openEntry(section: number, id: number) {
    setScreen(section === 1 ? "births" : section === 2 ? "marriages" : "deaths");
    setOpenReq((prev) => ({ section, id, n: (prev?.n ?? 0) + 1 }));
  }
  function entrySaved(caseId: number) {
    // Дело без года с первой записью получает её год — экран «Дело» должен
    // это узнать, иначе сохранит прежний пустой год (ревьюер 02.10.2026).
    if (mkCase && (caseId !== mkCase.id || mkCase.year === null)) setCaseReload((n) => n + 1);
  }
  type Screen = "case" | "births" | "marriages" | "deaths" | "about" | "search";
  const [screen, setScreen] = useState<Screen>("case");

  // --- Экран «Поиск» (Роман 03.10 и 09.10.2026) ---
  // Открывается кнопкой на «Деле» и Ctrl+F из любого места. Из блока персоны
  // формы — сразу с её ИОФ и НП: «в момент ввода часто возникает потребность
  // что-то быстро проверить… не прерывая и не теряя текущий набор». Формы не
  // размонтируются, поэтому набранное остаётся; Esc возвращает в то же поле.
  const [searchReq, setSearchReq] = useState<SearchRequest | null>(null);
  const cameFrom = useRef<{ screen: Screen; el: HTMLElement | null }>({ screen: "case", el: null });
  const screenRef = useRef<Screen>(screen);
  screenRef.current = screen;
  function openSearch(from?: HTMLElement | null) {
    const block = from?.closest<HTMLElement>(".formroot .person");
    const value = (label: string) => Array.from(block?.querySelectorAll<HTMLElement>(".field") ?? [])
      .find((f) => f.querySelector("label")?.textContent?.trim() === label)?.querySelector("input")?.value ?? "";
    if (screenRef.current !== "search") cameFrom.current = { screen: screenRef.current, el: from ?? null };
    setSearchReq((prev) => ({ iof: value("ИОФ"), place: value("НП"), clergy: !!block?.closest(".clergyslot"),
                              n: (prev?.n ?? 0) + 1 }));
    setScreen("search");
  }
  function closeSearch() {
    const { screen: back, el } = cameFrom.current;
    setScreen(back);
    // Курсор — в то же поле, откуда ушли: набор продолжается с того же места.
    setTimeout(() => { if (el && el.isConnected && el.offsetParent !== null) el.focus(); }, 0);
  }
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      // По коду клавиши, а не по букве: в русской раскладке это «а».
      // Клавиша без кода (так её шлёт драйвер сквозной проверки) — по номеру:
      // у физической F он 70 в любой раскладке.
      const isF = e.code === "KeyF" || (!e.code && e.keyCode === 70);
      if (!isF || !(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey) return;
      // Свой поиск вместо поиска по странице, который есть у окна программы.
      e.preventDefault();
      // Открыто окно с вопросом: сначала ответ в нём. Окно коротко мигает
      // рамкой — иначе клавиша молча не делала ничего (проверяющий 09.10.2026).
      const modal = document.querySelector<HTMLElement>(".modal");
      if (modal) {
        modal.classList.remove("nudge");
        void modal.offsetWidth; // перезапуск мигания при повторном нажатии
        modal.classList.add("nudge");
        window.setTimeout(() => modal.classList.remove("nudge"), 700);
        return;
      }
      openSearch(document.activeElement as HTMLElement | null);
    };
    document.addEventListener("keydown", on, true);
    return () => document.removeEventListener("keydown", on, true);
  }, []);

  useEffect(() => {
    invoke<Startup>("startup_state")
      .then((state) => {
        setStartup(state);
        if (state.error) return; // база не открылась, остальное бессмысленно
        // Приход открыт, но не всё гладко (справочники не сверены с общими,
        // открыт не тот приход, что в прошлый раз) — сказать, работа идёт.
        if (state.warning) report("Приход открыт с оговоркой", state.warning);
        invoke<DbInfo>("db_info")
          .then(setInfo)
          .catch((e) => report("Не удалось прочитать сведения о базе", e));
        invoke<LookupSize[]>("lookup_summary")
          .then(setLookups)
          .catch((e) => report("Не удалось прочитать состав справочников", e));
      })
      .catch((e) => report("Программа не смогла сообщить своё состояние", e));
  }, []);


  // Автопрокрутка к полю в фокусе (Роман 27.09.2026: «при перемещении
  // фокуса на скрытое поле страница должна автоматически прокручиваться так,
  // чтобы активное поле становилось полностью видимым»). Браузер сам
  // прокручивает только к полю вне окна, а поле под прилипшей кнопкой
  // «Сохранить» считает видимым — отсюда scroll-margin в styles.css.
  useEffect(() => {
    const on = (e: FocusEvent) => {
      const el = e.target as HTMLElement | null;
      // Только поля ввода. Кнопка получает фокус на нажатии мыши, и прокрутка
      // между нажатием и отпусканием уводила её из-под курсора — щелчок
      // терялся: «Сохранить изменения» не срабатывала (e2e #36, шаг 7).
      if (!el || !el.matches("input, textarea, select") || !el.closest(".formroot")
          || el.closest(".modal, .savebar")) return;
      el.scrollIntoView({ block: "nearest" });
    };
    document.addEventListener("focusin", on);
    // Кнопка «Сохранить» — продолжение полей формы (Enter на последнем поле
    // ведёт на неё, focus.ts): Shift+Enter и ↑ на ней — назад, к последнему
    // полю, а не нажатие (проверяющий #37: Shift+Enter сохранял запись).
    const back = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      if (!el?.closest(".savebar") || !((e.key === "Enter" && e.shiftKey) || e.key === "ArrowUp")) return;
      const fields = Array.from(el.closest(".formroot")?.querySelectorAll<HTMLInputElement>(FIELDS) ?? [])
        .filter((f) => !f.disabled && f.offsetParent !== null);
      const last = fields[fields.length - 1];
      if (!last) return;
      e.preventDefault();
      e.stopPropagation();
      last.focus();
      if (typeof last.select === "function") last.select();
    };
    document.addEventListener("keydown", back, true);
    return () => {
      document.removeEventListener("focusin", on);
      document.removeEventListener("keydown", back, true);
    };
  }, []);

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
    <div className={screen === "search" ? "app wide" : "app"}>
      <ErrorBar />
      {/* Заголовок, вкладки и размер шрифта — одной строкой. Высота — самый
          дефицитный ресурс: заказчику нужна вся запись на экране без прокрутки,
          пока справа открыт скан. Название программы человек и так знает. */}
      <header className="topbar">
        {/* Надписи «GenMetric» здесь нет: название и так в заголовке окна, а в
            узком окне кнопки вкладок не помещались (Роман 28.09.2026). Версия —
            во всплывающей подписи «ⓘ». */}
        <nav className="tabs">
          <button className={screen === "case" ? "on" : ""} onClick={() => setScreen("case")}>
            Дело
          </button>
          <button
            className={screen === "births" ? "on" : ""}
            onClick={() => setScreen("births")}
            disabled={!mkCase || !mkCase.id}
            title={mkCase && mkCase.id ? "" : "Сначала заполните дело"}
          >
            Рождения
          </button>
          <button
            className={screen === "marriages" ? "on" : ""}
            onClick={() => setScreen("marriages")}
            disabled={!mkCase || !mkCase.id}
            title={mkCase && mkCase.id ? "" : "Сначала заполните дело"}
          >
            Браки
          </button>
          <button
            className={screen === "deaths" ? "on" : ""}
            onClick={() => setScreen("deaths")}
            disabled={!mkCase || !mkCase.id}
            title={mkCase && mkCase.id ? "" : "Сначала заполните дело"}
          >
            Смерти
          </button>
        </nav>
        {/* «О программе» — маленькой кнопкой справа: с «Смертями» вкладок
            стало пять, и место нужно им (Роман 27.09.2026, Mike: «ⓘ»). */}
        <button className={screen === "about" ? "info on" : "info"} onClick={() => setScreen("about")}
                title={info ? `О программе · GenMetric ${info.app_version}` : "О программе"} aria-label="О программе">
          ⓘ
        </button>
        <FontScale />
      </header>

      {/* Форма рождений не размонтируется при переключении вкладок, а прячется
          стилем. Иначе набранное пропадает: заказчик 27.08.2026 — «после
          переключения на вкладку „дело“ или „о программе“ все поля во вкладке
          „Рождения“ становятся пустыми». Это была потеря работы, а не
          неудобство. */}
      <div hidden={screen !== "case"}>
        <CaseHeader onSaved={setMkCase} reload={caseReload} parishName={startup?.parish_name ?? ""}
                    onWorkYear={(year) => setWorkYear((prev) => ({ year, n: (prev?.n ?? 0) + 1 }))}
                    onOpenEntry={openEntry}
                    viewBlock={<>
                      {/* Кнопка поиска — здесь, а не в верхней строке окна: она
                          занята целиком (Роман 09.10.2026, ответ 4Б). */}
                      <div className="findblock">
                        <button type="button" className="toggle" data-find-person onClick={() => openSearch(null)}>
                          Найти персону <span className="kbd">Ctrl+F</span>
                        </button>
                        <p className="hint">
                          Все записи прихода, где встречается человек: дети, браки, смерть, у кого был восприемником
                          и поручителем. Ctrl+F из блока персоны при наборе ищет сразу её.
                        </p>
                      </div>
                      <OneColumn on={oneColumn} onToggle={toggleOneColumn} />
                    </>} />
      </div>
      <div hidden={screen !== "search"}>
        <SearchScreen active={screen === "search"} request={searchReq} onBack={closeSearch}
                      onOpenEntry={(section, id) => {
                        // Форма раздела занята — отказ здесь же, не уходя с
                        // поиска: иначе человек оказывался на форме без курсора
                        // и без поиска (проверяющий 09.10.2026). Сама форма
                        // проверяет то же ещё раз.
                        const form = ["", "Рождения", "Браки", "Смерти"][section];
                        if (dirtyForms().includes(form)) {
                          warn(`В форме «${form}» есть несохранённое`,
                               "сохраните или очистите набранное (или закончите правку открытой записи) — и откройте запись из поиска снова");
                          return;
                        }
                        openEntry(section, id);
                      }}
                      backTitle={cameFrom.current.screen === "case" || cameFrom.current.screen === "about" ? "Закрыть (Esc)" : "К набору (Esc)"} />
      </div>
      {/* Причт общий для всех разделов (27.09.2026) — clergy.tsx. Формы
          браков и смертей (25.09, 27.09) так же не размонтируются. */}
      {mkCase && mkCase.id > 0 && (
        <ClergyProvider>
          <div hidden={screen !== "births"}>
            <BirthForm mkCase={mkCase} onSaved={entrySaved} workYear={workYear} openReq={openReq} />
          </div>
          <div hidden={screen !== "marriages"}>
            <MarriageForm mkCase={mkCase} onSaved={entrySaved} workYear={workYear} openReq={openReq} />
          </div>
          <div hidden={screen !== "deaths"}>
            <DeathForm mkCase={mkCase} onSaved={entrySaved} workYear={workYear} openReq={openReq} />
          </div>
        </ClergyProvider>
      )}
      {screen === "about" && info && (
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
              {/* Справочник населённых пунктов пришёл в поставке впервые.
                  Если он не появился — обновление не доехало, и увидеть это
                  надо здесь, а не гадать над молчащим полем НП. */}
              <tr>
                <td>Населённых пунктов</td>
                <td>{info.places.toLocaleString("ru-RU")}</td>
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
          <p className="hint">
            Шрифт интерфейса — Inter, © The Inter Project Authors, лицензия SIL Open Font
            License 1.1 (текст лицензии — файл Inter-OFL.txt в папке программы).
          </p>
          {info.repaired_entries > 0 && (
            <p className="hint">
              При обновлении исправлено записей: {info.repaired_entries} — номер девочек,
              набранных до 13 сентября 2026, перенесён из мужской колонки в женскую.
              Копия базы до исправления лежит рядом с базой (файл «до-обновления»).
            </p>
          )}
          {info.unknown_sex_entries > 0 && (
            <p className="hint">
              Записей, где пол ребёнка не определён, а номер стоит в мужской колонке:{" "}
              {info.unknown_sex_entries}. Их программа не трогала — угадывать нельзя.
              В списке «Набрано» у них «№ м.»; если это девочки, скажите — поправим.
            </p>
          )}
          {info.clergy_noname_entries > 0 && (
            <p className="hint">
              Записей, где у церковнослужителя осталось только звание без имени:{" "}
              {info.clergy_noname_entries}. Так писала сборка 21–22 сентября после
              перезапуска. Откройте эти записи из списка «Набрано» и выберите причт
              из списка заново — имена восстановятся.
            </p>
          )}

          {/* Архив подсказок — здесь, а не на «Деле»: при наборе не нужен
              (Роман 05.10.2026). */}
          <ArchiveBlock />

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

      {/* На экране набора подвал не нужен: каждая строка высоты — это строка
          записи, которую человек должен видеть, не прокручивая. */}
      {(screen === "case" || screen === "about") && (
        <footer>
          Записи сохраняются в базу на вашем компьютере. Выгрузка в Familio
          и Excel — на экране «Дело», файлы ложатся в «Документы/GenMetric».
        </footer>
      )}
    </div>
  );
}