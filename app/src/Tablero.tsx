// El tablero.
//
// Las columnas son los estados de una tarea, pero arrastrar una tarjeta **no
// escribe el estado**: se lo pide al backend, que decide si eso es cosa del
// reloj o de la base. Si la interfaz escribiera el estado por su cuenta,
// soltar una tarjeta en "en curso" dejaría una tarea marcada como
// trabajándose sin ningún pomodoro detrás.
//
// El arrastre es el nativo del navegador, sin librería: son cuatro columnas y
// una tarjeta, y no compensa traerse cuarenta kilobytes para eso. Tiene dos
// trampas y las dos costaron que no funcionara nada:
//
//   - Tauri engancha el drag-and-drop del SISTEMA —soltar archivos sobre la
//     ventana— y al hacerlo se traga los eventos de arrastre de la propia
//     página. Se apaga con `dragDropEnabled: false` en tauri.conf.json.
//   - WebKit exige que `dragstart` escriba algo en `dataTransfer`. Sin eso el
//     arrastre se ve, pero el `drop` no llega nunca.
//
// El id de la tarjeta viaja DENTRO del dataTransfer y no solo en el estado de
// React: es el dato que el navegador garantiza que llega al drop.

import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { FiltroProyecto, useConDescendientes } from "./filtro";
import { Nota } from "./Nota";
import {
  ETIQUETA_PRIORIDAD,
  type Prioridad,
  type Proyecto,
  type Snapshot,
  type Tarea,
} from "./tipos";

type Columna = "pending" | "in_progress" | "paused" | "done";

const COLUMNAS: { id: Columna; titulo: string; corto: string; pista: string }[] = [
  { id: "pending", titulo: "Pendientes", corto: "pendiente", pista: "sin empezar" },
  { id: "in_progress", titulo: "En curso", corto: "en curso", pista: "soltar aquí sirve el café" },
  { id: "paused", titulo: "En el refri", corto: "al refri", pista: "empezadas y aparcadas" },
  { id: "done", titulo: "Hechas", corto: "hecha", pista: "esperando el bote" },
];

interface Props {
  tareas: Tarea[];
  proyectos: Proyecto[];
  snap: Snapshot;
  recargar: () => void;
}

export function Tablero({ tareas, proyectos, snap, recargar }: Props) {
  const [filtro, setFiltro] = useState<number | null>(null);
  const [arrastrando, setArrastrando] = useState<number | null>(null);
  const [encima, setEncima] = useState<Columna | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [componiendo, setComponiendo] = useState(false);
  // Que nota se esta leyendo. Vive aqui y no en la tarjeta porque el panel es
  // del tablero: solo se lee una a la vez, y abrir otra sustituye a la de antes.
  const [leyendo, setLeyendo] = useState<number | null>(null);

  // Filtrar por un proyecto incluye a sus hijos: elegir "personal" y no ver lo
  // de "personal / labs" sería una jerarquía de adorno. La regla la lleva
  // `filtro.tsx` porque la vista de la taza filtra igual.
  const conDescendientes = useConDescendientes(filtro, proyectos);

  const visibles = useMemo(
    () =>
      tareas.filter(
        (t) =>
          t.state !== "archived" &&
          (conDescendientes === null || conDescendientes.has(t.project_id)),
      ),
    [tareas, conDescendientes],
  );

  const enElBote = visibles.filter((t) => t.state === "done").length;

  async function mover(id: number, columna: Columna) {
    try {
      await invoke("mover_a_columna", { id, columna });
      setError(null);
    } catch (e) {
      // Una regla del método diciendo que no es la respuesta, no un fallo.
      setError(String(e));
    }
    recargar();
  }

  async function soltar(columna: Columna, cargado: string) {
    // El id viene del dataTransfer, con el estado de React como red de
    // seguridad: si el arrastre empezó antes de un re-render, `arrastrando`
    // puede haberse perdido, pero el dato que lleva el navegador no.
    const id = Number(cargado) || arrastrando;
    setArrastrando(null);
    setEncima(null);
    if (id) await mover(id, columna);
  }

  async function vaciar() {
    try {
      await invoke<number>("vaciar_papelera");
      setError(null);
    } catch (e) {
      setError(String(e));
    }
    recargar();
  }

  return (
    <div className="tablero">
      <div className="tablero__barra">
        <FiltroProyecto proyectos={proyectos} valor={filtro} alCambiar={setFiltro} />
        <button className="boton boton--fino" onClick={() => setComponiendo((v) => !v)}>
          {componiendo ? "Cancelar" : "Nueva tarea"}
        </button>
        <Bote cuantas={enElBote} alVaciar={vaciar} />
      </div>

      {componiendo && (
        <Compositor
          proyectos={proyectos}
          proyectoSugerido={filtro}
          alCrear={() => {
            setComponiendo(false);
            recargar();
          }}
          alFallar={setError}
        />
      )}

      {error && (
        <p className="aviso" onClick={() => setError(null)}>
          {error}
        </p>
      )}

      <div className={`tablero__cuerpo${leyendo !== null ? " tablero__cuerpo--con-nota" : ""}`}>
      <div className="columnas">
        {COLUMNAS.map((col) => {
          const suyas = visibles.filter((t) => t.state === col.id);
          return (
            <section
              key={col.id}
              className={`columna${encima === col.id ? " columna--diana" : ""}`}
              onDragOver={(e) => {
                // Sin esto el navegador no deja soltar. No es opcional.
                e.preventDefault();
                e.dataTransfer.dropEffect = "move";
                setEncima(col.id);
              }}
              onDragLeave={() => setEncima((c) => (c === col.id ? null : c))}
              onDrop={(e) => {
                e.preventDefault();
                soltar(col.id, e.dataTransfer.getData("text/plain"));
              }}
            >
              <header className="columna__cabeza">
                <h2>{col.titulo}</h2>
                <span className="columna__cuenta">{suyas.length}</span>
              </header>
              <p className="columna__pista">{col.pista}</p>

              <ul className="columna__lista">
                {suyas.map((t) => (
                  <Tarjeta
                    key={t.id}
                    tarea={t}
                    activa={snap.task?.id === t.id && snap.phase !== "idle"}
                    alArrastrar={() => setArrastrando(t.id)}
                    alSoltarse={() => setArrastrando(null)}
                    recargar={recargar}
                    alFallar={setError}
                    alMover={(columna) => mover(t.id, columna)}
                    alLeer={t.vault_note === null ? null : () => setLeyendo(t.id)}
                  />
                ))}
              </ul>
            </section>
          );
        })}
      </div>

      {/* La nota se cierra sola si la tarea deja de estar a la vista: un panel
          leyendo algo que el filtro ya escondio confunde mas de lo que sirve. */}
      {leyendo !== null && (() => {
        const t = visibles.find((v) => v.id === leyendo);
        if (!t) return null;
        return <Nota tareaId={t.id} titulo={t.title} alCerrar={() => setLeyendo(null)} />;
      })()}
      </div>
    </div>
  );
}

// ------------------------------------------------------------- la tarjeta

function Tarjeta({
  tarea,
  activa,
  alArrastrar,
  alSoltarse,
  recargar,
  alFallar,
  alMover,
  alLeer,
}: {
  tarea: Tarea;
  activa: boolean;
  alArrastrar: () => void;
  alSoltarse: () => void;
  recargar: () => void;
  alFallar: (e: string) => void;
  alMover: (columna: Columna) => void;
  /** `null` cuando la tarea no vino del vault: entonces no hay nota que leer. */
  alLeer: (() => void) | null;
}) {
  const [abierta, setAbierta] = useState(false);
  // La prioridad se congela con el primer pomodoro. La interfaz lo enseña en
  // vez de dejar intentarlo y fallar: una regla que solo aparece al chocarse
  // con ella parece un error de la aplicación.
  //
  // La condición es `first_started_at`, la MISMA que comprueba el backend. Con
  // una copia distinta —"tiene pomodoros" o "no está pendiente"— las dos se
  // separan: una tarea reabierta tras un pomodoro anulado saldría editable y
  // el guardado fallaría sin que nadie entendiera por qué.
  const congelada = tarea.first_started_at !== null;

  async function prioridad(p: Prioridad) {
    try {
      await invoke("cambiar_prioridad", { id: tarea.id, priority: p });
    } catch (e) {
      alFallar(String(e));
    }
    recargar();
  }

  // Apartar conserva el historial; borrar no. Son gestos distintos y por eso
  // este no pide confirmación: lo que hace es reversible con `coffe task reopen`.
  async function archivar() {
    try {
      await invoke("archivar_tarea", { id: tarea.id });
    } catch (e) {
      // Archivar la que tiene el reloj encima se rechaza, y el porqué viene
      // del backend: es la misma regla que usa la CLI.
      alFallar(String(e));
    }
    recargar();
  }

  async function borrar() {
    try {
      await invoke("borrar_tarea", { id: tarea.id, force: false });
    } catch (e) {
      alFallar(String(e));
    }
    recargar();
  }

  return (
    <li
      className={`tarjeta${activa ? " tarjeta--activa" : ""}`}
      draggable
      onDragStart={(e) => {
        // WebKit no considera valido un arrastre que no escribe nada aqui: sin
        // esta linea la tarjeta se arrastra y el `drop` no llega nunca.
        e.dataTransfer.setData("text/plain", String(tarea.id));
        e.dataTransfer.effectAllowed = "move";
        alArrastrar();
      }}
      onDragEnd={alSoltarse}
    >
      <button className="tarjeta__cuerpo" onClick={() => setAbierta((v) => !v)}>
        <span className={`prioridad prioridad--${tarea.priority}`} />
        <span className="tarjeta__titulo">{tarea.title}</span>
        <span className="tarjeta__proyecto">{tarea.proyecto}</span>
        <span className="tarjeta__meta">
          {tarea.pomodoros}
          {tarea.estimate_pomodoros ? `/${tarea.estimate_pomodoros}` : ""}
          {tarea.due_date ? ` · ${tarea.due_date}` : ""}
        </span>
      </button>

      {abierta && (
        <div className="tarjeta__reverso">
          {/* Arrastrar hasta "En curso" hace lo mismo, pero para la tarea que
              toca ahora mismo pedir el ratón que cruce el tablero es peaje. */}
          {!activa && tarea.state !== "done" && (
            <button
              className="boton boton--principal boton--fino"
              onClick={() => alMover("in_progress")}
            >
              Servir el café
            </button>
          )}

          {/* La misma acción que arrastrar, con un clic. No es solo comodidad:
              el arrastre depende de que el webview se porte bien, y una tarjeta
              que solo se puede mover arrastrándola es una tarjeta que a veces
              no se puede mover. */}
          <div className="tarjeta__mover">
            {COLUMNAS.filter((c) => c.id !== tarea.state && c.id !== "in_progress").map((c) => (
              <button key={c.id} className="chip" onClick={() => alMover(c.id)}>
                {c.corto}
              </button>
            ))}
            {/* Apartar no es una columna: el bote no se enseña. Pero tenía que
                estar en algún sitio — hasta ahora la única forma de sacar algo
                de la vista era borrarlo, y eso pierde el historial. */}
            <button
              className="chip chip--tenue"
              title="ya no se va a hacer. Conserva el historial; no es borrarla"
              onClick={archivar}
            >
              apartar
            </button>
          </div>
          <Estimador tarea={tarea} recargar={recargar} alFallar={alFallar} />
          <Entrega tarea={tarea} recargar={recargar} alFallar={alFallar} />

          <div className="tarjeta__prioridades">
            {(["high", "medium", "low"] as Prioridad[]).map((p) => (
              <button
                key={p}
                className={`chip chip--${p}${tarea.priority === p ? " chip--puesta" : ""}`}
                disabled={congelada}
                title={
                  congelada
                    ? "la prioridad se congela con el primer pomodoro"
                    : `prioridad ${ETIQUETA_PRIORIDAD[p]}`
                }
                onClick={() => prioridad(p)}
              >
                {ETIQUETA_PRIORIDAD[p]}
              </button>
            ))}
          </div>
          {congelada && (
            <p className="tarjeta__nota">
              Ya se trabajó: la prioridad con la que se hizo es historia.
            </p>
          )}
          {/* Leer aqui y abrir en Obsidian son cosas distintas: lo primero es
              consultar sin soltar lo que estas haciendo, lo segundo es ir a
              cambiarla. Por eso "Leer" va primero y con mas peso. */}
          {tarea.vault_note && alLeer !== null && (
            <button className="boton boton--fino" onClick={alLeer} title={tarea.vault_note}>
              Leer la nota
            </button>
          )}
          {tarea.vault_note && (
            <button
              className="boton boton--fino boton--tenue"
              onClick={() => invoke("abrir_nota", { id: tarea.id }).catch((e) => alFallar(String(e)))}
              title={tarea.vault_note}
            >
              Abrir en Obsidian
            </button>
          )}
          <button className="boton boton--fino boton--tenue" onClick={borrar}>
            Borrar
          </button>
        </div>
      )}
    </li>
  );
}

/**
 * Estimar en un clic.
 *
 * Los números no son arbitrarios: saltan como la serie de Fibonacci porque a
 * partir de cierto tamaño la diferencia entre 5 y 6 pomodoros es ruido, y
 * ofrecer 6 invita a fingir una precisión que nadie tiene. El 8 está en rojo
 * porque Cirillo diría que a esa altura la tarea hay que partirla.
 *
 * Una tarea con entrega y sin estimar no pesa en el calendario, así que esto no
 * es un adorno: es lo que hace que la cuenta signifique algo.
 */
function Estimador({
  tarea,
  recargar,
  alFallar,
}: {
  tarea: Tarea;
  recargar: () => void;
  alFallar: (e: string) => void;
}) {
  async function poner(n: number | null) {
    try {
      await invoke("estimar", { id: tarea.id, pomodoros: n });
    } catch (e) {
      alFallar(String(e));
    }
    recargar();
  }

  return (
    <div className="estimador">
      <span className="estimador__rotulo">Pomodoros</span>
      <div className="estimador__botones">
        {[1, 2, 3, 5, 8].map((n) => (
          <button
            key={n}
            className={`chip${tarea.estimate_pomodoros === n ? " chip--puesta" : ""}${
              n >= 8 ? " chip--demasiado" : ""
            }`}
            title={n >= 8 ? "a partir de aquí, Cirillo diría que la partas" : `${n} pomodoros`}
            onClick={() => poner(n)}
          >
            {n}
          </button>
        ))}
        {tarea.estimate_pomodoros !== null && (
          <button className="chip chip--tenue" onClick={() => poner(null)} title="quitar la estimación">
            ×
          </button>
        )}
      </div>
    </div>
  );
}

/**
 * La fecha de entrega, en la tarjeta.
 *
 * Hasta ahora la ventana solo la aceptaba al CREAR la tarea: corregir una fecha
 * —o ponersela a algo que ya existia— obligaba a salir a `coffe task edit
 * --due`. Con las tareas que ya hay en la base eso significaba que el
 * calendario no se podia llenar desde aqui.
 *
 * **Es un campo de texto y no un `type="date"` a proposito.** El control nativo
 * de fecha de este WebKit se deja escribir —los segmentos cambian en pantalla—
 * pero no dispara `input` ni `change`, y su `.value` se queda vacio: React no
 * se entera nunca de lo que el usuario escribio. Se comprobo con sondas en los
 * cuatro eventos: `keyup` disparo seis veces y `change` cero. Es la misma clase
 * de trampa que la del arrastre —se ve funcionar y no llega el dato— y por eso
 * el formato se pide a mano, igual que en `coffe task edit --due`.
 *
 * Se guarda al salir del campo o con Enter, no en cada tecla: a medio escribir
 * "2026-0" no es una fecha, y mandar cada pulsacion serian cinco escrituras
 * invalidas por una buena.
 */
function Entrega({
  tarea,
  recargar,
  alFallar,
}: {
  tarea: Tarea;
  recargar: () => void;
  alFallar: (e: string) => void;
}) {
  const guardada = tarea.due_date ?? "";
  const [borrador, setBorrador] = useState(guardada);

  // Si la tarea cambia por debajo —un import del vault, otra ventana— el campo
  // sigue a la base y no se queda enseñando lo que ya no es.
  useEffect(() => setBorrador(guardada), [guardada]);

  const limpio = borrador.trim();
  const valida = limpio === "" || esFecha(limpio);

  async function guardar() {
    if (limpio === guardada || !valida) return;
    try {
      await invoke("fijar_entrega", { id: tarea.id, fecha: limpio || null });
    } catch (e) {
      alFallar(String(e));
    }
    recargar();
  }

  // Con fecha pero sin estimar, la tarea sale en el dia del calendario y NO
  // pesa en la resta. Se avisa aqui, que es donde se acaba de poner la fecha,
  // en vez de dejar que el veredicto lo cuente como un hueco mas tarde.
  const noPesa = guardada !== "" && tarea.estimate_pomodoros === null;

  return (
    <div className="estimador">
      <span className="estimador__rotulo">Entrega</span>
      <div className="entrega">
        <input
          className={`campo${valida ? "" : " campo--malo"}`}
          type="text"
          inputMode="numeric"
          maxLength={10}
          placeholder="AAAA-MM-DD"
          value={borrador}
          onChange={(e) => setBorrador(e.target.value)}
          onBlur={guardar}
          onKeyDown={(e) => {
            if (e.key === "Enter") guardar();
            if (e.key === "Escape") setBorrador(guardada);
          }}
          title="sin fecha no entra en el calendario"
        />
        {guardada !== "" && (
          <button
            className="chip chip--tenue"
            onClick={() => {
              setBorrador("");
              void invoke("fijar_entrega", { id: tarea.id, fecha: null })
                .catch((e) => alFallar(String(e)))
                .finally(recargar);
            }}
            title="quitar la fecha de entrega"
          >
            ×
          </button>
        )}
      </div>
      {!valida && <p className="tarjeta__nota">Tiene que ser AAAA-MM-DD.</p>}
      {noPesa && (
        <p className="tarjeta__nota">Con fecha pero sin estimar: sale en el día y no suma.</p>
      )}
    </div>
  );
}

/** Una fecha de calendario real. `2026-02-30` encaja con el patrón y no existe,
 *  asi que ademas se comprueba que sobreviva la ida y vuelta. */
function esFecha(s: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(s)) return false;
  const d = new Date(`${s}T00:00:00Z`);
  return !Number.isNaN(d.getTime()) && d.toISOString().slice(0, 10) === s;
}

// ------------------------------------------------------------- compositor

function Compositor({
  proyectos,
  proyectoSugerido,
  alCrear,
  alFallar,
}: {
  proyectos: Proyecto[];
  proyectoSugerido: number | null;
  alCrear: () => void;
  alFallar: (e: string) => void;
}) {
  const [titulo, setTitulo] = useState("");
  const [proyecto, setProyecto] = useState<number | null>(proyectoSugerido ?? proyectos[0]?.id ?? null);
  const [prioridad, setPrioridad] = useState<Prioridad>("medium");
  const [entrega, setEntrega] = useState("");
  const [estimacion, setEstimacion] = useState("");

  async function crear(e: React.FormEvent) {
    e.preventDefault();
    if (!titulo.trim() || proyecto === null) return;
    if (entrega !== "" && !esFecha(entrega)) {
      alFallar(`la fecha ${entrega} no es AAAA-MM-DD`);
      return;
    }
    try {
      await invoke("crear_tarea", {
        projectId: proyecto,
        title: titulo.trim(),
        priority: prioridad,
        dueDate: entrega || null,
        estimatePomodoros: estimacion ? Number(estimacion) : null,
      });
      alCrear();
    } catch (err) {
      alFallar(String(err));
    }
  }

  if (proyectos.length === 0) {
    return (
      <p className="aviso">
        No hay proyectos todavía. Créalos con <code>coffe project add</code>.
      </p>
    );
  }

  return (
    <form className="compositor" onSubmit={crear}>
      <input
        className="campo campo--ancho"
        placeholder="Qué hay que hacer"
        value={titulo}
        onChange={(e) => setTitulo(e.target.value)}
        autoFocus
      />
      <select
        className="selector"
        value={proyecto ?? ""}
        onChange={(e) => setProyecto(Number(e.target.value))}
      >
        {proyectos.map((p) => (
          <option key={p.id} value={p.id}>
            {"  ".repeat(p.nivel) + p.name}
          </option>
        ))}
      </select>
      <select
        className="selector"
        value={prioridad}
        onChange={(e) => setPrioridad(e.target.value as Prioridad)}
      >
        <option value="high">alta</option>
        <option value="medium">media</option>
        <option value="low">baja</option>
      </select>
      {/* Texto y no `type="date"`, por lo mismo que en la tarjeta: el control
          nativo de este WebKit no reporta su valor a React. Mientras estuvo
          aqui, toda tarea creada desde la ventana nacio sin fecha —y sin
          fecha no entra en el calendario— sin que nada lo dijera. */}
      <input
        className={`campo${entrega === "" || esFecha(entrega) ? "" : " campo--malo"}`}
        type="text"
        inputMode="numeric"
        maxLength={10}
        placeholder="AAAA-MM-DD"
        value={entrega}
        onChange={(e) => setEntrega(e.target.value.trim())}
        title="fecha de entrega"
      />
      <input
        className="campo campo--corto"
        type="number"
        min={1}
        max={20}
        placeholder="pom."
        value={estimacion}
        onChange={(e) => setEstimacion(e.target.value)}
        title="cuántos pomodoros crees que lleva"
      />
      <button className="boton boton--principal boton--fino" type="submit">
        Añadir
      </button>
    </form>
  );
}

// ---------------------------------------------------------------- el bote

function Bote({ cuantas, alVaciar }: { cuantas: number; alVaciar: () => void }) {
  const lleno = cuantas > 0;
  return (
    <button
      className={`bote${lleno ? " bote--lleno" : ""}`}
      disabled={!lleno}
      onClick={alVaciar}
      title={lleno ? `Vaciar: ${cuantas} taza(s) al archivo` : "El bote está vacío"}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        {/* La tapa se levanta sola cuando hay algo dentro. */}
        <path className="bote__tapa" d="M4 6 h16" />
        <path className="bote__tapa" d="M10 6 V4 h4 v2" />
        <path d="M6 6 l1 14 h10 l1 -14" />
        {lleno && <path className="bote__tazas" d="M9.5 11 v6 M12 10 v7 M14.5 11 v6" />}
      </svg>
      <span>{cuantas}</span>
    </button>
  );
}
