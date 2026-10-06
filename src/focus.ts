/**
 * Перевод фокуса между полями ввода.
 *
 * Заказчик просил, чтобы фокус переходил не только по Tab, но и по Enter
 * и стрелке вниз: в Excel-Индексаторе переход шёл именно клавишей «вниз»,
 * и рука к этому привыкла.
 *
 * Поля помечаются атрибутом data-field, порядок берётся из порядка в DOM —
 * то есть из порядка на экране, а он повторяет порядок чтения записи в книге.
 */
/**
 * Поля, между которыми ходит фокус. Внутри модального окна (сверка имени,
 * карточка НП) — только его поля: Enter не должен уводить из окна в форму
 * под ним. Окно помечается атрибутом data-focus-scope.
 */
function fieldsAround(current: HTMLElement): HTMLInputElement[] {
  const scope = current.closest("[data-focus-scope]");
  // Открыто окно, а переход просят из формы под ним (отложенный переход
  // после выбора персоны) — фокус остаётся в окне. Пока окно жило внутри
  // поля, «следующим пустым» оказывалось его же поле поиска; с окном в body
  // фокус уводило в форму, окно оставалось без клавиатуры (стенд #37).
  if (!scope && document.querySelector(".modal")) return [];
  const root: ParentNode = scope ?? document;
  return Array.from(
    root.querySelectorAll<HTMLInputElement>("input[data-field]"),
  ).filter((el) => !el.disabled && el.offsetParent !== null);
}

/** Поля, которые клавиши перехода перепрыгивают (data-skip): месяц крещения
 *  и месяц погребения подставляются сами. Роман 05.10.2026: «логика перехода
 *  должна быть абсолютно одинаковой для всех клавиш навигации». Мышью в такое
 *  поле попасть можно, и из него клавиши ведут дальше как из обычного. */
function stops(fields: HTMLInputElement[], current: HTMLElement): HTMLInputElement[] {
  return fields.filter((el) => el === current || !el.hasAttribute("data-skip"));
}

export function focusNextField(current: HTMLElement, step: 1 | -1 = 1): void {
  const fields = stops(fieldsAround(current), current);

  const index = fields.indexOf(current as HTMLInputElement);
  if (index === -1) return;

  const next = fields[index + step];
  if (next) {
    next.focus();
    next.select();
    return;
  }
  // Последнее поле формы — дальше кнопка «Сохранить»: Enter на ней сохраняет.
  // Раньше Enter на последнем поле стоял на месте (проверяющий 27.09.2026).
  if (step === 1 && !current.closest("[data-focus-scope]"))
    current.closest(".formroot")?.querySelector<HTMLButtonElement>(".savebar button")?.focus();
}

/**
 * Переход к ближайшему следующему ПУСТОМУ полю.
 *
 * Нужен после выбора целой персоны из базы: он заполняет ИОФ, НП и звание
 * разом, и вести человека через уже заполненные поля по одному — три лишних
 * Enter на каждую подсказанную персону. Поправить заполненное всё равно можно:
 * стрелка вверх ведёт назад.
 *
 * Вызывать после того, как React применил новые значения (setTimeout 0):
 * пустота проверяется по DOM.
 */
export function focusNextEmptyField(current: HTMLElement): void {
  const fields = stops(fieldsAround(current), current);
  const index = fields.indexOf(current as HTMLInputElement);
  if (index === -1) return;
  const next = fields.slice(index + 1).find((el) => el.value.trim() === "") ?? fields[index + 1];
  if (next) {
    next.focus();
    next.select();
  }
}

/**
 * Прокрутить список подсказок к активной строке — сам список, а не страницу.
 * scrollIntoView двигал и форму под списком: строка подсказки уезжала из-под
 * руки вместе с полем (техдолг А2, с #22).
 */
export function scrollInList(el: HTMLElement | null): void {
  const list = el?.parentElement;
  if (!el || !list) return;
  if (el.closest(".modal")) {
    // Окно сверки прокручивается само, страница под ним стоит.
    el.scrollIntoView({ block: "nearest" });
    return;
  }
  // Сначала — поместить сам список на экран. Он может открыться у нижнего
  // края невысокого окна (ноутбук, рабочая область 700 px) — за краем или
  // под закреплённой внизу кнопкой «Сохранить». Тогда страница докручивается
  // ровно настолько, чтобы список поместился (но поле ввода не уходит за
  // верх), а если места всё равно мало — список укорачивается. В окне, где
  // список помещается, страница не двигается вовсе (проверяющий, 01.10.2026:
  // без этого активная строка уходила за экран).
  // «Пол» — верх кнопки «Сохранить». Пересчитывается после каждой прокрутки:
  // у конца формы кнопка перестаёт липнуть к низу окна и уезжает вверх вместе
  // со страницей — список, посчитанный по прежнему полу, ложился под неё.
  const floorNow = () => {
    const bar = Array.from(document.querySelectorAll<HTMLElement>(".savebar"))
      .find((b) => b.offsetParent !== null);
    return bar ? Math.min(window.innerHeight, bar.getBoundingClientRect().top) : window.innerHeight;
  };
  let floor = floorNow();
  let box = list.getBoundingClientRect();
  if (!list.classList.contains("up")) {
    for (let i = 0; i < 2 && box.bottom > floor; i++) {
      window.scrollBy(0, Math.min(box.bottom - floor + 4, Math.max(0, box.top - 48)));
      box = list.getBoundingClientRect();
      floor = floorNow();
    }
    if (box.bottom > floor) {
      // Докрутить некуда — у нижней персоны формы. Раньше список просто
      // укорачивался, порой до двух строк: «обрезается нижней границей окна»
      // (Роман 06.10.2026). Теперь, если над полем места больше, он
      // открывается вверх от поля.
      const body = list.parentElement?.getBoundingClientRect();
      const above = body ? body.top - 4 : 0, below = floor - box.top - 4;
      if (above > below) {
        list.classList.add("up");
        list.style.maxHeight = `${Math.max(64, Math.min(above, box.height))}px`;
      } else {
        list.style.maxHeight = `${Math.max(64, below)}px`;
      }
      box = list.getBoundingClientRect();
    }
  }
  // Затем — активную строку внутри списка.
  const item = el.getBoundingClientRect();
  if (item.top < box.top) list.scrollTop -= box.top - item.top;
  else if (item.bottom > box.bottom) list.scrollTop += item.bottom - box.bottom;
}
