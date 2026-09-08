import { useEffect, useMemo, useRef, useState } from "react";
import { Calendario } from "./Calendario";
import { Proyectos } from "./Proyectos";
import { Reportes } from "./Reportes";
import { Tablero } from "./Tablero";
import { Taza, type EstadoTaza } from "./Taza";
import { FiltroProyecto, useConDescendientes, useFiltroRecordado } from "./filtro";
import { useReloj, useTareas, useTema } from "./reloj";
import { ETIQUETA_PRIORIDAD, reloj, type Snapshot, type Tarea } from "./tipos";

type Vista = "taza" | "tablero" | "calendario" | "proyectos" | "reportes";

/** La ventana reabre donde se dejó. Es una preferencia, no un dato: si el
 *  navegador no deja leerla, se arranca en la taza y ya. */
const VISTAS: Vista[] = ["taza", "tablero", "calendario", "proyectos", "reportes"];
const NOMBRE_VISTA: Record<Vista, string> = {
  taza: "Taza",
  tablero: "Tablero",
  calendario: "Calendario",
  proyectos: "Proyectos",
  reportes: "Reportes",
};

const vistaGuardada: Vista = (() => {
  try {
    const v = localStorage.getItem("coffe.vista");
    return VISTAS.includes(v as Vista) ? (v as Vista) : "taza";
  } catch {
    return "taza";
  }
})();

/**
 * `Ctrl+1`…`Ctrl+5` cambian de vista. La ventana no tiene menú ni barra de
 * título del compositor, así que sin esto el único camino son las pestañas, y
 * en mitad de un pomodoro soltar el teclado para buscar el ratón es justo la
 * clase de interrupción que esto mide.
 */
function useAtajosDeVista(setVista: (v: Vista) => void) {
  useEffect(() => {
    function alPulsar(e: KeyboardEvent) {
      if (!e.ctrlKey || e.altKey || e.metaKey) return;
      const n = Number(e.key);
      const destino = n >= 1 && n <= VISTAS.length ? VISTAS[n - 1] : null;
      if (!destino) return;
      e.preventDefault();
      setVista(destino);
      try {
        localStorage.setItem("coffe.vista", destino);
      } catch {
        // Sin almacenamiento, el atajo funciona igual; solo no se recuerda.
      }
    }
    window.addEventListener("keydown", alPulsar);
    return () => window.removeEventListener("keydown", alPulsar);
  }, [setVista]);
}

export default function App() {
  const { snap, caido, mandar } = useReloj();
  const tema = useTema();
  const [aviso, setAviso] = useState<string | null>(null);
  const anulado = useAnulado(snap);
  const [vista, setVista] = useState<Vista>(vistaGuardada);
  useAtajosDeVista(setVista);
  const { tareas, proyectos, recargar } = useTareas(snap?.phase ?? "sin");
  // El filtro de la taza es SUYO: el tablero es donde se organiza todo y ahí
  // querer verlo todo es lo normal, mientras que aquí se está eligiendo qué
  // hacer AHORA y el resto de proyectos solo estorba.
  const [proyecto, setProyecto] = useFiltroRecordado("coffe.taza.proyecto", proyectos);
  const dentroDelFiltro = useConDescendientes(proyecto, proyectos);

  async function ordenar(o: Parameters<typeof mandar>[0]) {
    setAviso(await mandar(o));
  }

  if (caido && !snap) return <RelojApagado motivo={caido} />;
  if (!snap) return <div className="cargando">Sirviendo café…</div>;

  function irA(v: Vista) {
    setVista(v);
    try {
      localStorage.setItem("coffe.vista", v);
    } catch {
      // Modo privado o almacenamiento lleno: las pestañas siguen funcionando,
      // solo no se recuerda cuál era.
    }
  }

  return (
    <div className="ventana">
      <header className="barra" data-tauri-drag-region>
        <span className="barra__nombre">coffe</span>
        <nav className="pestanas">
          {VISTAS.map((v) => (
            <button
              key={v}
              className={`pestana${vista === v ? " pestana--puesta" : ""}`}
              onClick={() => irA(v)}
            >
              {NOMBRE_VISTA[v]}
            </button>
          ))}
        </nav>
        <span className="barra__tema">{tema?.nombre}</span>
      </header>

      {vista === "taza" ? (
        <main className="principal">
          <section className="escena">
            <Taza
              estado={anulado ? "fria" : estadoDeTaza(snap)}
              nivel={anulado ? 0.62 : nivelDeTaza(snap)}
            />
            <Cuenta snap={snap} anulado={anulado} />
          </section>

          <aside className="panel">
            <Controles snap={snap} ordenar={ordenar} />
            {aviso && <p className="aviso">{aviso}</p>}
            <FiltroProyecto
              proyectos={proyectos}
              valor={proyecto}
              alCambiar={setProyecto}
              todos="Todo lo que hay"
            />
            <Cola tareas={tareas} dentro={dentroDelFiltro} snap={snap} ordenar={ordenar} />
          </aside>
        </main>
      ) : vista === "tablero" ? (
        <main className="principal principal--tablero">
          <Tablero tareas={tareas} proyectos={proyectos} snap={snap} recargar={recargar} />
        </main>
      ) : vista === "calendario" ? (
        <main className="principal principal--tablero">
          <Calendario
            tareas={tareas}
            // Pulsar una entrega lleva al tablero: el calendario dice CUÁNDO,
            // el tablero es donde se hace algo al respecto.
            alElegir={() => irA("tablero")}
          />
        </main>
      ) : vista === "proyectos" ? (
        <main className="principal principal--tablero">
          <Proyectos proyectos={proyectos} tareas={tareas} recargar={recargar} />
        </main>
      ) : (
        <main className="principal principal--tablero">
          <Reportes />
        </main>
      )}

      <footer className="pie">
        <Marcador snap={snap} />
      </footer>
    </div>
  );
}

/**
 * Un pomodoro anulado no deja rastro en el snapshot: el reloj simplemente pasa
 * a parado, igual que si hubiera terminado. La diferencia se ve en el SALTO, no
 * en el estado, así que se mira aquí: si veníamos de un foco y la tarea no
 * acabó en el refri, es que ese café se tiró.
 *
 * Dura unos segundos y se va sola. Es un duelo, no una pantalla.
 */
function useAnulado(snap: Snapshot | null): boolean {
  const [anulado, setAnulado] = useState(false);
  const previo = useRef<Snapshot | null>(null);

  useEffect(() => {
    if (!snap) return;
    const antes = previo.current;
    // En StrictMode el efecto corre dos veces por commit. Sin esta guarda, la
    // segunda pasada compara el snapshot consigo mismo, no ve ningún salto, y
    // el café frío no aparece nunca.
    if (antes === snap) return;
    previo.current = snap;
    if (!antes) return;

    const corria = antes.phase === "focus" || antes.phase === "overlearning";
    const paro = snap.phase === "idle";
    const fueAlRefri = snap.parked?.id === antes.task?.id && snap.parked?.id !== antes.parked?.id;

    if (corria && paro && !fueAlRefri) setAnulado(true);
    // Cualquier reloj nuevo borra el luto: si ya estás en otro pomodoro, la
    // taza de antes deja de importar.
    if (!paro) setAnulado(false);
  }, [snap]);

  // El temporizador va en su PROPIO efecto, atado a `anulado` y no a `snap`.
  // Junto al de arriba se cancelaba solo: durante un descanso llega un
  // snapshot por segundo, cada uno limpiaba el temporizador anterior y el
  // café frío se quedaba puesto para siempre.
  useEffect(() => {
    if (!anulado) return;
    const t = setTimeout(() => setAnulado(false), 3500);
    return () => clearTimeout(t);
  }, [anulado]);

  return anulado;
}

// -------------------------------------------------------------- la escena

/** Qué taza toca. Es la traducción de la fase del reloj al dibujo. */
function estadoDeTaza(s: Snapshot): EstadoTaza {
  switch (s.phase) {
    case "focus":
      return "vaciando";
    case "overlearning":
      return "repaso";
    case "short_break":
    case "long_break":
      return "llenando";
    case "idle":
      // Con el reloj parado, lo que hay en el refri manda: es la diferencia
      // entre "no estoy haciendo nada" y "dejé algo a medias".
      return s.parked ? "refri" : "vacia";
  }
}

/**
 * Cuánto café hay en la taza. En foco baja con el tiempo y en descanso sube:
 * es el mismo `progress`, leído al derecho o al revés.
 */
function nivelDeTaza(s: Snapshot): number {
  switch (s.phase) {
    case "focus":
    case "overlearning":
      return 1 - s.progress;
    case "short_break":
    case "long_break":
      return s.progress;
    case "idle":
      return s.parked ? 0.55 : 0;
  }
}

function Cuenta({ snap, anulado }: { snap: Snapshot; anulado: boolean }) {
  const texto = anulado ? "Pomodoro anulado — ese café se tira" : titulo(snap);
  return (
    <div className="cuenta">
      <p className="cuenta__reloj">{reloj(snap)}</p>
      <p className="cuenta__fase">{texto}</p>
      {snap.task && (
        <p className="cuenta__tarea">
          {snap.task.title}
          <span className="cuenta__proyecto">{snap.task.project}</span>
        </p>
      )}
      {!snap.task && snap.parked && (
        <p className="cuenta__tarea cuenta__tarea--refri">
          {snap.parked.title}
          <span className="cuenta__proyecto">en el refri · {snap.parked.project}</span>
        </p>
      )}
    </div>
  );
}

function titulo(s: Snapshot): string {
  switch (s.phase) {
    case "focus":
      return "Pomodoro";
    case "overlearning":
      return "Repaso — la tarea ya está hecha";
    case "short_break":
      return "Descanso";
    case "long_break":
      return "Descanso largo";
    case "idle":
      return s.parked ? "En el refri" : "Sin pomodoro";
  }
}

// ------------------------------------------------------------- el panel

type Ordenar = (o: Parameters<ReturnType<typeof useReloj>["mandar"]>[0]) => void;

function Controles({ snap, ordenar }: { snap: Snapshot; ordenar: Ordenar }) {
  const corriendo = snap.phase === "focus" || snap.phase === "overlearning";
  const enDescanso = snap.phase === "short_break" || snap.phase === "long_break";

  return (
    <div className="controles">
      {corriendo && (
        <>
          <button className="boton boton--principal" onClick={() => ordenar({ tipo: "done" })}>
            Terminada
          </button>
          <button className="boton" onClick={() => ordenar({ tipo: "pause" })}>
            Al refri
          </button>
          <button className="boton" onClick={() => ordenar({ tipo: "interrupt", externa: true })}>
            Me interrumpieron
          </button>
          <button className="boton boton--tenue" onClick={() => ordenar({ tipo: "void" })}>
            Tirar el pomodoro
          </button>
        </>
      )}

      {enDescanso && (
        <>
          <p className="controles__nota">
            El descanso es parte del método. Levántate.
          </p>
          {!snap.strict && (
            <button className="boton" onClick={() => ordenar({ tipo: "skip_break" })}>
              Cortar el descanso
            </button>
          )}
        </>
      )}

      {snap.phase === "idle" && snap.parked && (
        <button
          className="boton boton--principal"
          onClick={() => ordenar({ tipo: "start", task_id: snap.parked!.id })}
        >
          Sacar del refri
        </button>
      )}

      {snap.phase === "idle" && !snap.parked && (
        <p className="controles__nota">Elige una tarea y sirve el café.</p>
      )}
    </div>
  );
}

function Cola({
  tareas,
  dentro,
  snap,
  ordenar,
}: {
  tareas: Tarea[];
  /** Los proyectos que deja pasar el filtro, o `null` si no hay filtro. */
  dentro: Set<number> | null;
  snap: Snapshot;
  ordenar: Ordenar;
}) {
  const vivas = useMemo(
    () => tareas.filter((t) => t.state !== "done" && t.state !== "archived"),
    [tareas],
  );
  const visibles = useMemo(
    () => (dentro === null ? vivas : vivas.filter((t) => dentro.has(t.project_id))),
    [vivas, dentro],
  );
  const corriendo = snap.phase === "focus" || snap.phase === "overlearning";

  // Lo que el filtro deja fuera se dice. Una lista corta que no avisa de que
  // está recortada se lee como si eso fuera todo lo que hay pendiente, y esa
  // es justo la mentira que esta app existe para no contar.
  const ocultas = vivas.length - visibles.length;
  const laActual = snap.task;
  const actualFuera = laActual !== null && !visibles.some((t) => t.id === laActual.id);

  if (vivas.length === 0) {
    return (
      <p className="cola__vacia">
        No hay tareas. Créalas en el tablero o con <code>coffe task add</code>.
      </p>
    );
  }

  if (visibles.length === 0) {
    return (
      <p className="cola__vacia">
        Nada vivo en este proyecto. Quedan {ocultas} en los demás: cambia el
        filtro para verlas.
      </p>
    );
  }

  return (
    <>
    <ul className="cola">
      {visibles.map((t) => {
        const actual = snap.task?.id === t.id;
        return (
          <li key={t.id} className={`cola__item${actual ? " cola__item--actual" : ""}`}>
            <button
              className="cola__boton"
              disabled={actual}
              // Con un pomodoro vivo, elegir otra tarea es un cambio en
              // caliente: anula el actual y lo apunta. No es lo mismo que
              // arrancar en frío, y el daemon tiene que saberlo.
              onClick={() =>
                ordenar(corriendo ? { tipo: "switch", task_id: t.id } : { tipo: "start", task_id: t.id })
              }
            >
              <span className={`prioridad prioridad--${t.priority}`} />
              <span className="cola__titulo">{t.title}</span>
              <span className="cola__meta">
                {t.pomodoros}
                {t.estimate_pomodoros ? `/${t.estimate_pomodoros}` : ""}
                {t.due_date ? ` · ${t.due_date}` : ""}
              </span>
              <span className="cola__proyecto">{t.proyecto}</span>
            </button>
          </li>
        );
      })}
    </ul>
    {ocultas > 0 && (
      <p className="cola__ocultas">
        {ocultas === 1 ? "1 tarea viva" : `${ocultas} tareas vivas`} fuera del
        filtro{actualFuera && ", incluida la que está corriendo"}
      </p>
    )}
    </>
  );
}

function Marcador({ snap }: { snap: Snapshot }) {
  const faltan = Math.max(0, snap.long_break_every - snap.completed_since_long_break);
  return (
    <>
      <span>
        <strong>{snap.pomodoros_hoy}</strong> hoy
      </span>
      <span className="ciclo" title={`${faltan} para el descanso largo`}>
        {Array.from({ length: snap.long_break_every }, (_, i) => (
          <i key={i} className={i < snap.completed_since_long_break ? "grano grano--lleno" : "grano"} />
        ))}
      </span>
      {snap.en_papelera > 0 && <span>{snap.en_papelera} en el bote</span>}
      {!snap.strict && <span className="pie__flexible">modo flexible</span>}
      {snap.task && (
        <span className="pie__prioridad">prioridad {ETIQUETA_PRIORIDAD[snap.task.priority]}</span>
      )}
    </>
  );
}

function RelojApagado({ motivo }: { motivo: string }) {
  return (
    <div className="apagado">
      <Taza estado="fria" nivel={0.3} />
      <h1>El reloj no está corriendo</h1>
      <p>{motivo}</p>
      <code>systemctl --user start coffe</code>
    </div>
  );
}
