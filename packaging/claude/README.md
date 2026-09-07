# Los hooks de Claude Code

Miden **cuánto del trabajo se hizo con Claude**, y lo miden de verdad: abren un
tramo cuando Claude empieza a responder y lo cierran cuando para. No se estima
ni se deduce del historial.

Se funden en `~/.claude/settings.json` (haz copia antes; si ya tienes hooks en
esos eventos, estos se AÑADEN, no los reemplazan):

```jsonc
"UserPromptSubmit": [{ "hooks": [
  { "type": "command", "command": "$HOME/.local/bin/coffe claude start", "timeout": 5 }
]}],
"Stop": [{ "hooks": [
  { "type": "command", "command": "$HOME/.local/bin/coffe claude stop", "timeout": 5 }
]}]
```

Ruta absoluta a proposito: un hook puede correr con un PATH pelado.

**En `SubagentStop` no**: se dispara con la sesion principal todavia trabajando,
asi que cerrar ahi perderia el resto del tramo.

## Que apunta y que no

- **Sin pomodoro corriendo no apunta nada.** No hay tarea a la que atribuir el
  tiempo, y arrancar un pomodoro por su cuenta seria peor: el metodo es una
  decision del usuario, no algo que le pase por escribirle a Claude.
- **Si el directorio es de OTRO proyecto, tampoco.** Estar en un pomodoro de
  `tl-mas` mientras Claude trabaja en otro repo no es tiempo de esa tarea.
- **Un directorio que no es de ningun proyecto si cuenta.** No hay evidencia en
  contra, y exigir que cada repo este registrado dejaria la medicion vacia justo
  para quien no ha configurado nada.
- Los hooks corren en **cada mensaje**: abrir dos veces el mismo tramo no lo
  duplica.
- Una sesion que muere de golpe deja el tramo abierto; el daemon lo cierra al
  arrancar, en el instante en que empezo. No sabemos cuanto duro, asi que no se
  inventa.

Se lee con `coffe task show <id>`.
