// La taza.
//
// Es SVG dibujado a mano y no una imagen porque el nivel del café tiene que
// moverse con el reloj: se recorta el líquido con un `clipPath` que sigue el
// interior de la taza, y lo que se anima es la ALTURA de ese líquido.
//
// La taza NO sigue al tema de Omarchy. Su borde es hueso y su café es café; si
// siguiera al tema, un tema verde daría café verde y dejaría de ser una taza.

export type EstadoTaza =
  | "vacia" // reloj parado, sin nada esperando
  | "llenando" // descanso: se recarga
  | "vaciando" // pomodoro corriendo
  | "repaso" // la tarea ya está hecha y el reloj sigue
  | "refri" // tarea aparcada
  | "fria"; // pomodoro anulado: café que se tira

interface Props {
  estado: EstadoTaza;
  /** Cuánto café queda, de 0 a 1. */
  nivel: number;
}

// Los colores de la taza, fuera del tema a propósito.
const HUESO = "#f3ece2";
const CAFE = "#6f4e37";
const CAFE_CLARO = "#8a6247";
const CAFE_FRIO = "#5c5450";
const ESCARCHA = "#bcd4e6";

// El interior útil de la taza, en coordenadas del viewBox. El líquido se mueve
// entre estas dos alturas y nunca fuera.
const BOCA = 74;
const FONDO = 158;

export function Taza({ estado, nivel }: Props) {
  const limpio = Math.min(1, Math.max(0, nivel));
  const alto = (FONDO - BOCA) * limpio;
  const y = FONDO - alto;

  const frio = estado === "fria";
  const enRefri = estado === "refri";
  const humea = (estado === "vaciando" || estado === "repaso" || estado === "llenando") && limpio > 0.04;

  return (
    <svg
      className={`taza taza--${estado}`}
      viewBox="0 0 200 200"
      role="img"
      aria-label={descripcion(estado, limpio)}
    >
      <defs>
        {/* El líquido solo puede pintarse dentro de la taza. */}
        <clipPath id="dentro">
          <path d="M 52 70 L 148 70 L 137 150 Q 135 160 124 160 L 76 160 Q 65 160 63 150 Z" />
        </clipPath>
        <linearGradient id="cafe" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={frio ? CAFE_FRIO : CAFE_CLARO} />
          <stop offset="100%" stopColor={frio ? "#4a4340" : CAFE} />
        </linearGradient>
      </defs>

      {enRefri && <Refri />}

      <g className="taza__conjunto">
        {humea && <Vapor />}

        {/* El asa va detrás del cuerpo para que el trazo no se cruce. */}
        <path
          d="M 148 88 q 26 2 26 22 q 0 20 -26 22"
          fill="none"
          stroke={HUESO}
          strokeWidth="7"
          strokeLinecap="round"
        />

        <g clipPath="url(#dentro)">
          {/* Lo que se anima es esta `y`. Un segundo lineal, que es justo el
              ritmo al que el daemon manda snapshots: así el nivel baja de
              corrido en vez de a saltos. */}
          <rect className="taza__cafe" x="45" y={y} width="110" height={FONDO - y + 4} fill="url(#cafe)" />
          {/* La elipse de arriba le da superficie al líquido: sin ella parece
              un rectángulo de color, no café. */}
          {limpio > 0.02 && (
            <ellipse className="taza__superficie" cx="100" cy={y} rx="55" ry="5" fill={frio ? "#6b625d" : CAFE_CLARO} />
          )}
          {frio && limpio > 0.02 && (
            /* La nata del café frío: la señal de que ese pomodoro se tiró. */
            <ellipse cx="100" cy={y} rx="47" ry="4" fill="none" stroke="#8e857f" strokeWidth="2" opacity="0.8" />
          )}
        </g>

        {/* El cuerpo va después del líquido: el trazo tiene que quedar encima. */}
        <path
          d="M 52 70 L 148 70 L 137 150 Q 135 160 124 160 L 76 160 Q 65 160 63 150 Z"
          fill="none"
          stroke={HUESO}
          strokeWidth="7"
          strokeLinejoin="round"
        />
        <ellipse cx="100" cy="70" rx="48" ry="8" fill="none" stroke={HUESO} strokeWidth="7" />

        <path
          d="M 58 172 L 142 172"
          stroke={HUESO}
          strokeWidth="7"
          strokeLinecap="round"
          opacity="0.85"
        />
      </g>

      {enRefri && <Escarcha />}
    </svg>
  );
}

function Vapor() {
  // Tres hilos con desfase distinto: a la vez subiendo parecen un peine.
  return (
    <g className="taza__vapor" stroke={HUESO} strokeWidth="4" strokeLinecap="round" fill="none">
      <path d="M 82 52 q 7 -10 0 -20 q -7 -10 0 -20" style={{ animationDelay: "0s" }} />
      <path d="M 100 46 q 7 -10 0 -20 q -7 -10 0 -20" style={{ animationDelay: "0.7s" }} />
      <path d="M 118 52 q 7 -10 0 -20 q -7 -10 0 -20" style={{ animationDelay: "1.4s" }} />
    </g>
  );
}

function Refri() {
  return (
    <g className="taza__refri" stroke={ESCARCHA} fill="none" strokeWidth="5" strokeLinejoin="round">
      <rect x="22" y="14" width="156" height="176" rx="12" />
      {/* La junta de la puerta: es lo que lo hace leerse como refri y no como
          una caja cualquiera. */}
      <path d="M 22 62 L 178 62" />
      <path d="M 158 32 L 158 48" strokeLinecap="round" />
      <path d="M 158 80 L 158 104" strokeLinecap="round" />
    </g>
  );
}

function Escarcha() {
  // Va delante de todo: es el cristal de la puerta, no una capa de la taza.
  return (
    <g className="taza__escarcha" fill={ESCARCHA} opacity="0.16">
      <rect x="22" y="14" width="156" height="176" rx="12" />
    </g>
  );
}

function descripcion(estado: EstadoTaza, nivel: number): string {
  const pct = Math.round(nivel * 100);
  switch (estado) {
    case "vaciando":
      return `Taza de café al ${pct}%, vaciándose`;
    case "llenando":
      return `Taza recargándose, al ${pct}%`;
    case "repaso":
      return `Taza al ${pct}%: la tarea está hecha y el pomodoro sigue`;
    case "refri":
      return "Taza guardada en el refrigerador";
    case "fria":
      return "Café frío: el pomodoro se anuló";
    case "vacia":
      return "Taza vacía, sin pomodoro";
  }
}
