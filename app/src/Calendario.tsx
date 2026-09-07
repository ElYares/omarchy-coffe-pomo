// El calendario.
//
// No está aquí para enseñar en qué casilla cae cada entrega —eso lo hace
// cualquier calendario— sino para responder si **cabe**. La cuenta la hace
// `coffe_core::agenda`, que es pura y está probada; esto solo la pinta.

import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ETIQUETA_PRIORIDAD,
  hoyLocal,
  pendientes,
  type DiaAgenda,
  type Plan,
  type Tarea,
} from "./tipos";

/** Cuántos días pide el plan. Dos meses cubren la vista de mes y la siguiente
 *  sin tener que volver a preguntar al cambiar de página. */
const HORIZONTE = 70;

const DIAS_SEMANA = ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"];
const MESES = [
  "enero", "febrero", "marzo", "abril", "mayo", "junio",
  "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre",
];

interface Props {
  tareas: Tarea[];
  alElegir: (id: number) => void;
}

export function Calendario({ tareas, alElegir }: Props) {
  const [plan, setPlan] = useState<Plan>({ dias: [], sin_estimar: 0 });
  const [vista, setVista] = useState<"mes" | "agenda">("mes");
  const hoy = useMemo(hoyLocal, []);
  const [mesVisible, setMesVisible] = useState(() => hoy.slice(0, 7));

  useEffect(() => {
    invoke<Plan>("agenda", { dias: HORIZONTE })
      .then(setPlan)
      .catch(() => setPlan({ dias: [], sin_estimar: 0 }));
  }, [tareas]);

  // Lo que vence cada día, indexado por fecha.
  const porDia = useMemo(() => {
    const m = new Map<string, Tarea[]>();
    for (const t of tareas) {
      if (!t.due_date || t.state === "done" || t.state === "archived") continue;
      const lista = m.get(t.due_date) ?? [];
      lista.push(t);
      m.set(t.due_date, lista);
    }
    return m;
  }, [tareas]);

  const planPorDia = useMemo(
    () => new Map(plan.dias.map((d) => [d.fecha, d])),
    [plan],
  );

  const apretado = plan.dias.find((d) => d.imposible) ?? null;

  return (
    <div className="calendario">
      <header className="calendario__barra">
        <Veredicto dia={apretado} hoy={hoy} sinEstimar={plan.sin_estimar} />
        <div className="calendario__vistas">
          {(["mes", "agenda"] as const).map((v) => (
            <button
              key={v}
              className={`pestana${vista === v ? " pestana--puesta" : ""}`}
              onClick={() => setVista(v)}
            >
              {v === "mes" ? "Mes" : "Agenda"}
            </button>
          ))}
        </div>
      </header>

      {vista === "mes" ? (
        <Mes
          mes={mesVisible}
          hoy={hoy}
          porDia={porDia}
          planPorDia={planPorDia}
          alCambiarMes={setMesVisible}
          alElegir={alElegir}
        />
      ) : (
        <Agenda plan={plan.dias} hoy={hoy} porDia={porDia} alElegir={alElegir} />
      )}
    </div>
  );
}

/**
 * La respuesta corta a "¿voy bien?". Es lo primero que hay que leer, así que no
 * puede sonar más seguro de lo que es: una tarea con fecha y sin estimación no
 * pesa en ningún día, y decir "todo cabe" sin mencionarlas sería afirmar algo
 * que la cuenta no ha mirado.
 */
function Veredicto({
  dia,
  hoy,
  sinEstimar,
}: {
  dia: DiaAgenda | null;
  hoy: string;
  sinEstimar: number;
}) {
  const pendiente =
    sinEstimar > 0 ? (
      <>
        {" "}
        <span className="veredicto__hueco">
          {sinEstimar} con entrega y sin estimar no{" "}
          {sinEstimar === 1 ? "entra" : "entran"} en la cuenta.
        </span>
      </>
    ) : null;

  if (!dia) {
    return (
      <p className={`veredicto ${sinEstimar > 0 ? "veredicto--parcial" : "veredicto--cabe"}`}>
        {sinEstimar > 0 ? "Lo que se puede contar cabe." : "Todo lo comprometido cabe."}
        {pendiente}
      </p>
    );
  }

  const exceso = dia.deuda_acumulada - dia.capacidad_acumulada;
  return (
    <p className="veredicto veredicto--no-cabe">
      <strong>{comoSeLlama(dia.fecha, hoy)}</strong> no cabe: sobran {exceso} pomodoro
      {exceso === 1 ? "" : "s"}. Mueve trabajo o mueve la fecha.
      {pendiente}
    </p>
  );
}

// ------------------------------------------------------------- vista de mes

function Mes({
  mes,
  hoy,
  porDia,
  planPorDia,
  alCambiarMes,
  alElegir,
}: {
  mes: string;
  hoy: string;
  porDia: Map<string, Tarea[]>;
  planPorDia: Map<string, DiaAgenda>;
  alCambiarMes: (m: string) => void;
  alElegir: (id: number) => void;
}) {
  const celdas = useMemo(() => rejillaDelMes(mes), [mes]);
  const [anio, numMes] = mes.split("-").map(Number);

  return (
    <>
      <div className="mes__cabeza">
        <button className="boton boton--fino" onClick={() => alCambiarMes(sumarMeses(mes, -1))}>
          ←
        </button>
        <h2>
          {MESES[numMes - 1]} {anio}
        </h2>
        <button className="boton boton--fino" onClick={() => alCambiarMes(sumarMeses(mes, 1))}>
          →
        </button>
      </div>

      <div className="mes">
        {DIAS_SEMANA.map((d) => (
          <span key={d} className="mes__dia-semana">
            {d}
          </span>
        ))}

        {celdas.map((fecha) => {
          if (fecha === null) return <span key={Math.random()} className="mes__hueco" />;

          const suyas = porDia.get(fecha) ?? [];
          const dia = planPorDia.get(fecha);
          const pom = suyas.reduce((n, t) => n + (pendientes(t) ?? 0), 0);
          const clases = [
            "mes__celda",
            fecha === hoy ? "mes__celda--hoy" : "",
            fecha < hoy && suyas.length > 0 ? "mes__celda--vencida" : "",
            dia?.imposible ? "mes__celda--imposible" : "",
            dia && !dia.laborable ? "mes__celda--descanso" : "",
          ]
            .filter(Boolean)
            .join(" ");

          return (
            <div key={fecha} className={clases}>
              <span className="mes__numero">{Number(fecha.slice(8))}</span>
              {suyas.length > 0 && (
                <ul className="mes__tareas">
                  {suyas.slice(0, 3).map((t) => (
                    <li key={t.id}>
                      <button
                        className={`mes__tarea prioridad-borde--${t.priority}`}
                        onClick={() => alElegir(t.id)}
                        title={`${t.title} — ${t.proyecto}`}
                      >
                        {t.title}
                      </button>
                    </li>
                  ))}
                  {suyas.length > 3 && (
                    <li className="mes__resto">y {suyas.length - 3} más</li>
                  )}
                </ul>
              )}
              {pom > 0 && <span className="mes__carga">{pom} pom.</span>}
            </div>
          );
        })}
      </div>
    </>
  );
}

// ---------------------------------------------------------- vista de agenda

function Agenda({
  plan,
  hoy,
  porDia,
  alElegir,
}: {
  plan: DiaAgenda[];
  hoy: string;
  porDia: Map<string, Tarea[]>;
  alElegir: (id: number) => void;
}) {
  // Lo vencido va arriba del todo: es lo que ya debería estar hecho.
  const vencidas = useMemo(
    () =>
      [...porDia.entries()]
        .filter(([f]) => f < hoy)
        .flatMap(([, ts]) => ts)
        .sort((a, b) => (a.due_date ?? "").localeCompare(b.due_date ?? "")),
    [porDia, hoy],
  );

  const conAlgo = plan.filter((d) => (porDia.get(d.fecha) ?? []).length > 0);

  if (vencidas.length === 0 && conAlgo.length === 0) {
    return <p className="calendario__vacio">No hay nada con fecha de entrega.</p>;
  }

  return (
    <div className="agenda">
      {vencidas.length > 0 && (
        <section className="agenda__dia agenda__dia--vencida">
          <header className="agenda__cabeza">
            <h3>Vencidas</h3>
            <span>{vencidas.length}</span>
          </header>
          {vencidas.map((t) => (
            <FilaTarea key={t.id} tarea={t} alElegir={alElegir} conFecha />
          ))}
        </section>
      )}

      {conAlgo.map((d) => {
        const suyas = porDia.get(d.fecha) ?? [];
        return (
          <section key={d.fecha} className={`agenda__dia${d.imposible ? " agenda__dia--apretado" : ""}`}>
            <header className="agenda__cabeza">
              <h3>{comoSeLlama(d.fecha, hoy)}</h3>
              <span>
                {d.deuda_acumulada} debidos · {d.capacidad_acumulada} caben
              </span>
            </header>
            {suyas.map((t) => (
              <FilaTarea key={t.id} tarea={t} alElegir={alElegir} />
            ))}
          </section>
        );
      })}
    </div>
  );
}

function FilaTarea({
  tarea,
  alElegir,
  conFecha,
}: {
  tarea: Tarea;
  alElegir: (id: number) => void;
  conFecha?: boolean;
}) {
  const faltan = pendientes(tarea);
  return (
    <button className="agenda__tarea" onClick={() => alElegir(tarea.id)}>
      <span className={`prioridad prioridad--${tarea.priority}`} />
      <span className="agenda__titulo">{tarea.title}</span>
      <span className="agenda__meta">
        {faltan === null ? "sin estimar" : `${faltan} pom.`}
        {conFecha && tarea.due_date ? ` · ${tarea.due_date}` : ""}
      </span>
      <span className="agenda__proyecto">
        {tarea.proyecto} · {ETIQUETA_PRIORIDAD[tarea.priority]}
      </span>
    </button>
  );
}

// ------------------------------------------------------------------ fechas

/** `hoy`, `mañana` o `mié 9 de septiembre`. Un nombre se lee más rápido que
 *  una fecha, y para lo que está cerca es lo único que importa. */
function comoSeLlama(fecha: string, hoy: string): string {
  if (fecha === hoy) return "hoy";
  if (fecha === sumarDias(hoy, 1)) return "mañana";

  const [, m, d] = fecha.split("-").map(Number);
  const dia = new Date(`${fecha}T12:00:00`);
  // getDay() da 0 para domingo; nuestra semana empieza en lunes.
  const nombre = DIAS_SEMANA[(dia.getDay() + 6) % 7];
  return `${nombre} ${d} de ${MESES[m - 1]}`;
}

function sumarDias(fecha: string, n: number): string {
  const d = new Date(`${fecha}T12:00:00`);
  d.setDate(d.getDate() + n);
  return d.toISOString().slice(0, 10);
}

function sumarMeses(mes: string, n: number): string {
  const [a, m] = mes.split("-").map(Number);
  const d = new Date(a, m - 1 + n, 1);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
}

/** Las celdas del mes, con huecos al principio para que el día 1 caiga en su
 *  columna. La semana empieza en lunes. */
function rejillaDelMes(mes: string): (string | null)[] {
  const [anio, m] = mes.split("-").map(Number);
  const primero = new Date(anio, m - 1, 1);
  const cuantos = new Date(anio, m, 0).getDate();
  const huecos = (primero.getDay() + 6) % 7;

  const celdas: (string | null)[] = Array(huecos).fill(null);
  for (let d = 1; d <= cuantos; d++) {
    celdas.push(`${mes}-${String(d).padStart(2, "0")}`);
  }
  return celdas;
}
