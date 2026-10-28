import { report } from "./errors";

/**
 * Связь окна программы с окном скана (спека 2026-10-27-skan-oknom-i-dela).
 *
 * Источник истины — окно программы: оно шлёт окну скана, что показать
 * (`scan-sync`, `scan-pan`), окно скана отвечает действиями человека
 * (`scan-act`) и тем, что видно (`scan-view`). Имена событий у направлений
 * разные: `listen` Tauri слышит события, посланные любому окну.
 *
 * В программе — события Tauri; на стенде (браузер, Tauri нет) — канал
 * `BroadcastChannel` между двумя страницами одного адреса.
 */

const TAURI = "__TAURI_INTERNALS__" in window;

/** Эта страница — окно скана: метка окна Tauri, на стенде — адрес «?scan». */
export const isScanWindow: boolean = TAURI
  ? (window as unknown as { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } } })
      .__TAURI_INTERNALS__?.metadata?.currentWindow?.label === "scan"
  : new URLSearchParams(window.location.search).has("scan");

const ME = isScanWindow ? "scan" : "main";
const channel = !TAURI && "BroadcastChannel" in window ? new BroadcastChannel("genmetric-scan") : null;

/** Послать событие окну `to`. */
export function send(to: "main" | "scan", event: string, payload: unknown): void {
  if (!TAURI) {
    channel?.postMessage({ to, event, payload });
    return;
  }
  import("@tauri-apps/api/event")
    .then(({ emitTo }) => emitTo(to, event, payload))
    .catch((e) => report(to === "scan" ? "Окно скана не получило, что показать" : "Окно программы не получило действие из окна скана", e));
}

/** Слушать событие, посланное этому окну. Возвращает отписку. */
export function listen<T>(event: string, fn: (payload: T) => void): () => void {
  if (!TAURI) {
    const on = (m: MessageEvent) => {
      if (m.data?.to === ME && m.data.event === event) fn(m.data.payload as T);
    };
    channel?.addEventListener("message", on);
    return () => channel?.removeEventListener("message", on);
  }
  let off: (() => void) | undefined;
  let gone = false;
  import("@tauri-apps/api/event")
    .then(({ listen: on }) => on<T>(event, (e) => fn(e.payload)))
    .then((unlisten) => { if (gone) unlisten(); else off = unlisten; })
    .catch((e) => report("Связь окна программы с окном скана не установилась", e));
  return () => { gone = true; off?.(); };
}

/** Что окно скана показывает: состояние скана окна программы. */
export type ScanSync = {
  on: boolean;
  year: number | null;
  dir: string | null;
  sep: string;
  files: string[];
  missing: string | null;
  index: number;
  away: "edit" | "link" | null;
  keepPlace: boolean;
};

/** Действие человека в окне скана. */
export type ScanAct =
  | { kind: "ready" }
  | { kind: "step"; by: 1 | -1 }
  | { kind: "goto"; index: number }
  | { kind: "keep"; on: boolean }
  | { kind: "pick" }
  | { kind: "back" }
  | { kind: "hide" }
  | { kind: "type"; text: string }
  /** Enter, Tab, Esc, Ctrl+Enter, нажатые в окне скана, — полю формы. */
  | { kind: "key"; key: "Enter" | "Tab" | "Escape"; ctrl: boolean; meta: boolean; shift: boolean };

/** Что видно в окне скана: открытый файл и помещается ли лист по ширине
 *  (тогда Alt+← и Alt+→ остаются полю формы). */
export type ScanView = { loaded: string | null; fits: boolean };
