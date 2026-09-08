// El filtro por proyecto.
//
// Vivía dentro del tablero. La vista de la taza necesita EXACTAMENTE la misma
// regla —elegir un padre incluye a sus hijos— y dos copias de la misma regla
// acaban separándose: una contaría `strapp / TL más` dentro de `strapp` y la
// otra no, y el mismo proyecto enseñaría dos listas distintas según la pestaña.

import { useEffect, useMemo, useState } from "react";
import type { Proyecto } from "./tipos";

/**
 * El proyecto elegido y todos sus descendientes.
 *
 * `null` es "sin filtro", que NO es lo mismo que un conjunto vacío: vacío no
 * deja pasar nada. Quien lo consuma tiene que distinguirlos.
 */
export function useConDescendientes(
  filtro: number | null,
  proyectos: Proyecto[],
): Set<number> | null {
  return useMemo(() => {
    if (filtro === null) return null;
    const dentro = new Set<number>([filtro]);
    let creció = true;
    while (creció) {
      creció = false;
      for (const p of proyectos) {
        if (p.parent_id !== null && dentro.has(p.parent_id) && !dentro.has(p.id)) {
          dentro.add(p.id);
          creció = true;
        }
      }
    }
    return dentro;
  }, [filtro, proyectos]);
}

/**
 * Un filtro que sobrevive a cerrar la ventana.
 *
 * Es una preferencia, no un dato: si el almacenamiento no está, se arranca sin
 * filtro y ya. Lo que sí importa es que un proyecto BORRADO no deje la lista
 * vacía para siempre —el usuario vería "no hay tareas" sin ninguna pista de por
 * qué—, así que en cuanto se sabe qué proyectos existen, un id que ya no está
 * se suelta.
 */
export function useFiltroRecordado(
  clave: string,
  proyectos: Proyecto[],
): [number | null, (v: number | null) => void] {
  const [filtro, setFiltro] = useState<number | null>(() => {
    try {
      const guardado = Number(localStorage.getItem(clave));
      return Number.isInteger(guardado) && guardado > 0 ? guardado : null;
    } catch {
      return null;
    }
  });

  useEffect(() => {
    // Con la lista todavía vacía no se sabe nada: esperar es lo correcto, o el
    // filtro se borraría solo en cada arranque antes de que carguen.
    if (filtro === null || proyectos.length === 0) return;
    if (!proyectos.some((p) => p.id === filtro)) recordar(null);
  }, [filtro, proyectos]);

  function recordar(v: number | null) {
    setFiltro(v);
    try {
      if (v === null) localStorage.removeItem(clave);
      else localStorage.setItem(clave, String(v));
    } catch {
      // Sin almacenamiento el filtro funciona igual; solo no se recuerda.
    }
  }

  return [filtro, recordar];
}

export function FiltroProyecto({
  proyectos,
  valor,
  alCambiar,
  todos = "Todos los proyectos",
}: {
  proyectos: Proyecto[];
  valor: number | null;
  alCambiar: (v: number | null) => void;
  /** Qué dice la opción de "sin filtro". La taza la quiere más corta. */
  todos?: string;
}) {
  return (
    <select
      className="selector"
      value={valor ?? ""}
      onChange={(e) => alCambiar(e.target.value === "" ? null : Number(e.target.value))}
    >
      <option value="">{todos}</option>
      {proyectos.map((p) => (
        <option key={p.id} value={p.id}>
          {/* La sangría es lo que hace que el desplegable se lea como árbol. */}
          {"  ".repeat(p.nivel) + p.name}
        </option>
      ))}
    </select>
  );
}
