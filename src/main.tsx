import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";
import { report } from "./errors";

// Непойманная ошибка в обработчике (например, в сохранении) не должна
// пропадать молча: сборка #36 на e2e «Сохранить изменения» ничего не сделала
// и ничего не сказала. Всё непойманное — в полосу ошибок и в журнал e2e.
window.addEventListener("unhandledrejection", (e) => {
  report("Непредвиденная ошибка программы", e.reason instanceof Error ? e.reason.stack ?? e.reason : e.reason);
});
window.addEventListener("error", (e) => {
  report("Непредвиденная ошибка программы", e.error instanceof Error ? e.error.stack ?? e.message : e.message);
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
