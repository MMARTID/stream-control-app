// Pestaña «Ajustes»: conexión con OBS, micro, cámaras y escenas rápidas.
// Se edita una copia y solo se guarda al pulsar «Guardar cambios». El panel web y el avatar
// tienen sus propias pestañas y se guardan al instante.
import { api, type CameraConfig, type InputView, type QuickScene, type Settings } from "../api";
import { connected, state } from "../state";
import { $, esc, run, toast } from "../util";

interface Draft {
  obs: Settings["obs"];
  micInput: string | null;
  quickScenes: QuickScene[];
  cameras: CameraConfig[];
}

let draft: Draft | null = null;
let baseline = "";
let inputs: InputView[] = [];
let root: HTMLElement;

const pick = (s: Settings): Draft => structuredClone({ obs: s.obs, micInput: s.micInput, quickScenes: s.quickScenes, cameras: s.cameras });
const dirty = () => !!draft && JSON.stringify(draft) !== baseline;

export function mountSettings(el: HTMLElement) {
  root = el;
  root.addEventListener("input", onInput);
  root.addEventListener("change", onInput);
  root.addEventListener("click", onClick);
}

/** Se llama cada vez que cambia el estado. Mientras hay cambios sin guardar no se pisan los campos. */
export function updateSettings() {
  const s = state.snap;
  if (!s) return;
  if (!draft || (!dirty() && JSON.stringify(pick(s.settings)) !== baseline)) {
    draft = pick(s.settings);
    baseline = JSON.stringify(draft);
    render();
  } else {
    renderConnectionStatus();
    renderBar();
  }
}

/** Al entrar en la pestaña se descartan los cambios no guardados y se recarga la lista de entradas de OBS. */
export function enterSettings() {
  const s = state.snap;
  if (!s) return;
  draft = pick(s.settings);
  baseline = JSON.stringify(draft);
  render();
  void loadInputs(false);
}

async function loadInputs(report: boolean) {
  try {
    inputs = await api.listInputs();
    render();
  } catch (e) {
    if (report) toast(String(e));
  }
}

function field(label: string, control: string, hint = "") {
  return `<label class="field"><span>${label}</span>${control}${hint ? `<small>${hint}</small>` : ""}</label>`;
}

function render() {
  const s = state.snap;
  if (!draft || !s) return;
  const d = draft;
  const micOptions =
    `<option value="">(ninguno)</option>` +
    inputs.map((i) => `<option value="${esc(i.name)}" ${i.name === d.micInput ? "selected" : ""}>${esc(i.name)}</option>`).join("") +
    (d.micInput && !inputs.some((i) => i.name === d.micInput) ? `<option value="${esc(d.micInput)}" selected>${esc(d.micInput)}</option>` : "");
  const sourceList = `<datalist id="sources">${inputs.map((i) => `<option value="${esc(i.name)}">`).join("")}</datalist>`;
  const sceneList = `<datalist id="scene-names">${s.scenes.map((n) => `<option value="${esc(n)}">`).join("")}</datalist>`;

  root.innerHTML = `
    <div class="page-head"><h1>Ajustes</h1><p>Lo básico para empezar. El panel para el móvil y el avatar tienen su propia pestaña.</p></div>

    <section class="card">
      <h3><span class="num">1</span> Conexión con OBS <span id="conn-chip"></span></h3>
      <p>En OBS: <b>Herramientas → Ajustes del servidor WebSocket</b> → «Habilitar servidor WebSocket». La app se conecta sola cuando OBS está abierto.</p>
      <div class="row">
        ${field("Dirección", `<input type="text" data-bind="host" value="${esc(d.obs.host)}" />`, "Casi siempre 127.0.0.1")}
        ${field("Puerto", `<input type="number" min="1" max="65535" data-bind="port" value="${d.obs.port}" />`, "Por defecto 4455")}
        ${field("Contraseña", `<input type="password" id="obs-password" autocomplete="off" placeholder="${s.hasPassword ? "Guardada en el sistema" : "Sin contraseña"}" />`, "Se guarda en el almacén seguro de Windows, nunca en un archivo")}
      </div>
      <div class="row">
        <button data-action="save-password">Guardar contraseña</button>
        ${s.hasPassword ? `<button data-action="clear-password" class="ghost">Borrar contraseña</button>` : ""}
      </div>
    </section>

    <section class="card">
      <h3><span class="num">2</span> Micrófono</h3>
      <p>La entrada de audio de OBS que se silencia con el botón de micro y al entrar en escenas como «Pausa / baño».</p>
      <div class="row">
        ${field("Entrada de audio de OBS", `<select data-bind="micInput">${micOptions}</select>`)}
        <button class="ghost" data-action="refresh-inputs">Recargar lista de OBS</button>
      </div>
      ${!connected() ? `<p class="hint">Conecta con OBS para ver sus entradas de audio.</p>` : ""}
    </section>

    <section class="card">
      <h3><span class="num">3</span> Cámaras</h3>
      <p>Cada cámara es una fuente de OBS dentro de la escena. Al cambiar, se muestra la nueva y después se oculta la anterior, así nunca hay un instante sin imagen.</p>
      ${d.cameras
        .map(
          (c, i) => `<div class="row item">
            ${field("Nombre que verás", `<input type="text" data-cam="${i}" data-key="name" value="${esc(c.name)}" placeholder="Móvil" />`)}
            ${field("Fuente en OBS", `<input type="text" list="sources" data-cam="${i}" data-key="source" value="${esc(c.source)}" placeholder="Elige o escribe el nombre" />`)}
            <button class="ghost" data-action="remove-camera" data-index="${i}" aria-label="Quitar cámara">Quitar</button>
          </div>`,
        )
        .join("")}
      <div><button data-action="add-camera">+ Añadir cámara</button></div>
      ${sourceList}
    </section>

    <section class="card">
      <h3><span class="num">4</span> Escenas rápidas</h3>
      <p>Referencias a escenas que ya existen en OBS (no se copian). «Silenciar micro» lo silencia al entrar y lo reactiva al salir.</p>
      ${d.quickScenes
        .map(
          (q, i) => `<div class="row item">
            ${field("Escena de OBS", `<input type="text" list="scene-names" data-quick="${i}" data-key="scene" value="${esc(q.scene)}" />`)}
            ${field("Nombre del botón", `<input type="text" data-quick="${i}" data-key="label" value="${esc(q.label)}" />`)}
            ${field("Icono", `<input type="text" size="3" data-quick="${i}" data-key="icon" value="${esc(q.icon)}" />`)}
            ${field("Color", `<input type="color" data-quick="${i}" data-key="color" value="${esc(q.color)}" />`)}
            <label class="check-inline"><input type="checkbox" data-quick="${i}" data-key="muteMic" ${q.muteMic ? "checked" : ""} /> Silenciar micro</label>
            <button class="ghost" data-action="remove-quick" data-index="${i}" aria-label="Quitar escena rápida">Quitar</button>
          </div>`,
        )
        .join("")}
      <div><button data-action="add-quick">+ Añadir escena rápida</button></div>
      ${sceneList}
    </section>

    <div id="save-bar" class="save-bar" hidden>
      <span>Hay cambios sin guardar</span>
      <button class="ghost" data-action="discard">Descartar</button>
      <button class="primary" data-action="save">Guardar cambios</button>
    </div>`;
  renderConnectionStatus();
  renderBar();
}

function renderConnectionStatus() {
  const chip = document.getElementById("conn-chip");
  const s = state.snap;
  if (!chip || !s) return;
  const labels = { connected: ["ok", "Conectado"], connecting: ["wait", "Conectando…"], reconnecting: ["wait", "Esperando a OBS…"], disconnected: ["bad", "Sin conexión"] } as const;
  const [kind, text] = labels[s.connection.state];
  chip.innerHTML = `<span class="chip ${kind}">${text}</span>${s.connection.error && s.connection.state !== "connected" ? ` <small class="err">${esc(s.connection.error)}</small>` : ""}`;
}

function renderBar() {
  const bar = document.getElementById("save-bar");
  if (bar) bar.hidden = !dirty();
}

function onInput(e: Event) {
  const t = e.target as HTMLInputElement | HTMLSelectElement;
  if (!draft || !(t instanceof HTMLInputElement || t instanceof HTMLSelectElement)) return;
  const bind = t.dataset.bind;
  if (bind === "host") draft.obs.host = t.value;
  else if (bind === "port") draft.obs.port = Number(t.value);
  else if (bind === "micInput") draft.micInput = t.value || null;
  const key = t.dataset.key;
  if (key) {
    const value = t instanceof HTMLInputElement && t.type === "checkbox" ? t.checked : t.value;
    if (t.dataset.cam !== undefined) (draft.cameras[Number(t.dataset.cam)] as unknown as Record<string, unknown>)[key] = value;
    else if (t.dataset.quick !== undefined) (draft.quickScenes[Number(t.dataset.quick)] as unknown as Record<string, unknown>)[key] = value;
  }
  renderBar();
}

async function onClick(e: Event) {
  const el = (e.target as HTMLElement).closest<HTMLElement>("[data-action]");
  if (!el || !draft) return;
  const index = Number(el.dataset.index);
  switch (el.dataset.action) {
    case "add-camera": draft.cameras.push({ id: "", name: "", source: "" }); return render();
    case "remove-camera": draft.cameras.splice(index, 1); return render();
    case "add-quick": draft.quickScenes.push({ scene: "", label: "", color: "#3b82f6", icon: "▶", muteMic: false }); return render();
    case "remove-quick": draft.quickScenes.splice(index, 1); return render();
    case "refresh-inputs": return void loadInputs(true);
    case "discard": draft = JSON.parse(baseline); return render();
    case "save": return void save();
    case "save-password": {
      const input = $<HTMLInputElement>("obs-password");
      if (!input.value) return toast("Escribe la contraseña antes de guardarla");
      const value = input.value;
      input.value = "";
      return void run(api.setObsPassword(value).then(() => toast("Contraseña de OBS guardada", "ok")));
    }
    case "clear-password": return void run(api.setObsPassword(null).then(() => toast("Contraseña de OBS borrada", "ok")));
  }
}

async function save() {
  const s = state.snap;
  if (!draft || !s) return;
  // Se mezcla sobre los ajustes más recientes: el panel y el avatar pueden haber cambiado mientras tanto.
  const next: Settings = { ...structuredClone(s.settings), obs: draft.obs, micInput: draft.micInput, quickScenes: draft.quickScenes, cameras: draft.cameras };
  const ok = await run(api.saveSettings(next).then(() => true));
  if (ok) {
    baseline = JSON.stringify(draft);
    toast("Ajustes guardados", "ok");
    renderBar();
  }
}
