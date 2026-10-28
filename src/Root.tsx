import App from "./App";
import { ScanWindow } from "./scan";
import { isScanWindow } from "./scanlink";

/** Окно программы или окно скана (спека 2026-10-27-skan-oknom-i-dela): та же
 *  страница, режим — по метке окна. */
export default function Root() {
  return isScanWindow ? <ScanWindow /> : <App />;
}
