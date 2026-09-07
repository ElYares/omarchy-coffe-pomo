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
