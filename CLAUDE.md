# coffe

Pomodoro para Omarchy: módulo de waybar + ventana con taza de café.

Lee `docs/ARQUITECTURA.md` antes de tocar el dominio y `docs/FASES.md` para
saber en qué punto está el trabajo.

## Estructura

```
crates/coffe-core/   dominio (machine.rs), persistencia (db/), servicio (service.rs)
crates/coffe-ipc/    protocolo del socket + cliente síncrono
crates/coffe-cli/    binario `coffe`: CLI, daemon (daemon.rs) y barra (barra.rs)
app/                 la ventana: React + TS sobre Tauri v2
app/src-tauri/       su backend: comandos, tema y el hilo suscrito al reloj
packaging/           systemd, waybar, hyprland y notas de instalación
```

`service.rs` es el puente: aplica un comando a la máquina y escribe lo que sus
efectos dicen. Cada efecto es **una** escritura; si ese `match` empieza a tener
lógica dentro, es que le falta un efecto a la máquina.

## Reglas que no se negocian

Están en `docs/ARQUITECTURA.md` como D1..D5, y cada una tiene su prueba en
`crates/coffe-core/tests/machine.rs`. Si una prueba estorba, el que está mal es
el cambio, no la prueba: el valor entero de la app es que no mienta sobre el
tiempo del usuario.

Resumen: el pomodoro es indivisible y no se pausa (interrumpirlo lo anula); si
empieza tiene que sonar (terminar la tarea antes no corta el reloj); el descanso
es obligatorio; el descanso largo llega cada 4 pomodoros **completados**; la
prioridad se congela con el primer pomodoro y **no se descongela reabriendo la
tarea** —la condición mira `first_started_at`, no el estado—; un pomodoro que
venció con el equipo dormido se anula en vez de regalarse; y un tramo de trabajo
se cierra cuando el reloj vuelve a cero, nunca al día siguiente.

**El estado de una tarea lo manda el reloj, no la interfaz.** El tablero no
escribe estados: le pide al daemon lo que toca al reloj (arrancar, cambiar,
aparcar, terminar) y solo escribe directo lo que no lo toca. La decisión vive
en `decidir()` en `app/src-tauri/src/lib.rs`, con pruebas.

**Un total que se calla lo que ignora es peor que no tener total**, porque se
lee como si lo supiera todo. Si un cálculo deja algo fuera —una tarea sin
estimar, una nota con un estado que no se reconoce— eso se cuenta aparte y se
enseña, nunca se traga.

Cuando una regla se enseña en la interfaz —el congelado de la prioridad, por
ejemplo— tiene que mirar **el mismo dato** que el backend. Dos copias de la
misma regla con condiciones distintas se separan, y el usuario ve un botón
activo que al pulsarlo falla.

## Convenciones

- **Idioma**: comentarios, nombres de prueba y mensajes de error en español.
  Los identificadores del dominio que ya son términos técnicos (`Task`,
  `Priority`, `TimerState`) se quedan en inglés.
- **La máquina de estados es pura**: recibe `now` como argumento, nunca mira el
  reloj del sistema. Por eso se puede probar un descanso largo sin esperar dos
  horas. No metas `Utc::now()` dentro de `machine.rs`.
- **La máquina no persiste**: devuelve `Effect`s y el daemon los escribe.
- **Las invariantes fuertes viven en el esquema** (índices parciales), no solo
  en Rust: un daemon con un bug no debe poder dejar dos pomodoros abiertos.
- **Las migraciones no se editan**, se añaden. `db/schema.rs`, versionadas con
  `PRAGMA user_version`.
- **Fechas**: RFC 3339 en UTC en la base. El huso se resuelve al pintar.

## Comprobar

```bash
cargo test                   # 114 pruebas
cargo clippy --all-targets   # sin avisos
cargo fmt --check
cd app && pnpm build         # tsc estricto + vite
```

La ventana en desarrollo son **dos procesos**: `cd app && pnpm dev` levanta Vite
en el 1420 y `cargo run -p coffe-app` abre contra él. En depuración Tauri usa
`devUrl`, no `dist`: sin Vite la ventana abre con "Connection refused".

**Para release: `cd app && pnpm tauri build --no-bundle`, nunca `cargo build
--release -p coffe-app`.** El CLI de Tauri activa la feature que sirve los
assets embebidos; con cargo a secas el binario de release sigue apuntando a
`devUrl`. Compila, enlaza y arranca sin quejarse: el fallo solo se ve al abrir
la ventana, y es fácil confundirlo con una instancia de desarrollo que quedó
viva.

**Tauri se traga el drag-and-drop de la página.** Engancha el DnD del sistema
—soltar archivos sobre la ventana— y con eso las tarjetas se arrastran pero el
`drop` no llega nunca. Se apaga con `"dragDropEnabled": false` en la ventana de
`tauri.conf.json`. Y **WebKit exige que `dragstart` escriba en `dataTransfer`**;
sin esa línea, tampoco hay drop. Las dos cosas juntas, o no funciona.

`tauri.conf.json` es **JSON estricto**: un comentario lo rompe.

**Tauri v2 exige `capabilities/default.json`.** Sin `core:event:allow-listen`
la ventana arranca, pinta y responde a `invoke`, pero no recibe un solo evento:
se queda congelada en el primer estado, sin error visible. El aviso solo sale
por la consola del webview.

Para probar el daemon sin tocar los datos reales, apunta `XDG_DATA_HOME`,
`XDG_CONFIG_HOME` y `XDG_RUNTIME_DIR` a otro sitio. **`XDG_RUNTIME_DIR` tiene
que ser una ruta corta**: un socket unix no admite más de 108 bytes, y el
directorio de scratchpad se pasa.

## Sistema

Este repo se integra con la máquina del usuario. Antes de tocar
`~/.config/waybar/`, `~/.config/hypr/` o `~/.config/omarchy/`, usa la skill
`omarchy` y **haz respaldo con fecha del archivo** antes de editarlo, que es la
convención que ya sigue ese directorio.

- Señal de waybar reservada para el pomodoro: **RTMIN+15** (7 a 14 ya están en
  uso por otros módulos).
- Waybar no recarga solo: `omarchy restart waybar`.
