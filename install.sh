#!/usr/bin/env bash
#
# Instala coffe. Es idempotente: correrlo dos veces no rompe nada.
#
# Lo que NO hace, a proposito:
#   - No edita tu config de waybar ni la de hyprland. Son archivos con historia
#     y con tus decisiones dentro; te dice que pegar y donde.
#   - No toca ~/.claude/settings.json. Mismo motivo.
# Ver packaging/README.md para esos tres pasos.

set -euo pipefail

BIN="${HOME}/.local/bin"
UNIDADES="${HOME}/.config/systemd/user"
RAIZ="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

verde() { printf '\033[32m%s\033[0m\n' "$1"; }
aviso() { printf '\033[33m%s\033[0m\n' "$1"; }
malo()  { printf '\033[31m%s\033[0m\n' "$1" >&2; }

# --- lo que hace falta antes de empezar -------------------------------------

falta=()
command -v cargo >/dev/null || falta+=("cargo (rust)")
command -v pnpm  >/dev/null || falta+=("pnpm")
pkg-config --exists webkit2gtk-4.1 2>/dev/null || falta+=("webkit2gtk-4.1")

if ((${#falta[@]})); then
  malo "Falta: ${falta[*]}"
  malo "En Arch:  sudo pacman -S rust pnpm webkit2gtk-4.1"
  exit 1
fi

# --- la ventana ---------------------------------------------------------------
#
# Con el CLI de Tauri y NO con `cargo build --release`: es el CLI el que activa
# la feature que sirve los assets embebidos. Con cargo a secas el binario
# compila, enlaza y arranca, y abre en blanco con "Connection refused".

echo "Compilando (la primera vez tarda: Tauri son ~600 crates)…"
cd "$RAIZ/app"
pnpm install --silent
pnpm exec tauri build --no-bundle
cd "$RAIZ"
cargo build --release -p coffe-cli

install -Dm755 "$RAIZ/target/release/coffe"     "$BIN/coffe"
install -Dm755 "$RAIZ/target/release/coffe-app" "$BIN/coffe-app"
verde "Binarios en $BIN"

case ":$PATH:" in
  *":$BIN:"*) ;;
  *) aviso "Ojo: $BIN no esta en tu PATH." ;;
esac

# --- el reloj -----------------------------------------------------------------

install -Dm644 "$RAIZ/packaging/coffe.service" "$UNIDADES/coffe.service"
systemctl --user daemon-reload
systemctl --user enable --now coffe
verde "Daemon corriendo: $(systemctl --user is-active coffe)"

# --- lo que hay que pegar a mano ---------------------------------------------

cat <<EOF

$(verde "Listo.")  Prueba con:   coffe status

Faltan tres cosas que NO toco por ti, porque son archivos tuyos con historia:

  1. El modulo de waybar
       packaging/waybar/module.jsonc  ->  ~/.config/waybar/config.jsonc
       packaging/waybar/style.css     ->  ~/.config/waybar/style.css
     Y nombra "custom/coffe" en la lista de modulos. Luego:
       omarchy restart waybar

     Omarchy conmuta la barra POR TEMA: ponlo tambien en config.<tema>.jsonc
     y style.<tema>.css, o desaparecera al cambiar de tema y volver.

  2. Los atajos
       packaging/hypr/bindings.conf        ->  ~/.config/hypr/bindings.conf
       packaging/hypr/windowrules-coffe.conf -> ~/.config/hypr/
     y su \`source\` en hyprland.conf. Luego:
       hyprctl reload && hyprctl configerrors

  3. Los hooks de Claude Code
       packaging/claude/README.md  dice que pegar en ~/.claude/settings.json

Y para empezar:
  coffe project add <proyecto>
  coffe task add "lo que sea"
  coffe start <id>
EOF
