// El visor de la nota del vault.
//
// Solo LEE. Editar sigue siendo cosa de Obsidian: la nota es la fuente y esto
// es una ventana a ella. Dos editores sobre el mismo archivo se pisan, y el que
// pierde es el que no estaba mirando.
//
// El markdown se pinta a mano y no con una librería. No es orgullo: las notas
// del vault usan seis cosas —encabezados, listas, casillas, negrita, código y
// enlaces— y traerse un parser completo para eso son cuarenta kilobytes y una
// dependencia que actualizar. Lo que no se reconoce sale como texto plano, que
// es exactamente lo que un lector espera de una línea rara.

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Cuerpo {
  texto: string;
  ruta: string;
}

export function Nota({
  tareaId,
  titulo,
  alCerrar,
}: {
  tareaId: number;
  titulo: string;
  alCerrar: () => void;
}) {
  const [cuerpo, setCuerpo] = useState<Cuerpo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let vigente = true;
    setCuerpo(null);
    setError(null);
    invoke<Cuerpo>("leer_nota", { id: tareaId })
      .then((c) => vigente && setCuerpo(c))
      .catch((e) => vigente && setError(String(e)));
    return () => {
      vigente = false;
    };
  }, [tareaId]);

  return (
    <aside className="nota">
      <header className="nota__cabeza">
        <h2 className="nota__titulo">{titulo}</h2>
        <button className="chip chip--tenue" onClick={alCerrar} title="cerrar la nota">
          ×
        </button>
      </header>

      {error !== null && <p className="aviso">{error}</p>}
      {error === null && cuerpo === null && <p className="nota__cargando">Leyendo…</p>}

      {cuerpo !== null && (
        <>
          <div className="nota__cuerpo">{pintar(cuerpo.texto)}</div>
          {/* De dónde salió esto. Un visor que no dice qué archivo enseña
              obliga a adivinar cuál abrir cuando quieres cambiarlo. */}
          <footer className="nota__pie">
            <code>{cuerpo.ruta}</code>
            <button
              className="boton boton--fino"
              onClick={() => void invoke("abrir_nota", { id: tareaId })}
            >
              Abrir en Obsidian
            </button>
          </footer>
        </>
      )}
    </aside>
  );
}

// ------------------------------------------------------------- el markdown

/** Las líneas agrupadas en bloques. Un bloque es lo que se pinta de una vez:
 *  un párrafo, una lista entera, un trozo de código. */
/** Lo que empieza un bloque nuevo. Se usa para saber donde NO sigue el
 *  anterior: las notas del vault van con salto suave a los 80 caracteres, asi
 *  que la segunda linea de un punto es parte de ese punto, no un parrafo. */
const ABRE_BLOQUE = /^(#{1,4}\s|\s*[-*]\s|\s*\d+[.)]\s|\s*>|```|\s*\|)/;

function pintar(md: string) {
  const lineas = md.split("\n");
  const salida: React.ReactNode[] = [];
  let i = 0;
  let clave = 0;

  while (i < lineas.length) {
    const l = lineas[i];

    // Código en bloque: se copia crudo hasta el cierre. Nada de lo de dentro
    // se interpreta, que es justo para lo que existe.
    if (l.startsWith("```")) {
      const trozo: string[] = [];
      i++;
      while (i < lineas.length && !lineas[i].startsWith("```")) trozo.push(lineas[i++]);
      i++;
      salida.push(
        <pre key={clave++} className="md__codigo">
          <code>{trozo.join("\n")}</code>
        </pre>,
      );
      continue;
    }

    // Tablas. La "Trazabilidad" de las historias es una, y sin esto sale una
    // sopa de barras verticales. Se pide la linea separadora (`|---|`) para no
    // confundir con una tabla un parrafo que use `|` por otra cosa.
    if (l.trimStart().startsWith("|") && /^\s*\|[\s:|-]+\|\s*$/.test(lineas[i + 1] ?? "")) {
      const celdas = (fila: string) =>
        fila.trim().replace(/^\||\|$/g, "").split("|").map((c) => c.trim());
      const cabecera = celdas(l);
      i += 2;
      const cuerpo: string[][] = [];
      while (i < lineas.length && lineas[i].trimStart().startsWith("|")) {
        cuerpo.push(celdas(lineas[i]));
        i++;
      }
      salida.push(
        <div key={clave++} className="md__tabla-caja">
          <table className="md__tabla">
            <thead>
              <tr>
                {cabecera.map((c, n) => (
                  <th key={n}>{enLinea(c)}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {cuerpo.map((fila, n) => (
                <tr key={n}>
                  {fila.map((c, m) => (
                    <td key={m}>{enLinea(c)}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
      continue;
    }

    const enc = /^(#{1,4})\s+(.*)$/.exec(l);
    if (enc) {
      const nivel = enc[1].length;
      const Etiqueta = (["h3", "h4", "h5", "h6"] as const)[nivel - 1];
      salida.push(
        <Etiqueta key={clave++} className={`md__h md__h${nivel}`}>
          {enLinea(enc[2])}
        </Etiqueta>,
      );
      i++;
      continue;
    }

    if (/^\s*[-*]\s+/.test(l)) {
      const items: React.ReactNode[] = [];
      const crudos = recoger(lineas, i, /^\s*[-*]\s+/);
      i = crudos.fin;
      for (const texto of crudos.textos) {
        // `- [ ]` y `- [x]`: los criterios de aceptación del vault son eso, y
        // una casilla se lee de un vistazo donde un `[ ]` literal no.
        const caja = /^\[([ xX])\]\s*(.*)$/.exec(texto);
        items.push(
          <li key={items.length} className={caja ? "md__tarea" : undefined}>
            {caja ? (
              <>
                <span className="md__caja">{caja[1].toLowerCase() === "x" ? "☑" : "☐"}</span>
                {enLinea(caja[2])}
              </>
            ) : (
              enLinea(texto)
            )}
          </li>,
        );
      }
      salida.push(
        <ul key={clave++} className="md__lista">
          {items}
        </ul>,
      );
      continue;
    }

    // Listas numeradas. El "Flujo principal" de los casos de uso es esto, y
    // sin tratarlas los pasos se aplastan en un parrafo corrido donde ya no se
    // ve donde acaba uno y empieza el siguiente.
    if (/^\s*\d+[.)]\s+/.test(l)) {
      const items: React.ReactNode[] = [];
      // El numero del primero manda: un flujo que empieza en 3 se pinta
      // empezando en 3, no en 1.
      const primero = Number(/^\s*(\d+)/.exec(l)?.[1] ?? 1);
      const crudos = recoger(lineas, i, /^\s*\d+[.)]\s+/);
      i = crudos.fin;
      for (const texto of crudos.textos) {
        items.push(<li key={items.length}>{enLinea(texto)}</li>);
      }
      salida.push(
        <ol key={clave++} className="md__lista" start={primero}>
          {items}
        </ol>,
      );
      continue;
    }

    if (/^\s*>\s?/.test(l)) {
      const trozo: string[] = [];
      while (i < lineas.length && /^\s*>\s?/.test(lineas[i])) {
        trozo.push(lineas[i].replace(/^\s*>\s?/, ""));
        i++;
      }
      salida.push(
        <blockquote key={clave++} className="md__cita">
          {enLinea(trozo.join(" "))}
        </blockquote>,
      );
      continue;
    }

    if (l.trim() === "") {
      i++;
      continue;
    }

    // Párrafo: las líneas seguidas son el mismo párrafo, como en markdown.
    const trozo: string[] = [];
    while (
      i < lineas.length &&
      lineas[i].trim() !== "" &&
      !ABRE_BLOQUE.test(lineas[i])
    ) {
      trozo.push(lineas[i]);
      i++;
    }
    salida.push(
      <p key={clave++} className="md__p">
        {enLinea(trozo.join(" "))}
      </p>,
    );
  }

  return salida;
}

/** Los puntos de una lista, con sus lineas de continuacion ya pegadas.
 *
 *  Sin esto un punto partido en dos lineas —lo normal en el vault, que envuelve
 *  a los 80— se rompe: la negrita que abre en la primera y cierra en la segunda
 *  se queda sin cerrar y salen los asteriscos en crudo. */
function recoger(
  lineas: string[],
  desde: number,
  marca: RegExp,
): { textos: string[]; fin: number } {
  const textos: string[] = [];
  let i = desde;
  while (i < lineas.length) {
    if (marca.test(lineas[i])) {
      textos.push(lineas[i].replace(marca, ""));
      i++;
    } else if (textos.length > 0 && lineas[i].trim() !== "" && !ABRE_BLOQUE.test(lineas[i])) {
      textos[textos.length - 1] += ` ${lineas[i].trim()}`;
      i++;
    } else {
      break;
    }
  }
  return { textos, fin: i };
}

/** Negrita, código, enlaces y los `[[wikilinks]]` de Obsidian.
 *
 *  Los wikilinks se pintan como texto y no como enlace: llevan a otra nota del
 *  vault, y este visor solo sabe abrir la de la tarea. Un enlace que no lleva a
 *  ningún sitio es peor que un texto que no promete nada. */
function enLinea(s: string): React.ReactNode[] {
  const partes: React.ReactNode[] = [];
  const patron = /(\*\*[^*]+\*\*)|(`[^`]+`)|(\[\[[^\]]+\]\])|(\[[^\]]+\]\([^)]+\))/g;
  let ultimo = 0;
  let m: RegExpExecArray | null;
  let k = 0;

  while ((m = patron.exec(s)) !== null) {
    if (m.index > ultimo) partes.push(s.slice(ultimo, m.index));
    const t = m[0];
    if (t.startsWith("**")) {
      partes.push(<strong key={k++}>{t.slice(2, -2)}</strong>);
    } else if (t.startsWith("`")) {
      partes.push(<code key={k++}>{t.slice(1, -1)}</code>);
    } else if (t.startsWith("[[")) {
      // `[[ruta|como se lee]]` → se queda con lo de después de la barra.
      const dentro = t.slice(2, -2);
      partes.push(
        <span key={k++} className="md__wiki">
          {dentro.includes("|") ? dentro.slice(dentro.indexOf("|") + 1) : dentro}
        </span>,
      );
    } else {
      partes.push(<span key={k++}>{t.slice(1, t.indexOf("]"))}</span>);
    }
    ultimo = m.index + t.length;
  }
  if (ultimo < s.length) partes.push(s.slice(ultimo));
  return partes;
}
