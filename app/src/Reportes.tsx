// En qué se te fue el tiempo.
//
// Es lo mismo que `coffe report`, con la misma regla: **ningún número aparece
// solo**. Cuarenta pomodoros no dicen nada sin saber en cuántos días cayeron, y
// un porcentaje sobre tres casos no es un porcentaje, es una anécdota con
// decimales.
//
// Los números salen enteros del backend —incluidos la tasa de anulación y el
// factor de estimación—, así que aquí no se calcula nada: dos copias de la
// misma aritmética acaban discrepando y el usuario ve una cifra distinta según
// mire la terminal o la ventana.

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { duracion, type DiaActividad, type Reporte } from "./tipos";

/** Por debajo de esto, un porcentaje es una anécdota con decimales. */
const MINIMO_PARA_HABLAR_DE_TENDENCIA = 5;

const PERIODOS: { dias: number; nombre: string }[] = [
  { dias: 7, nombre: "7 días" },
  { dias: 30, nombre: "30 días" },
  { dias: 90, nombre: "90 días" },
  { dias: 365, nombre: "un año" },
];

export function Reportes() {
  const [dias, setDias] = useState(30);
  const [rep, setRep] = useState<Reporte | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [nota, setNota] = useState<string | null>(null);

  useEffect(() => {
    invoke<Reporte>("reporte", { dias })
      .then((r) => {
        setRep(r);
        setError(null);
      })
      .catch((e) => setError(String(e)));
  }, [dias]);

  const exportar = useCallback(async (que: "pomodoros" | "tareas", dias: number) => {
    try {
      const destino = await save({
        title: `Exportar ${que}`,
        defaultPath: `coffe-${que}.csv`,
        filters: [{ name: "CSV", extensions: ["csv"] }],
      });
      // `null` es que cerró el selector. No es un fallo y no merece un aviso.
      if (!destino) return;
      const filas = await invoke<number>("exportar_csv", { que, dias, destino });
      // Cuántas filas, siempre. Un archivo vacío escrito sin una queja es la
      // clase de éxito que se descubre tarde, al abrirlo.
      setNota(
        filas === 0
          ? `${destino}: se escribió, pero con cero filas.`
          : `${filas} fila(s) en ${destino}`,
      );
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  if (error && !rep) return <p className="aviso">{error}</p>;
  if (!rep) return <p className="reporte__vacio">Contando…</p>;

  const r = rep.resumen;
  const vacio = r.pomodoros_completados === 0 && r.pomodoros_anulados === 0;

  return (
    <div className="reporte">
      <div className="reporte__barra">
        <select
          className="selector"
          value={dias}
          onChange={(e) => setDias(Number(e.target.value))}
        >
          {PERIODOS.map((p) => (
            <option key={p.dias} value={p.dias}>
              Últimos {p.nombre}
            </option>
          ))}
        </select>
        <span className="reporte__hueco" />
        <button className="boton boton--fino" onClick={() => exportar("pomodoros", dias)}>
          CSV de pomodoros
        </button>
        <button className="boton boton--fino" onClick={() => exportar("tareas", dias)}>
          CSV de tareas
        </button>
      </div>

      {error && (
        <p className="aviso" onClick={() => setError(null)}>
          {error}
        </p>
      )}
      {nota && (
        <p className="aviso aviso--bueno" onClick={() => setNota(null)}>
          {nota}
        </p>
      )}

      {vacio ? (
        <p className="reporte__vacio">
          Ni un pomodoro en {rep.dias} días. Nada que contar — y eso también es
          un dato.
        </p>
      ) : (
        <div className="reporte__cuerpo">
          <Cifras rep={rep} />
          <Mapa rep={rep} />
          <Reparto rep={rep} />
          <Estimaciones rep={rep} />
        </div>
      )}
    </div>
  );
}

// -------------------------------------------------------------- las cifras

function Cifras({ rep }: { rep: Reporte }) {
  const r = rep.resumen;
  const interrupciones = r.interrupciones_internas + r.interrupciones_externas;

  return (
    <section className="reporte__bloque">
      <h2>Lo que sonó</h2>
      <dl className="cifras">
        <Cifra
          rotulo="pomodoros"
          valor={String(r.pomodoros_completados)}
          // El total sin los días es engañoso: cuarenta en cuatro días y
          // cuarenta en veinte describen dos vidas distintas.
          pie={`en ${r.dias_con_trabajo} día(s) con trabajo`}
        />
        <Cifra
          rotulo="efectivo"
          valor={duracion(r.segundos_efectivos)}
          pie={
            r.dias_con_trabajo > 0
              ? `${(r.pomodoros_completados / r.dias_con_trabajo).toFixed(1)} al día de media`
              : undefined
          }
        />
        <Cifra
          rotulo="tirados"
          valor={String(r.pomodoros_anulados)}
          pie={
            r.pomodoros_anulados > 0
              ? `${Math.round(rep.tasa_anulacion * 100)}% de los que empezaste`
              : "ninguno se fue al bote"
          }
        />
        <Cifra rotulo="terminadas" valor={String(r.tareas_terminadas)} pie="tarea(s)" />
        {interrupciones > 0 && (
          <Cifra
            rotulo="interrupciones"
            valor={String(interrupciones)}
            pie={`${r.interrupciones_internas} internas · ${r.interrupciones_externas} externas`}
          />
        )}
        {r.segundos_con_claude > 0 && (
          <Cifra
            rotulo="con Claude"
            valor={duracion(r.segundos_con_claude)}
            pie={
              r.segundos_efectivos > 0
                ? `${Math.round((r.segundos_con_claude / r.segundos_efectivos) * 100)}% del efectivo`
                : undefined
            }
          />
        )}
      </dl>
    </section>
  );
}

function Cifra({ rotulo, valor, pie }: { rotulo: string; valor: string; pie?: string }) {
  return (
    <div className="cifra">
      <dt>{rotulo}</dt>
      <dd>{valor}</dd>
      {pie && <p className="cifra__pie">{pie}</p>}
    </div>
  );
}

// ---------------------------------------------------------------- el mapa

const DIAS_SEMANA = ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"];
const MESES = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic"];

/** `AAAA-MM-DD` a fecha local. `new Date("AAAA-MM-DD")` la leería como UTC y
 *  en −06:00 caería en el día anterior: justo el error que el mapa corrige. */
function fechaLocal(iso: string): Date {
  const [a, m, d] = iso.split("-").map(Number);
  return new Date(a, m - 1, d);
}

/** Qué pinta el mapa. Aquí no se cuenta nada: los días, sus cifras y el
 *  escalón vienen del núcleo. Solo se acomodan en semanas. */
function Mapa({ rep }: { rep: Reporte }) {
  if (rep.mapa.length === 0) return null;

  // Columnas de lunes a domingo. La primera semana se rellena por delante para
  // que cada fila sea siempre el mismo día de la semana.
  const hueco = (fechaLocal(rep.mapa[0].fecha).getDay() + 6) % 7;
  const casillas: (DiaActividad | null)[] = [...Array(hueco).fill(null), ...rep.mapa];
  const semanas: (DiaActividad | null)[][] = [];
  for (let i = 0; i < casillas.length; i += 7) semanas.push(casillas.slice(i, i + 7));

  const tirados = rep.mapa.filter((d) => d.completados === 0 && d.anulados > 0).length;

  return (
    <section className="reporte__bloque">
      <h2>Día a día</h2>
      <div className="mapa">
        <div className="mapa__semana mapa__semana--rotulos">
          <span className="mapa__mes" />
          {DIAS_SEMANA.map((d, i) => (
            <span key={d} className="mapa__rotulo">
              {i % 2 === 0 ? d : ""}
            </span>
          ))}
        </div>
        {semanas.map((semana, i) => (
          <div key={i} className="mapa__semana">
            <span className="mapa__mes">{rotuloDeMes(semana)}</span>
            {semana.map((dia, j) =>
              dia === null ? (
                <span key={j} className="mapa__dia mapa__dia--fuera" />
              ) : (
                <span
                  key={j}
                  className={
                    `mapa__dia mapa__dia--${dia.escalon}` +
                    (dia.completados === 0 && dia.anulados > 0 ? " mapa__dia--tirado" : "")
                  }
                  title={descripcion(dia)}
                />
              ),
            )}
          </div>
        ))}
      </div>
      <Leyenda cortes={rep.cortes} />
      {tirados > 0 && (
        <p className="reporte__pista">
          {tirados} día(s) con pomodoros empezados y ninguno que sonara: van con
          borde, porque vacíos no estuvieron.
        </p>
      )}
    </section>
  );
}

/** El mes, solo en la semana donde empieza. Si no, 53 columnas con su rótulo
 *  se leerían peor que ninguna. */
function rotuloDeMes(semana: (DiaActividad | null)[]): string {
  const primero = semana.find((d) => d !== null && fechaLocal(d.fecha).getDate() === 1);
  return primero ? MESES[fechaLocal(primero.fecha).getMonth()] : "";
}

function descripcion(d: DiaActividad): string {
  const f = fechaLocal(d.fecha);
  const fecha = `${DIAS_SEMANA[(f.getDay() + 6) % 7]} ${f.getDate()} ${MESES[f.getMonth()]}`;
  const tirados = d.anulados > 0 ? ` · ${d.anulados} tirado(s)` : "";
  return `${fecha}: ${d.completados} sonaron${tirados}`;
}

/** Los cortes con sus números. Un color sin su rango obliga a adivinar si
 *  «oscuro» son cinco pomodoros o quince. */
function Leyenda({ cortes }: { cortes: number[] }) {
  const rangos = cortes.map((c, i) => {
    const siguiente = cortes[i + 1];
    if (siguiente === undefined) return `${c}+`;
    return siguiente - 1 === c ? String(c) : `${c}–${siguiente - 1}`;
  });

  return (
    <div className="mapa__leyenda">
      <span className="mapa__muestra">
        <i className="mapa__dia mapa__dia--0" /> 0
      </span>
      {rangos.map((r, i) => (
        <span key={r} className="mapa__muestra">
          <i className={`mapa__dia mapa__dia--${i + 1}`} /> {r}
        </span>
      ))}
      <span className="mapa__muestra">
        <i className="mapa__dia mapa__dia--0 mapa__dia--tirado" /> solo tirados
      </span>
    </div>
  );
}

// ------------------------------------------------------------- el reparto

function Reparto({ rep }: { rep: Reporte }) {
  if (rep.cargas.length === 0) return null;
  const tope = Math.max(1, ...rep.cargas.map((c) => c.segundos_efectivos));

  return (
    <section className="reporte__bloque">
      <h2>Dónde se fue</h2>
      <p className="reporte__pista">
        Un proyecto suma lo suyo y lo de sus hijos: si no, la jerarquía sería de
        adorno.
      </p>
      <ul className="reparto">
        {rep.cargas.map((c) => (
          <li key={c.project_id} className="reparto__fila">
            <span className="reparto__nombre" title={c.ruta}>
              {c.ruta}
            </span>
            <span className="reparto__barra">
              <i style={{ width: `${Math.max(2, (c.segundos_efectivos / tope) * 100)}%` }} />
            </span>
            <span className="reparto__cifra">
              {c.pomodoros} pom · {duracion(c.segundos_efectivos)}
              {c.anulados > 0 && <em> · {c.anulados} al bote</em>}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}

// --------------------------------------------------------- las estimaciones

function Estimaciones({ rep }: { rep: Reporte }) {
  const p = rep.precision;

  return (
    <section className="reporte__bloque">
      <h2>Tus estimaciones</h2>

      {p.tareas === 0 ? (
        <>
          <p className="reporte__pista">
            Todavía no hay ninguna tarea terminada <strong>con</strong>{" "}
            estimación. Sin eso no hay con qué comparar: estima antes de empezar.
          </p>
          {p.sin_estimar > 0 && (
            <p className="reporte__pista">
              Terminaste {p.sin_estimar} sin estimar. Están medidas, pero no hay
              contra qué contrastarlas.
            </p>
          )}
        </>
      ) : (
        <>
          <p className="estimaciones__frase">
            En {p.tareas} tarea(s) dijiste <strong>{p.estimados}</strong>{" "}
            pomodoros y gastaste <strong>{p.reales}</strong>.
          </p>
          <ul className="estimaciones__desglose">
            <li>
              <span>{p.subestimadas}</span> te quedaste corto
            </li>
            <li>
              <span>{p.clavadas}</span> clavadas
            </li>
            <li>
              <span>{p.sobreestimadas}</span> sobró
            </li>
          </ul>
          <Veredicto factor={rep.factor} tareas={p.tareas} />
          {/* Lo que la cuenta NO mira. Un factor sacado de dos tareas mientras
              otras treinta se terminaron a ojo se lee como tu forma de estimar,
              y es la de dos tareas. */}
          {p.sin_estimar > 0 && (
            <p className="reporte__pista">
              {p.sin_estimar} terminada(s) sin estimar quedan fuera de esta
              cuenta.
            </p>
          )}
        </>
      )}
    </section>
  );
}

function Veredicto({ factor, tareas }: { factor: number | null; tareas: number }) {
  if (factor === null) return null;

  // Se enseña igual, pero diciendo que no es una tendencia: callarlo invitaría
  // a creérselo.
  if (tareas < MINIMO_PARA_HABLAR_DE_TENDENCIA) {
    return (
      <p className="veredicto veredicto--parcial">
        De momento ×{factor.toFixed(1)}, pero con {tareas} tarea(s) eso no es una
        tendencia.
      </p>
    );
  }

  if (factor > 1.15) {
    return (
      <p className="veredicto veredicto--no-cabe">
        Multiplica por {factor.toFixed(1)} lo que estimes: se te queda corto.
      </p>
    );
  }
  if (factor < 0.85) {
    return (
      <p className="veredicto veredicto--parcial">
        Estimas de más: gastas ×{factor.toFixed(1)} de lo que dices.
      </p>
    );
  }
  return <p className="veredicto veredicto--cabe">Van bien: ×{factor.toFixed(1)}.</p>;
}
