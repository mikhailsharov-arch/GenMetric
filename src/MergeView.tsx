/**
 * Слияние с файлом прихода помощника — вид окна «Приходы» (спека
 * 2026-10-17-sliyanie-prihodov.md). Роман 19 и 20.10.2026: помощник набирает
 * свои годы в своём приходе, его файл «регулярно, по ходу» вливается в приход
 * Романа; спорная запись — «обе сохранить и показать в списке на сверку».
 *
 * Сначала предпросмотр: годы и разделы в обоих приходах, сколько записей
 * новых, сколько уже есть, что пойдёт на сверку, где расходятся реквизиты дел
 * и карточки пунктов. Числа считает Rust тем же слиянием, которое потом
 * выполнит «Слить» (`merge::run` в транзакции, которая откатывается).
 */

type YearRow = { year: number | null; ours: number[]; theirs: number[]; added: number[] };

export type MergeSeen = {
  source: string; indexers: string; years: YearRow[];
  total: number; added: number; duplicates: number; known: number;
  edits: number; deleted: number; twins: number; gone: number;
  cases_new: string[]; case_filled: string[]; case_diffs: string[];
  places_new: number; place_filled: string[]; place_diffs: string[];
  aliases: number; warnings: string[];
  parish_name: string; copy: string | null;
};

const sum = (a: number[]) => a.reduce((x, y) => x + y, 0);
/** «12 / 3 / 7» — рождения, браки, смерти; ноль — тире. */
const triple = (a: number[]) => (sum(a) === 0 ? "—" : a.join(" / "));

/** Сколько строк списка показывать в окне. */
const SHOWN = 40;

function List({ title, lines, mark }: { title: string; lines: string[]; mark: string }) {
  if (lines.length === 0) return null;
  return (
    <>
      <p className="hint" {...{ [mark]: lines.length }}>{title}: {lines.length}.</p>
      <ul className="importnotes">
        {lines.slice(0, SHOWN).map((l) => <li key={l}>{l}</li>)}
        {lines.length > SHOWN && <li>…и ещё {lines.length - SHOWN} — все строки расхождений будут в списке на сверку.</li>}
      </ul>
    </>
  );
}

export default function MergeView({ seen, fileName, merged, busy, onRun, onBack, onDone }: {
  seen: MergeSeen; fileName: string; merged: boolean; busy: boolean;
  onRun: () => void; onBack: () => void; onDone: () => void;
}) {
  const toReview = seen.edits + seen.deleted + seen.twins;
  // Дубли тоже стоит записать: слияние запомнит их, и правка помощника в такой записи потом придёт на сверку.
  const nothing = seen.added === 0 && toReview === 0 && seen.duplicates === 0 && seen.case_filled.length === 0 && seen.aliases === 0;
  return (
    <div data-merge={merged ? "done" : "plan"}>
      <p data-merge-head>
        {merged ? "Слито: файл " : "Файл "}«{fileName}»
        {seen.source ? ` — ${seen.source}` : ""}{seen.indexers ? `; индексировал ${seen.indexers}` : ""}.{" "}
        {merged ? "Записи добавлены в приход " : "Записи будут добавлены в открытый приход "}
        <b>«{seen.parish_name}»</b>.
        {!merged && " Чтобы слить в другой приход — вернитесь назад и сначала откройте его."}
      </p>
      {!merged && seen.warnings.map((w) => <p key={w} className="reviewbusy" data-merge-warning>{w}</p>)}

      <table className="facts merge" data-merge-counts>
        <tbody>
          <tr><td>Записей в файле помощника</td><td>{seen.total}</td></tr>
          <tr><td><b>{merged ? "Добавлено новых" : "Будет добавлено новых"}</b></td><td data-merge-added><b>{seen.added}</b></td></tr>
          {seen.known > 0 && <tr><td>Уже слиты прошлыми слияниями и с тех пор у помощника не менялись</td><td data-merge-known>{seen.known}</td></tr>}
          {seen.duplicates > 0 && (
            <tr><td>Уже есть у вас — та же страница, тот же счёт и те же люди (не добавляются)</td><td data-merge-duplicates>{seen.duplicates}</td></tr>
          )}
          {seen.twins > 0 && (
            <tr><td>Та же страница и тот же счёт, а люди другие — {merged ? "сохранены" : "сохранятся"} обе записи, на сверку</td><td data-merge-twins>{seen.twins}</td></tr>
          )}
          {seen.edits > 0 && (
            <tr><td>Помощник поправил слитые раньше записи — на сверку, ваши записи не меняются</td><td data-merge-edits>{seen.edits}</td></tr>
          )}
          {seen.deleted > 0 && (
            <tr><td>У вас удалены, у помощника есть — на сверку, сами не возвращаются</td><td data-merge-deleted>{seen.deleted}</td></tr>
          )}
          {seen.gone > 0 && (
            <tr><td>Слиты раньше, а в этом файле их уже нет (помощник удалил?) — у вас остаются</td><td>{seen.gone}</td></tr>
          )}
          {seen.places_new > 0 && <tr><td>Новых населённых пунктов</td><td>{seen.places_new}</td></tr>}
          {seen.aliases > 0 && <tr><td>Написаний имён, связанных помощником со словарём</td><td>{seen.aliases}</td></tr>}
        </tbody>
      </table>

      {seen.years.length > 0 && (
        <>
          <p className="hint">По годам — рождения / браки / смерти:</p>
          <table className="facts merge years" data-merge-years>
            <thead>
              <tr><th>Год</th><th>У вас{merged ? " было" : ""}</th><th>В файле</th><th>{merged ? "Добавлено" : "Новых"}</th></tr>
            </thead>
            <tbody>
              {seen.years.map((y) => (
                <tr key={y.year ?? "нет"} className={sum(y.added) > 0 ? "" : "dim"}>
                  <td>{y.year ?? "без года"}</td><td>{triple(y.ours)}</td><td>{triple(y.theirs)}</td><td>{triple(y.added)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}

      <List title={merged ? "Заведены дела" : "Будут заведены дела"} lines={seen.cases_new} mark="data-merge-cases" />
      <List title={merged ? "Пустые реквизиты ваших дел дополнены из файла" : "Пустые реквизиты ваших дел дополнятся из файла"}
            lines={seen.case_filled} mark="data-merge-case-filled" />
      <List title="Реквизиты дел расходятся (остаются ваши; строки — и в списке на сверку)" lines={seen.case_diffs} mark="data-merge-case-diffs" />
      <List title={merged ? "Карточки ваших пунктов дополнены из файла" : "Карточки ваших пунктов дополнятся из файла"}
            lines={seen.place_filled} mark="data-merge-place-filled" />
      <List title="Карточки пунктов расходятся (остаются ваши; строки — и в списке на сверку)" lines={seen.place_diffs} mark="data-merge-place-diffs" />

      {merged ? (
        <p className="hint" data-merge-done>
          {toReview > 0 ? `На сверку: ${toReview} — список откроется сам. ` : ""}
          Файл помощника не изменён. Копия прихода до слияния: <span className="mono">{seen.copy}</span>
        </p>
      ) : (
        <p className="hint">
          {nothing ? "Нового в файле нет: всё уже слито. " : ""}
          Файл помощника программа только читает. Перед слиянием она сама делает копию вашего прихода. Тот же файл
          можно сливать снова и снова по мере набора: уже слитые записи второй раз не добавляются. Если записей в
          файле меньше, чем набрал помощник, — он скопировал файл при открытой программе: пусть закроет её и
          пришлёт файл заново.
        </p>
      )}
      <div className="modalbar">
        {merged ? (
          <button type="button" className="primary" data-autofocus data-merge-finish onClick={onDone}>Готово</button>
        ) : (
          <>
            <button type="button" className="primary" data-merge-go disabled={busy || nothing} onClick={onRun}>
              {busy ? "Сливаю…" : `Слить (новых записей: ${seen.added}${toReview > 0 ? `, на сверку: ${toReview}` : ""})`}
            </button>
            <button type="button" className="toggle" data-autofocus disabled={busy} onClick={onBack}>Назад</button>
          </>
        )}
      </div>
    </div>
  );
}
