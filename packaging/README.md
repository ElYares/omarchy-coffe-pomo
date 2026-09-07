# Instalación

Todavía no hay instalador — llega en la Fase 8. Mientras tanto:

```bash
# 1. Compilar e instalar el binario
cargo build --release
install -Dm755 target/release/coffe ~/.local/bin/coffe

# 2. Dejar el reloj corriendo
install -Dm644 packaging/coffe.service ~/.config/systemd/user/coffe.service
systemctl --user daemon-reload
systemctl --user enable --now coffe

# 3. Comprobar
coffe status
journalctl --user -u coffe -f
```

## La ventana

```bash
cd app && pnpm install && pnpm tauri build --no-bundle && cd ..
install -Dm755 target/release/coffe-app ~/.local/bin/coffe-app

# La regla de ventana (flotante y centrada):
#   hypr/windowrules-coffe.conf -> ~/.config/hypr/
# y anadir su `source` en ~/.config/hypr/hyprland.conf
```

**Tiene que ser `pnpm tauri build`, no `cargo build --release`.** El CLI de
Tauri activa la feature que hace que la ventana sirva los assets embebidos;
compilando solo con cargo, el binario de release sigue apuntando a `devUrl` y
abre con "Connection refused" aunque no haya ningun Vite a la vista. Es un
fallo silencioso: compila, enlaza y arranca, y solo se ve al abrir la ventana.

En desarrollo son dos procesos: `cd app && pnpm dev` levanta Vite en el 1420, y
`cargo run -p coffe-app` abre la ventana contra ese servidor.

## La barra y los atajos

```bash
# El modulo de waybar: pegar el bloque en la config y los estilos en el css.
#   waybar/module.jsonc  -> ~/.config/waybar/config.jsonc
#   waybar/style.css     -> ~/.config/waybar/style.css
# Y nombrar "custom/coffe" en la lista de modulos que corresponda.
omarchy restart waybar

# Los atajos:
#   hypr/bindings.conf   -> ~/.config/hypr/bindings.conf
hyprctl reload && hyprctl configerrors
```

Omarchy conmuta la barra por tema: el hook `theme-set` copia
`config.<tema>.jsonc` y `style.<tema>.css` sobre los activos. O sea que el
modulo hay que ponerlo tambien en la variante del tema que se use, o
desaparecera al cambiar de tema y volver.

Los nombres de color del css son los de Nordfjell y Emberwood. Un tema con otro
vocabulario necesita los suyos; lo unico garantizado en todos es lo que define
`~/.config/omarchy/current/theme/waybar.css`.

## Rutas

El daemon guarda todo en `~/.local/share/coffe/coffe.db` y escucha en
`$XDG_RUNTIME_DIR/coffe.sock`. Para probar sin tocar tus datos reales, apunta
`XDG_DATA_HOME`, `XDG_CONFIG_HOME` y `XDG_RUNTIME_DIR` a otro sitio — pero deja
`XDG_RUNTIME_DIR` **corto**: la ruta de un socket unix no puede pasar de 108
bytes.

## Configuración

`~/.config/coffe/config.toml`. Si no existe, se usan los valores del método
clásico:

```toml
[pomodoro]
focus_minutes = 25
short_break_minutes = 5
long_break_minutes = 15
long_break_every = 4
strict = true
max_pomodoros_per_task = 7

[agenda]
# Con que se compara la carga de un dia en el calendario. Ocho pomodoros son
# cuatro horas de foco, que ya es un dia honesto.
pomodoros_por_dia = 8
# Por defecto el fin de semana no suma capacidad: una agenda que da por hecho
# que trabajas el sabado esconde justo el problema que deberia ensenar.
fines_de_semana = false
```

`strict = false` permite pausar y reanudar el pomodoro y saltarse el descanso.
Los pomodoros hechos así quedan marcados aparte y no cuentan como canónicos.
