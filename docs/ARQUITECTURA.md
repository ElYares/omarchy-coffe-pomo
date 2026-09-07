# Arquitectura

## El problema que define la forma

Un pomodoro es un reloj que **no puede depender de que una ventana esté abierta**.
Si el temporizador vive dentro de la interfaz, cerrarla mata la sesión y waybar
deja de saber la verdad. Por eso el sistema se parte en un demonio que posee el
reloj y dos clientes tontos que lo consultan.

```
┌─────────────┐   socket unix    ┌──────────────────────┐
│  coffe-app  │◄────────────────►│     coffe daemon     │
│  (Tauri)    │                  │  máquina de estados  │
│  taza,kanban│                  │  reloj + SQLite      │
│  calendario │                  └──────┬───────┬───────┘
└─────────────┘                         │       │
                                        │       │ pkill -RTMIN+15 waybar
┌─────────────┐   socket unix           │       ▼
│ coffe (CLI) │◄────────────────────────┘   ┌────────┐
│ bar/start/  │                             │ waybar │
│ pause/done  │◄─── hooks de Claude Code    └────────┘
└─────────────┘                             ┌────────┐
       │                                    │  mako  │
       ▼                                    └────────┘
  ~/.local/share/coffe/coffe.db
```

## Piezas

| Pieza | Qué es | Por qué existe |
|---|---|---|
| `coffe-core` | lib: dominio + persistencia | El pomodoro y sus reglas viven aquí, sin I/O de red ni UI. Es lo único que se prueba a fondo. |
| `coffe-ipc` | lib: protocolo del socket | Tipos compartidos entre daemon, CLI y app. Un solo sitio donde cambia el contrato. |
| `coffe` | bin: CLI y daemon (`coffe daemon`) | Un binario: menos que instalar. Arranque ~2 ms, que es lo que waybar necesita. |
| `coffe-app` | bin: ventana Tauri v2 | La parte visual. Habla el mismo socket que la CLI. |

## Stack y por qué

**Rust** para núcleo, daemon y CLI. Un binario sin runtime, arranque instantáneo
(waybar invoca `coffe bar` constantemente; Python o Node cuestan 40-80 ms por
invocación contra ~2 ms), `rusqlite` con SQLite embebido, `tokio` para el reloj
y el socket.

**Tauri v2 + React + TypeScript** para la ventana. La decisión la manda el
diseño: la taza que se vacía, el café que se recarga, el refri y la papelera son
SVG con máscaras animadas por CSS — trivial en web, semanas de Cairo en GTK4. El
sistema ya trae `webkit2gtk-4.1`, así que Tauri no instala nada. La app pesa
~8 MB en vez de los ~150 de Electron.

**SQLite** y no JSON: las métricas que se piden (tiempo efectivo contra tiempo
de calendario, interrupciones por tarea, carga por proyecto) son consultas
relacionales, no lecturas de un archivo.

### Descartado

- **GTK4 nativo**: todo el costo cae justo en la parte de diseño que da valor.
- **Electron**: peso y arranque, sin ventaja sobre Tauri aquí.
- **Python / Node para el módulo de waybar**: el arranque se nota en la barra.

## Decisiones de dominio

### D1 — El pomodoro es indivisible; la tarea no

Regla de Cirillo: si un pomodoro empieza, tiene que sonar. No existe un pomodoro
de 25 minutos con una pausa en medio.

Se resuelve en dos niveles:

- **Nivel pomodoro (estricto)**: no se pausa. Interrumpirlo lo **anula**
  (`voided`) y no cuenta para nada. Al volver se sirve taza nueva desde 25:00.
- **Nivel tarea (flexible)**: la tarea sí se aparca y el reloj acumulado de la
  tarea reanuda donde se quedó. El refri guarda **la tarea**, no el pomodoro.

Así el refri conserva su sentido visual y las estadísticas no mienten.

### D2 — Ciclos 25/5/15, canon

25 min de foco, 5 de descanso corto, 15 de descanso largo cada **4 pomodoros
completados** (los anulados no cuentan). El descanso es obligatorio: en modo
estricto no se puede arrancar un pomodoro nuevo mientras corre un descanso.

### D3 — SQLite manda; el vault se importa

La app es dueña de las tareas. Un importador lee los `HU-XXX.md` del Backlog del
vault de Obsidian y los trae al tablero guardando el enlace a la nota. La nota
queda como documentación; el estado y los tiempos viven en la base.

### D4 — Las prioridades se congelan al arrancar

Alta, media y baja. Solo se pueden cambiar mientras la tarea está `pending`.
En cuanto tiene un pomodoro encima, la prioridad es historia y no se reescribe.

### D5 — Interrupciones al estilo Cirillo

Se registran con su marca: internas (`'`, tú te distraes) y externas (`"`, algo
o alguien te interrumpe). Es el dato que responde "cuántas pausas hubo antes de
acabar la tarea".

### D6 — Una campana que nadie oyó no cuenta

Si el reloj descubre que un pomodoro venció hace más de dos minutos —el equipo
estaba suspendido, o el daemon caído— **no lo da por completado**: lo anula con
motivo `daemon_lost`. Regalar pomodoros que nadie trabajó es exactamente la
mentira que este proyecto existe para no contar.

Dentro de esos dos minutos de gracia sí suena, y se apunta en el minuto en que
debía sonar, no en el que se descubrió.

### D7 — Un tramo de trabajo se cierra cuando el reloj vuelve a cero

Los tramos (`task_sessions`) miden la dedicación real. Se abren al arrancar un
pomodoro y se cierran cuando acaba el ciclo, se aparca la tarea, se cambia de
tarea o se da por hecha — **nunca se quedan abiertos toda la noche** porque
alguien olvidó aparcar el viernes.

Cada cierre guarda su motivo, y solo `parked` y `switched` cuentan como pausa.
Sin eso, "cuántas veces se aparcó la tarea" sería en realidad "cuántos ciclos
tuvo", que no es lo mismo ni de lejos.

## Modelo de datos

- `projects` — árbol con `parent_id`, profundidad libre
  (`strapp > tl-mas`, `personal > labs > video-procesos`, `clientes > nutricore`).
  Opcionalmente `repo_path`, de donde sale la detección por `cwd`.
- `tasks` — proyecto, prioridad, estado, `due_date`, estimación en pomodoros,
  enlace al HU del vault, `first_started_at`, `completed_at`.
- `pomodoros` — inicio, fin, `completed | voided`, motivo de anulación.
- `interruptions` — tipo (`internal` / `external`), momento, duración.
- `task_sessions` — cada tramo real de trabajo, con el motivo de su cierre; de
  aquí sale el tiempo acumulado de una tarea que se aparcó y se retomó.

Las tres métricas que se pidieron salen de ahí:

| Métrica | De dónde |
|---|---|
| Tiempo de pomodoro | `pomodoros` con `completed` |
| Tiempo total de la tarea | `completed_at - first_started_at` |
| Dedicación real | suma de `task_sessions` |
| Pausas antes de acabar | `task_sessions` cerradas como `parked` o `switched` |
| Interrupciones | `interruptions`, internas y externas por separado |

## El protocolo

Socket unix con **JSON por líneas**: una petición por línea, una respuesta por
línea. Se eligió así porque se depura con `socat` y porque el cliente no
necesita runtime asíncrono — y el cliente que más corre es el módulo de waybar,
donde el arranque se ve.

`Subscribe` deja la conexión abierta y recibe un `Snapshot` por línea: en cada
transición, y cada segundo mientras haya reloj corriendo. Es lo que evita que
la barra arranque un proceso por segundo para su cuenta atrás.

El daemon separa lo que cambia cada segundo (la cuenta atrás) de lo que solo
cambia cuando pasa algo (qué tarea, cuántos pomodoros hoy, cuántas tazas en el
bote). Lo segundo cuesta consultas y no las paga el latido.

Los comandos de gestión (`project`, `task`) **no** pasan por el socket: van
directos a SQLite, para que apuntar una tarea funcione con el daemon apagado.

## Estados

**Reloj (daemon)**: `Idle` · `Focus` · `Ringing` · `ShortBreak` · `LongBreak`

**Tarea**: `pending` · `in_progress` · `paused` · `done` · `archived`

## Mapa de estados a animación

| Estado | Taza |
|---|---|
| `Focus` | Se vacía, proporcional al tiempo restante |
| `ShortBreak` / `LongBreak` | Se recarga |
| Tarea `paused` | La taza entra al refri; al reanudar sale y se sirve de nuevo |
| Pomodoro anulado | Café frío, se tira |
| Tarea `done` | La taza cae al bote |
| Bote lleno | Vaciar papelera archiva las tareas terminadas |

La taza queda **fuera del tema**: borde blanco y café americano fijos. Todo lo
demás toma color de `~/.config/omarchy/current/theme/colors.toml`.

## Integración con el escritorio

- **Waybar**: módulo `custom/coffe`, patrón `exec` → JSON + `signal`, igual que
  `claudebar` (RTMIN+13) y `quest` (RTMIN+14). Le toca **RTMIN+15**, que es la
  primera libre en la configuración actual.
- **Tema**: `~/.config/omarchy/hooks/theme-set.d/coffe` avisa al daemon y la
  ventana repinta en vivo, sin editar el hook grande.
- **Claude Code**: un hook en `Stop` llama a `coffe` para cerrar el tramo cuando
  la sesión que estaba desarrollando la tarea termina. Mismo patrón que los
  hooks `quest-state.sh` y `claude-notify.sh` que ya corren.

## Rutas

| Qué | Dónde |
|---|---|
| Base de datos | `~/.local/share/coffe/coffe.db` |
| Configuración | `~/.config/coffe/config.toml` |
| Socket | `$XDG_RUNTIME_DIR/coffe.sock` |
