import React from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./estilos.css";

const raiz = document.getElementById("raiz");
if (!raiz) throw new Error("falta #raiz en index.html");

createRoot(raiz).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
