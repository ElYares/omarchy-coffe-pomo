# Fases

Cada fase termina en algo que se puede usar o probar. No hay fase que solo
"prepare terreno" para la siguiente.

Estado: `[ ]` pendiente · `[~]` en curso · `[x]` hecha

---

## [x] Fase 0 — Cimientos

Workspace de Cargo con los cuatro miembros, rutas XDG, configuración por
defecto y las decisiones escritas.

**Entrega**: `cargo build` compila el esqueleto; `coffe --version` responde.

Hecho. `coffe paths` además imprime dónde vive cada cosa y qué ciclos están
en uso, que es lo primero que hace falta cuando algo no aparece.

- [x] Documentar arquitectura y decisiones (`docs/ARQUITECTURA.md`)
- [x] Documentar fases (este archivo)
- [x] Workspace: `coffe-core`, `coffe-ipc`, `coffe-cli`
- [x] Rutas XDG y carga de `~/.config/coffe/config.toml`
- [x] `CLAUDE.md` del repo

---

## [x] Fase 1 — El corazón: dominio y persistencia

La máquina de estados del pomodoro con sus reglas estrictas, y el esquema de
SQLite con migraciones. Sin red, sin UI, sin reloj de pared: el dominio recibe
un instante y devuelve el estado siguiente, que es lo que lo hace comprobable.

**Entrega**: `cargo test` cubriendo las reglas; una base que se crea sola.

Hecho: 39 pruebas verdes (21 de las reglas del método, 18 de persistencia).

- [x] Esquema y migraciones: `projects`, `tasks`, `pomodoros`,
      `interruptions`, `task_sessions`
- [x] Árbol de proyectos con `parent_id` y consulta de ruta completa
- [x] Máquina de estados `Idle → Focus → Ringing → Break → Idle`
- [x] Regla: el pomodoro no se pausa, se anula (D1)
- [x] Regla: descanso largo cada 4 **completados** (D2)
- [x] Regla: descanso obligatorio, no se solapa con un foco nuevo (D2)
- [x] Regla: la prioridad se congela al primer pomodoro (D4)
- [x] Regla: aviso si la estimación pasa de 7 pomodoros o no llega a 1
- [x] Interrupciones internas y externas (D5)
- [x] Tramos de tarea: aparcar y retomar acumulando tiempo
- [x] Pruebas de cada regla, incluidos los casos feos (anular a mitad,
      reiniciar el daemon con un foco vivo, cambio de tarea en caliente)

---

## [x] Fase 2 — Daemon y CLI

El reloj real, el socket unix y los comandos. A partir de aquí ya se puede
cronometrar desde la terminal.

**Entrega**: `coffe daemon` corriendo y `coffe start/pause/void/done/status`
funcionando de verdad.

Hecho, y probado contra un daemon real: ciclo completo de foco a descanso a
reloj parado, reinicio del daemon con un foco vivo, y las reglas estrictas
rechazando lo que tienen que rechazar. 56 pruebas.

Salió además `coffe task show`, que enseña las tres medidas del tiempo juntas
—efectivo, dedicación y calendario—, que es donde se ve lo que costó de verdad
una tarea.

- [x] Protocolo del socket en `coffe-ipc` (petición/respuesta + suscripción)
- [x] `coffe daemon`: reloj, persistencia por transición, recuperación al
      arrancar si había un foco vivo
- [x] Comandos: `start`, `pause`, `resume`, `void`, `done`, `switch`, `status`,
      `interrupt`, `skip-break`, `trash`
- [x] Gestión de proyectos y tareas por CLI: `project add|list`,
      `task add|list|show|priority`, con proyectos por ruta (`strapp/tl-mas`)
- [x] Unidad de systemd de usuario para el daemon

---

## [x] Fase 3 — Waybar

El pomodoro usable a diario sin abrir ninguna ventana.

**Entrega**: el módulo en la barra, con clics y avisos.

Hecho e instalado. El módulo **no lleva `interval`**: se queda suscrito al
daemon y recibe una línea de JSON por segundo, así que la cuenta atrás se mueve
sin arrancar 86.400 procesos al día. Con el daemon apagado imprime texto vacío
—el módulo se esconde solo— y sale; waybar lo relanza.

Queda puesto en `config.jsonc` y `config.nordfjell.jsonc` (más sus `style`).
**Los otros cuatro temas no lo tienen**: `midnight-statusline` y `windows-xp`
usan otro vocabulario de colores y hay que elegirles el suyo.

- [x] `coffe bar`: JSON con `text`, `tooltip`, `class` por estado
- [x] Señal **RTMIN+15** desde el daemon en cada transición
- [x] Módulo `custom/coffe` en `~/.config/waybar/config.jsonc`
      (con respaldo previo del archivo)
- [x] Los bloques instalados, copiados a `packaging/` para que el repo y la
      máquina no se separen
- [x] Estilos por clase en `style.css`, tomando color del tema
- [x] Clics: derecho aparca, medio anula. El izquierdo saca del refri lo
      último guardado — pasa a abrir la ventana en la Fase 4
- [x] Avisos por mako al sonar y al terminar el descanso
- [x] Atajos de Hyprland: `SUPER ALT + P` alterna arrancar y refri,
      `SUPER SHIFT ALT + P` apunta una interrupción externa

---

## [ ] Fase 4 — La ventana y la taza

Tauri levantado, tema sincronizado y la pieza de diseño que da nombre al
proyecto.

**Entrega**: la ventana abre, muestra el estado real y la taza se vacía.

- [ ] Tauri v2 + React + TS + Vite, conectado al socket del daemon
- [ ] Lector de `colors.toml` a variables CSS; hook `theme-set.d/coffe`
      para repintar en vivo
- [ ] Regla de ventana en Hyprland (flotante, tamaño y posición)
- [ ] Taza en SVG: nivel de café por máscara animada
- [ ] Estado foco: se vacía · descanso: se recarga
- [ ] Estado aparcado: la taza entra al refri y sale al retomar
- [ ] Pomodoro anulado: café frío que se tira
- [ ] Borde blanco y café americano, fijos, fuera del tema

---

## [ ] Fase 5 — Tablero kanban

**Entrega**: se crean, mueven y priorizan tareas desde la ventana.

- [ ] Columnas por estado, filtro por proyecto y por categoría del árbol
- [ ] Arrastrar y soltar entre columnas
- [ ] Alta, media y baja, con el bloqueo de la regla D4 visible en la interfaz
- [ ] Selector de proyecto que refleja el árbol completo
- [ ] Iniciar un pomodoro desde la tarjeta
- [ ] Papelera: las terminadas caen al bote, y vaciarlo las archiva

---

## [ ] Fase 6 — Calendario

**Entrega**: ver qué se entrega y cuándo.

- [ ] Fecha de entrega por tarea
- [ ] Vista de mes y vista de agenda
- [ ] Señal visual de vencida y de vence hoy
- [ ] Carga estimada por día, en pomodoros, para ver el día imposible antes
      de vivirlo

---

## [ ] Fase 7 — Vault y Claude

**Entrega**: las tareas llegan de donde ya las escribes, y el tiempo se cierra
solo.

- [ ] Importador de `10 Projects/<proy>/Backlog/HU-XXX.md` del vault
- [ ] Enlace de vuelta a la nota desde la tarjeta
- [ ] Detección de proyecto por `cwd` usando `repo_path`
- [ ] Hook de Claude Code en `Stop` que cierra el tramo de la tarea
- [ ] Marca de qué parte del trabajo se hizo con Claude

---

## [ ] Fase 8 — Reportes y empaquetado

**Entrega**: instalable, y con las respuestas que motivaron el proyecto.

- [ ] Reportes: tiempo efectivo contra tiempo de calendario, interrupciones
      por tarea, carga por proyecto y por categoría, precisión de la estimación
- [ ] Exportación a CSV
- [ ] `install.sh`: binarios, unidad de systemd, módulo de waybar, hooks
- [ ] `README` con capturas
