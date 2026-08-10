import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Порт зафиксирован: его же ждёт Tauri в режиме разработки.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: "es2021", sourcemap: false },
});
