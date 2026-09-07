// La conexión con el daemon. Todo lo que la ventana sabe del tiempo entra por
// aquí: no hay ningún `setInterval` contando por su cuenta, porque dos relojes
// se separan y el bueno siempre es el del daemon.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import type { Orden, Proyecto, Snapshot, Tarea, Tema } from "./tipos";

export function useReloj() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [caido, setCaido] = useState<string | null>(null);

  useEffect(() => {
    let vivo = true;

    // El primer estado se pide, no se espera: con el reloj parado el daemon no
    // empuja nada y la ventana se quedaría en blanco hasta el primer pomodoro.
    invoke<Snapshot>("estado")
      .then((s) => vivo && (setSnap(s), setCaido(null)))
      .catch((e) => vivo && setCaido(String(e)));

    const paraSnap = listen<Snapshot>("snapshot", (ev) => {
      setSnap(ev.payload);
      setCaido(null);
    });
    const paraCaida = listen<string>("reloj-caido", (ev) => setCaido(ev.payload));

    return () => {
      vivo = false;
      paraSnap.then((f) => f());
      paraCaida.then((f) => f());
    };
  }, []);

  async function mandar(orden: Orden): Promise<string | null> {
    try {
      setSnap(await invoke<Snapshot>("mandar", { orden }));
      return null;
    } catch (e) {
      // Una regla del método diciendo que no llega por aquí, y no es un fallo:
      // es la respuesta, y hay que enseñarla.
      return String(e);
    }
  }

  return { snap, caido, mandar };
}

export function useTema() {
  const [tema, setTema] = useState<Tema | null>(null);

  useEffect(() => {
    invoke<Tema>("tema").then(setTema);
    const para = listen<Tema>("tema", (ev) => setTema(ev.payload));
    return () => {
      para.then((f) => f());
    };
  }, []);

  useEffect(() => {
    if (!tema) return;
    const raiz = document.documentElement;
    raiz.style.setProperty("--fondo", tema.background);
    raiz.style.setProperty("--tinta", tema.foreground);
    raiz.style.setProperty("--acento", tema.accent);
    raiz.style.setProperty("--seleccion", tema.selection_background);
    raiz.dataset.tema = tema.oscuro ? "oscuro" : "claro";
  }, [tema]);

  return tema;
}

/** Las tareas y proyectos, releídos cuando el reloj cambia de estado. */
export function useTareas(pista: unknown) {
  const [tareas, setTareas] = useState<Tarea[]>([]);
  const [proyectos, setProyectos] = useState<Proyecto[]>([]);

  useEffect(() => {
    invoke<Tarea[]>("tareas").then(setTareas).catch(() => setTareas([]));
    invoke<Proyecto[]>("proyectos").then(setProyectos).catch(() => setProyectos([]));
  }, [pista]);

  return { tareas, proyectos };
}
