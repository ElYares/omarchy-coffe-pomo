// El árbol de proyectos.
//
// Un proyecto no es solo un nombre: puede llevar la carpeta de su repo —lo que
// hace que coffe lo reconozca por el directorio en el que estás— y la carpeta
// de su backlog en el vault. Sin eso el árbol existe pero no trabaja.
//
// Mover se hace con un desplegable y no arrastrando. Aquí el arrastre no
// aportaría: son pocas filas, la jerarquía se lee mejor en una lista que en un
// gesto, y ya nos costó una vez que el webview se lo tragara.

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { CarpetaVault, Importacion, Proyecto, Tarea } from "./tipos";

interface Props {
  proyectos: Proyecto[];
  tareas: Tarea[];
  recargar: () => void;
}

export function Proyectos({ proyectos, tareas, recargar }: Props) {
  const [abierto, setAbierto] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [parte, setParte] = useState<string | null>(null);
  const [carpetas, setCarpetas] = useState<CarpetaVault[]>([]);
  const [creando, setCreando] = useState(false);

  useEffect(() => {
    invoke<CarpetaVault[]>("carpetas_vault").then(setCarpetas).catch(() => setCarpetas([]));
  }, [proyectos]);

  async function hacer(fn: () => Promise<unknown>) {
    try {
      await fn();
      setError(null);
    } catch (e) {
      setError(String(e));
    }
    recargar();
  }

  return (
    <div className="proyectos">
      <header className="proyectos__barra">
        <p className="proyectos__pista">
          Un proyecto con su repo puesto se reconoce solo cuando trabajas dentro.
        </p>
        <button className="boton boton--fino" onClick={() => setCreando((v) => !v)}>
          {creando ? "Cancelar" : "Nuevo proyecto"}
        </button>
      </header>

      {creando && (
        <NuevoProyecto
          proyectos={proyectos}
          alCrear={() => {
            setCreando(false);
            recargar();
          }}
          alFallar={setError}
        />
      )}

      {error && (
        <p className="aviso" onClick={() => setError(null)}>
          {error}
        </p>
      )}
      {parte && (
        <p className="aviso aviso--bueno" onClick={() => setParte(null)}>
          {parte}
        </p>
      )}

      {proyectos.length === 0 ? (
        <p className="proyectos__vacio">
          Todavía no hay ninguno. Empieza por el de más arriba —tu empresa, tus
          clientes, lo tuyo— y cuelga lo demás de ahí.
        </p>
      ) : (
        <ul className="arbol">
          {proyectos.map((p) => (
            <Fila
              key={p.id}
              proyecto={p}
              proyectos={proyectos}
              carpetas={carpetas}
              tareas={tareas.filter((t) => t.project_id === p.id).length}
              abierto={abierto === p.id}
              alAbrir={() => setAbierto((a) => (a === p.id ? null : p.id))}
              hacer={hacer}
              alImportar={setParte}
              alFallar={setError}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

// -------------------------------------------------------------- una fila

function Fila({
  proyecto,
  proyectos,
  carpetas,
  tareas,
  abierto,
  alAbrir,
  hacer,
  alImportar,
  alFallar,
}: {
  proyecto: Proyecto;
  proyectos: Proyecto[];
  carpetas: CarpetaVault[];
  tareas: number;
  abierto: boolean;
  alAbrir: () => void;
  hacer: (fn: () => Promise<unknown>) => Promise<void>;
  alImportar: (parte: string) => void;
  alFallar: (e: string) => void;
}) {
  const [nombre, setNombre] = useState(proyecto.name);
  const [confirmando, setConfirmando] = useState<string | null>(null);

  // Los descendientes no pueden ser el padre de su propio ancestro: el núcleo
  // lo rechaza, pero ofrecerlos en la lista sería invitar al error.
  const descendientes = descendenciaDe(proyectos, proyecto.id);
  const posiblesPadres = proyectos.filter(
    (p) => p.id !== proyecto.id && !descendientes.has(p.id),
  );

  async function elegirRepo() {
    const elegida = await open({ directory: true, title: "La carpeta del repo" });
    if (typeof elegida === "string") {
      await hacer(() => invoke("fijar_repo", { id: proyecto.id, ruta: elegida }));
    }
  }

  async function importar() {
    try {
      const r = await invoke<Importacion>("importar_vault", { id: proyecto.id });
      const notas: string[] = [];
      if (r.estados_raros.length > 0) {
        notas.push(`${r.estados_raros.length} con un estado que no reconozco`);
      }
      if (r.sin_prioridad.length > 0) notas.push(`${r.sin_prioridad.length} sin prioridad`);
      if (r.prioridad_congelada.length > 0) {
        notas.push(`${r.prioridad_congelada.length} ya trabajadas: no les toqué la prioridad`);
      }
      alImportar(
        `${proyecto.name}: ${r.creadas} nueva(s), ${r.actualizadas} actualizada(s), ` +
          `${r.omitidas} ya hechas en el vault.` +
          (notas.length > 0 ? ` (${notas.join("; ")})` : ""),
      );
    } catch (e) {
      alFallar(String(e));
    }
  }

  return (
    <li className={`arbol__fila${proyecto.archived ? " arbol__fila--archivado" : ""}`}>
      <button
        className="arbol__cabeza"
        style={{ paddingLeft: `${10 + proyecto.nivel * 18}px` }}
        onClick={alAbrir}
      >
        <span className="arbol__nombre">{proyecto.name}</span>
        <span className="arbol__marcas">
          {proyecto.repo_path && <i className="marca marca--repo" title={proyecto.repo_path} />}
          {proyecto.vault_path && (
            <i className="marca marca--vault" title={`vault: ${proyecto.vault_path}`} />
          )}
        </span>
        <span className="arbol__cuenta">{tareas > 0 ? `${tareas} tarea(s)` : "—"}</span>
      </button>

      {abierto && (
        <div className="arbol__reverso">
          <label className="campo-fila">
            <span>Nombre</span>
            <input
              className="campo campo--ancho"
              value={nombre}
              onChange={(e) => setNombre(e.target.value)}
              onBlur={() =>
                nombre.trim() &&
                nombre !== proyecto.name &&
                hacer(() => invoke("renombrar_proyecto", { id: proyecto.id, nombre: nombre.trim() }))
              }
            />
          </label>

          <label className="campo-fila">
            <span>Cuelga de</span>
            <select
              className="selector campo--ancho"
              value={proyecto.parent_id ?? ""}
              onChange={(e) =>
                hacer(() =>
                  invoke("mover_proyecto", {
                    id: proyecto.id,
                    parentId: e.target.value === "" ? null : Number(e.target.value),
                  }),
                )
              }
            >
              <option value="">— la raíz —</option>
              {posiblesPadres.map((p) => (
                <option key={p.id} value={p.id}>
                  {"  ".repeat(p.nivel) + p.name}
                </option>
              ))}
            </select>
          </label>

          <label className="campo-fila">
            <span>Repo</span>
            <span className="campo-fila__valor">
              {proyecto.repo_path ?? <em>sin carpeta</em>}
            </span>
            <button className="boton boton--fino" onClick={elegirRepo}>
              Elegir…
            </button>
            {proyecto.repo_path && (
              <button
                className="boton boton--fino boton--tenue"
                onClick={() => hacer(() => invoke("fijar_repo", { id: proyecto.id, ruta: null }))}
              >
                Quitar
              </button>
            )}
          </label>

          <label className="campo-fila">
            <span>Vault</span>
            <select
              className="selector campo--ancho"
              value={proyecto.vault_path ?? ""}
              onChange={(e) =>
                hacer(() =>
                  invoke("fijar_vault", {
                    id: proyecto.id,
                    carpeta: e.target.value === "" ? null : e.target.value,
                  }),
                )
              }
            >
              <option value="">— sin ligar —</option>
              {carpetas.map((c) => (
                <option
                  key={c.nombre}
                  value={c.nombre}
                  // Una carpeta ya ligada a otro proyecto no se ofrece: dos
                  // proyectos importando el mismo backlog duplican el trabajo.
                  disabled={c.ligada_a !== null && c.nombre !== proyecto.vault_path}
                >
                  {c.nombre} · {c.vivas} pendiente(s)
                  {c.ligada_a && c.nombre !== proyecto.vault_path ? ` — ya en ${c.ligada_a}` : ""}
                </option>
              ))}
            </select>
            {proyecto.vault_path && (
              <button className="boton boton--fino" onClick={importar}>
                Importar
              </button>
            )}
          </label>

          <div className="arbol__acciones">
            <button
              className="boton boton--fino"
              onClick={() =>
                hacer(() =>
                  invoke("archivar_proyecto", {
                    id: proyecto.id,
                    archivado: !proyecto.archived,
                  }),
                )
              }
            >
              {proyecto.archived ? "Devolver a la vista" : "Archivar"}
            </button>

            {confirmando === null ? (
              <button
                className="boton boton--fino boton--tenue"
                onClick={async () => {
                  const [hijos, tareas] = await invoke<[number, number]>("contenido_proyecto", {
                    id: proyecto.id,
                  });
                  setConfirmando(
                    hijos + tareas === 0
                      ? "vacio"
                      : `${hijos} subproyecto(s) y ${tareas} tarea(s), con su tiempo medido`,
                  );
                }}
              >
                Borrar
              </button>
            ) : (
              <span className="arbol__confirmar">
                {confirmando === "vacio" ? (
                  <>Está vacío. ¿Seguro?</>
                ) : (
                  <>Se lleva {confirmando}. Esto no se deshace.</>
                )}
                <button
                  className="boton boton--fino boton--peligro"
                  onClick={() =>
                    hacer(() =>
                      invoke("borrar_proyecto", {
                        id: proyecto.id,
                        force: confirmando !== "vacio",
                      }),
                    )
                  }
                >
                  Borrar
                </button>
                <button className="boton boton--fino" onClick={() => setConfirmando(null)}>
                  No
                </button>
              </span>
            )}
          </div>
        </div>
      )}
    </li>
  );
}

// ------------------------------------------------------------------ alta

function NuevoProyecto({
  proyectos,
  alCrear,
  alFallar,
}: {
  proyectos: Proyecto[];
  alCrear: () => void;
  alFallar: (e: string) => void;
}) {
  const [nombre, setNombre] = useState("");
  const [padre, setPadre] = useState<string>("");

  async function crear(e: React.FormEvent) {
    e.preventDefault();
    if (!nombre.trim()) return;
    try {
      await invoke("crear_proyecto", {
        nombre: nombre.trim(),
        parentId: padre === "" ? null : Number(padre),
      });
      alCrear();
    } catch (err) {
      alFallar(String(err));
    }
  }

  return (
    <form className="compositor" onSubmit={crear}>
      <input
        className="campo campo--ancho"
        placeholder="Nombre del proyecto"
        value={nombre}
        onChange={(e) => setNombre(e.target.value)}
        autoFocus
      />
      <select className="selector" value={padre} onChange={(e) => setPadre(e.target.value)}>
        <option value="">— en la raíz —</option>
        {proyectos.map((p) => (
          <option key={p.id} value={p.id}>
            {"  ".repeat(p.nivel) + p.name}
          </option>
        ))}
      </select>
      <button className="boton boton--principal boton--fino" type="submit">
        Crear
      </button>
    </form>
  );
}

/** Los ids que cuelgan de uno, a cualquier profundidad. */
function descendenciaDe(proyectos: Proyecto[], id: number): Set<number> {
  const dentro = new Set<number>();
  let creció = true;
  while (creció) {
    creció = false;
    for (const p of proyectos) {
      if (p.parent_id !== null && (p.parent_id === id || dentro.has(p.parent_id)) && !dentro.has(p.id)) {
        dentro.add(p.id);
        creció = true;
      }
    }
  }
  return dentro;
}
