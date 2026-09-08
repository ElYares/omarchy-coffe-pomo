// Espejo de lo que manda el backend. Si esto y `coffe-ipc` se separan, la
// ventana enseña basura sin quejarse, así que los nombres van copiados tal cual
// del Rust —incluidos los que están en español.

export type Fase =
  | "idle"
  | "focus"
  | "overlearning"
  | "short_break"
  | "long_break";

export type Prioridad = "low" | "medium" | "high";

export type EstadoTarea =
  | "pending"
  | "in_progress"
  | "paused"
  | "done"
  | "archived";

export interface TareaBreve {
  id: number;
  title: string;
  project: string;
  priority: Prioridad;
  estimate_pomodoros: number | null;
  done_pomodoros: number;
}

export interface Snapshot {
  phase: Fase;
  remaining_secs: number;
  /** De 0 a 1: cuánto se lleva consumido de la fase actual. */
  progress: number;
  strict: boolean;
  completed_since_long_break: number;
  long_break_every: number;
  task: TareaBreve | null;
  /** La última tarea que se mandó al refri, si sigue ahí. */
  parked: TareaBreve | null;
  pomodoros_hoy: number;
  en_papelera: number;
  now: string;
}

export interface Tarea {
  id: number;
  project_id: number;
  title: string;
  notes: string | null;
  priority: Prioridad;
  state: EstadoTarea;
  estimate_pomodoros: number | null;
  due_date: string | null;
  position: number;
  /** Cuándo se trabajó por primera vez. Es la señal de que la prioridad ya no
   *  se toca — la misma que usa el backend, para que no puedan discrepar. */
  first_started_at: string | null;
  completed_at: string | null;
  /** Ruta de la nota en el vault, si la tarea vino de ahí. */
  vault_note: string | null;
  proyecto: string;
  pomodoros: number;
}

export interface Proyecto {
  id: number;
  parent_id: number | null;
  name: string;
  ruta: string;
  nivel: number;
  archived: boolean;
  /** La carpeta del repo. Es lo que reconoce el proyecto por el directorio. */
  repo_path: string | null;
  /** La carpeta dentro del vault de Obsidian. */
  vault_path: string | null;
}

/** Una carpeta del vault con Backlog. */
export interface CarpetaVault {
  nombre: string;
  /** Historias que son trabajo pendiente. */
  vivas: number;
  /** El proyecto que ya la tiene ligada, si hay alguno. */
  ligada_a: string | null;
}

/** El parte de una importación. */
export interface Importacion {
  creadas: number;
  actualizadas: number;
  omitidas: number;
  sin_prioridad: string[];
  estados_raros: [string, string][];
  prioridad_congelada: string[];
}

export interface Tema {
  nombre: string;
  accent: string;
  foreground: string;
  background: string;
  selection_background: string;
  colores: string[];
  oscuro: boolean;
}

export type Orden =
  | { tipo: "start"; task_id: number }
  | { tipo: "pause" }
  | { tipo: "void" }
  | { tipo: "switch"; task_id: number }
  | { tipo: "done" }
  | { tipo: "interrupt"; externa: boolean }
  | { tipo: "skip_break" };

export const ETIQUETA_PRIORIDAD: Record<Prioridad, string> = {
  high: "alta",
  medium: "media",
  low: "baja",
};

/** `24:32`, o `--:--` con el reloj parado. */
export function reloj(s: Snapshot): string {
  if (s.phase === "idle") return "--:--";
  const m = Math.floor(s.remaining_secs / 60);
  const seg = s.remaining_secs % 60;
  return `${String(m).padStart(2, "0")}:${String(seg).padStart(2, "0")}`;
}

/** Un día del plan, tal como lo calcula `coffe_core::agenda`. */
export interface DiaAgenda {
  /** `AAAA-MM-DD`. */
  fecha: string;
  laborable: boolean;
  /** Pomodoros que vencen exactamente ese día. */
  debidos: number;
  /** Todo lo que vence desde hoy hasta ese día, inclusive. */
  deuda_acumulada: number;
  capacidad_acumulada: number;
  /** Lo que se debe para entonces ya no cabe en lo que queda. */
  imposible: boolean;
}

/** El plan entero: los días y lo que no se pudo contar. */
export interface Plan {
  dias: DiaAgenda[];
  /** Tareas con entrega y sin estimación: no pesan en ningún día. */
  sin_estimar: number;
}

export interface Config {
  pomodoro: {
    focus_minutes: number;
    short_break_minutes: number;
    long_break_minutes: number;
    long_break_every: number;
    strict: boolean;
    max_pomodoros_per_task: number;
  };
  agenda: { pomodoros_por_dia: number; fines_de_semana: boolean };
}

/** `AAAA-MM-DD` de hoy en hora local. `toISOString` daría el día en UTC, que
 *  de madrugada es otro día y desplazaría el calendario entero. */
export function hoyLocal(): string {
  const d = new Date();
  const mes = String(d.getMonth() + 1).padStart(2, "0");
  const dia = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${mes}-${dia}`;
}

/** Pomodoros que le faltan a una tarea. Sin estimación no se puede decir. */
export function pendientes(t: Tarea): number | null {
  if (t.estimate_pomodoros === null) return null;
  return Math.max(0, t.estimate_pomodoros - t.pomodoros);
}

// --- los reportes -------------------------------------------------------

/** Lo que pasó en un tramo de tiempo. Espejo de `ResumenPeriodo`. */
export interface ResumenPeriodo {
  pomodoros_completados: number;
  pomodoros_anulados: number;
  segundos_efectivos: number;
  tareas_terminadas: number;
  interrupciones_internas: number;
  interrupciones_externas: number;
  /** Días distintos con al menos un pomodoro. Sirve para leer el total: 40
   *  pomodoros en cuatro días no es lo mismo que en veinte. */
  dias_con_trabajo: number;
  segundos_con_claude: number;
}

/** Cuánto se lleva cada proyecto, con lo de sus hijos ya sumado. */
export interface CargaProyecto {
  project_id: number;
  ruta: string;
  pomodoros: number;
  anulados: number;
  segundos_efectivos: number;
}

/** Si tus estimaciones sirven de algo. */
export interface Precision {
  tareas: number;
  estimados: number;
  reales: number;
  subestimadas: number;
  clavadas: number;
  sobreestimadas: number;
  /** Terminadas SIN estimación: no entran en ninguna cuenta de arriba y por eso
   *  se enseñan. */
  sin_estimar: number;
}

/** El reporte entero. La tasa y el factor vienen calculados del backend: no se
 *  recalculan aquí o serían dos versiones del mismo número. */
export interface Reporte {
  dias: number;
  resumen: ResumenPeriodo;
  tasa_anulacion: number;
  cargas: CargaProyecto[];
  precision: Precision;
  factor: number | null;
}

/** `1 h 20 min`, o un guion cuando no hay nada que contar. */
export function duracion(segundos: number): string {
  if (segundos <= 0) return "—";
  const h = Math.floor(segundos / 3600);
  const m = Math.floor((segundos % 3600) / 60);
  return h > 0 ? `${h} h ${String(m).padStart(2, "0")} min` : `${m} min`;
}
