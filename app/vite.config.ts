import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// El puerto va fijo porque tauri.conf.json lo tiene escrito: si vite se mueve
// a otro, la ventana de desarrollo abre en blanco sin decir por qué.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: "es2022" },
});
