# Glosario

Qué significa cada palabra del dominio y cuáles **no** usar en su lugar.

Existe porque aquí varias palabras que en la calle son sinónimos nombran reglas
distintas: aparcar no es pausar, anular no es cancelar, apartar no es borrar. Y
cada una deja una huella distinta en el tiempo medido. Confundirlas en una
conversación acaba confundiéndolas en el código.

Formato tomado del `CONTEXT.md` de usememos/memos. Si una definición de aquí
choca con `ARQUITECTURA.md` o con una prueba, el que está mal es este archivo.

---

## El reloj

**Pomodoro**:
Un bloque de 25 minutos de foco sobre **una** tarea. Es indivisible: o suena o
se anula; no hay pomodoro con una pausa en medio. Regla D1.
_Evitar_: sesión, bloque, intervalo, sprint

**Sonar**:
Que un pomodoro llegue a sus 25 minutos. Es lo único que lo vuelve
`completed` y lo que cuenta en los reportes. Al sonar, el reloj pasa por
`Ringing` antes del descanso. Una campana que vence con el equipo dormido más de
dos minutos no suena: se anula con motivo `daemon_lost`. Regla D6.
_Evitar_: terminar el pomodoro, acabar, cumplir

**Anular**:
Tirar un pomodoro en curso. Queda como `voided`, con su motivo, y no cuenta
para nada: ni tiempo efectivo ni descanso largo. En los reportes sale como
«tirados», contado aparte, y nunca suma. Regla D1.
_Evitar_: cancelar, pausar, parar, abortar

**Descanso**:
Los 5 minutos que siguen a un pomodoro que sonó. Es obligatorio: en modo
estricto no se puede arrancar otro pomodoro mientras corre, y `skip-break` se
rechaza. Regla D2.
_Evitar_: pausa

**Descanso largo**:
15 minutos en lugar de 5, cada 4 pomodoros **completados**. Los anulados no
cuentan para llegar a él. Regla D2.
_Evitar_: descanso grande, recreo

**Interrupción interna**:
Tú te distraes (marca `'`). Se apunta y el pomodoro sigue: apuntarla es todo lo
que hace. Si no cabe en el pomodoro, se anula a mano. Regla D5.
_Evitar_: pausa

**Interrupción externa**:
Algo o alguien te interrumpe (marca `"`). Igual que la interna: se apunta sin
cortar el pomodoro. Regla D5.
_Evitar_: pausa

---

## La tarea

**Aparcar**:
Meter la tarea en el refri. Si había un pomodoro corriendo, **se anula** (motivo
`paused`); lo que se guarda es la tarea y su tiempo acumulado, no el pomodoro.
Cierra el tramo con motivo `parked`. El verbo de la CLI se llama `coffe pause`
por costumbre, pero lo que pasa es esto. Regla D1 y decisión 001.
_Evitar_: pausar el pomodoro, suspender, congelar

**Refri**:
Donde espera la tarea aparcada. `coffe resume` la saca.
_Evitar_: pausa, pendientes

**Apartar**:
Decidir que una tarea ya no se hace: pasa a `archived` directo en la base, sin
tocar el reloj, y conserva su historial y sus pomodoros en los reportes. Apartar
la tarea que tiene el reloj encima se rechaza. Es `coffe task drop`.
Decisión 003.
_Evitar_: borrar, eliminar, cancelar (borrar es `task rm`, y se lleva el
historial)

**Bote**:
Las tareas terminadas que todavía no se han archivado: las «tazas del bote».
Vaciarlo las archiva en bloque; no borra nada. En el código es `papelera`.
_Evitar_: basura

> **Choque abierto:** la ventana usa «bote» para dos cosas. La barra de estado
> dice «27 en el bote» (tareas terminadas) y Reportes dice «ninguno se fue al
> bote» y «N al bote» (pomodoros **anulados**). Hasta que se resuelva, en una
> conversación di «tirados» para los pomodoros y deja «bote» para las tareas.

**Tramo**:
Un trecho continuo de trabajo sobre una tarea (`task_sessions`). Se abre al
arrancar un pomodoro y se cierra cuando el reloj vuelve a cero: al acabar el
ciclo, al aparcar, al cambiar de tarea o al darla por hecha. Nunca se queda
abierto de un día para otro. Solo `parked` y `switched` cuentan como pausa.
Regla D7.
_Evitar_: sesión, bloque

**Prioridad congelada**:
Alta, media o baja, y solo se cambia mientras la tarea no ha tenido ningún
pomodoro. La condición mira `first_started_at`, no el estado: **reabrir la
tarea no la descongela**. Regla D4.
_Evitar_: prioridad bloqueada, fijada

**Estimación**:
Cuántos pomodoros crees que costará la tarea, dicho **antes** de empezar. Avisa
si pasa de 7 (hay que partirla) o no llega a 1. Una tarea sin estimación no pesa
en el calendario ni entra en el reporte de precisión, y por eso se cuenta
aparte en vez de callarse.
_Evitar_: esfuerzo, puntos, tamaño

---

## Las tres medidas del tiempo

Una tarea tiene tres tiempos y no son intercambiables. Los tres salen en
`coffe task show`.

**Tiempo efectivo**:
La suma de los pomodoros que **sonaron**. Es el foco que vale; lo anulado no
entra.
_Evitar_: tiempo trabajado, tiempo real

**Dedicación**:
La suma de los tramos, descansos incluidos. Queda entre el efectivo y el de
calendario.
_Evitar_: tiempo invertido, tiempo total

**Tiempo de calendario**:
Del primer arranque de la tarea a su final, con los días muertos dentro. Dice
cuánto tardó en salir, no cuánto se trabajó.
_Evitar_: duración, tiempo transcurrido, lead time
