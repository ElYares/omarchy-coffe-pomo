import { useEffect, useMemo, useRef, useState } from "react";
import { Taza, type EstadoTaza } from "./Taza";
import { useReloj, useTareas, useTema } from "./reloj";
import { ETIQUETA_PRIORIDAD, reloj, type Snapshot, type Tarea } from "./tipos";

export default function App() {
  const { snap, caido, mandar } = useReloj();
  const tema = useTema();
  const [aviso, setAviso] = useState<string | null>(null);
  const anulado = useAnulado(snap);
  // Releer la lista en cada cambio de fase basta: las tareas no se mueven solas.
  const { tareas } = useTareas(snap?.phase ?? "sin");

  async function ordenar(o: Parameters<typeof mandar>[0]) {
    setAviso(await mandar(o));
  }

  if (caido && !snap) return <RelojApagado motivo={caido} />;
  if (!snap) return <div className="cargando">Sirviendo café…</div>;

  return (
    <div className="ventana">
      <header className="barra" data-tauri-drag-region>
        <span className="barra__nombre">coffe</span>
        <span className="barra__tema">{tema?.nombre}</span>
      </header>

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
          <Cola tareas={tareas} snap={snap} ordenar={ordenar} />
        </aside>
      </main>

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
  snap,
  ordenar,
}: {
  tareas: Tarea[];
  snap: Snapshot;
  ordenar: Ordenar;
}) {
  const vivas = useMemo(
    () => tareas.filter((t) => t.state !== "done" && t.state !== "archived"),
    [tareas],
  );
  const corriendo = snap.phase === "focus" || snap.phase === "overlearning";

  if (vivas.length === 0) {
    return (
      <p className="cola__vacia">
        No hay tareas. Créalas con <code>coffe task add</code> — el tablero llega
        en la fase siguiente.
      </p>
    );
  }

  return (
    <ul className="cola">
      {vivas.map((t) => {
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
