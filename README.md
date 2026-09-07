# coffe

Un pomodoro para [Omarchy](https://omarchy.org/): vive en waybar y abre una
ventana donde el temporizador es una taza de café que se vacía mientras
trabajas y se recarga en el descanso.

Sigue el método de Cirillo de verdad, no de adorno: el pomodoro es indivisible,
el descanso es obligatorio, y lo que se interrumpe se anula y se registra como
tal. Las estadísticas no sirven de nada si mienten.

## Qué hace

- **Proyectos en árbol** — `strapp / tl-mas`, `personal / labs / video`,
  `clientes / nutricore`, a la profundidad que haga falta.
- **Tablero kanban** con prioridad alta, media y baja, que se congela en cuanto
  la tarea arranca.
- **Calendario** de entregas, para ver el día imposible antes de vivirlo.
- **Tiempos que se pueden auditar**: tiempo efectivo contra tiempo de
  calendario, pomodoros tirados y por qué, e interrupciones internas y externas.
- **Sin ventana abierta**: el reloj lo lleva un daemon; waybar y la app solo lo
  miran.

## Estado

En construcción. Ver [`docs/FASES.md`](docs/FASES.md).

Hechas las fases 0 a 3: el dominio, la persistencia, el reloj y la barra. Ya se
usa a diario —la taza vive en waybar con su cuenta atrás— y falta la ventana
(Fase 4). 57 pruebas.

```bash
coffe project add strapp
coffe project add tl-mas --parent strapp
coffe project move tl-mas --parent clientes   # el árbol se reorganiza cuando cambia el trabajo
coffe project repo tl-mas ~/develop/work/tl-mas

coffe task add "integración de facturación" -p strapp/tl-mas -P alta -d 2026-09-11 -e 4
coffe task edit 1 --due 2026-09-15

coffe start 1        # arranca el pomodoro
coffe interrupt -e   # apunta que te interrumpieron, sin cortar nada
coffe pause          # al refri: anula el pomodoro, guarda la tarea
coffe resume         # saca del refri lo último que guardaste
coffe status
coffe task show 1    # efectivo, dedicación y calendario, uno debajo de otro
```

En la barra queda así, con la cuenta atrás moviéndose:

```
☕ 24:40   15% · 2h 00m   ⚡1  🏠1   <   LAN   VOL 70%   PWR 93%
```

Clic derecho manda la tarea al refri, el central tira el pomodoro. `SUPER ALT+P`
alterna arrancar y aparcar sin soltar el teclado.

Instalación en [`packaging/README.md`](packaging/README.md).

## Desarrollo

```bash
cargo test
cargo run -p coffe-cli --bin coffe -- paths
```

Arquitectura y decisiones: [`docs/ARQUITECTURA.md`](docs/ARQUITECTURA.md).
