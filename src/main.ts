import { api, type InputView, type QuickScene, type Settings, type Snapshot } from "./api";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

/** Todo texto que viene de OBS o de los ajustes pasa por aquí antes de ir a innerHTML. */
const esc = (s: string) =>
  s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

let snap: Snapshot | null = null;
let confirmingStop = false;
let confirmTimer: number | undefined;
let toastTimer: number | undefined;

// Ajustes: se edita una copia local y solo se guarda al pulsar «Guardar».
let draft: Settings | null = null;
let inputs: InputView[] = [];

function toast(message: string) {
  const el = $("toast");
  el.textContent = message;
  el.hidden = false;
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (el.hidden = true), 6000);
}

/** Ejecuta una acción y muestra el error, si lo hay, sin ventanas emergentes (RNF-6). */
async function run(action: Promise<unknown>) {
  try {
    await action;
  } catch (e) {
    toast(String(e));
  }
}

// ---------- Vista en directo ----------

const connLabels = {
  connected: "Conectado a OBS",
  connecting: "Conectando con OBS…",
  reconnecting: "Esperando a OBS…",
  disconnected: "Desconectado",
} as const;

function renderHeader(s: Snapshot) {
  const { state, error } = s.connection;
  const pill = $("connection");
  pill.className = `pill ${state}`;
  pill.innerHTML = esc(connLabels[state]) + (error && state !== "connected" ? `<small>${esc(error)}</small>` : "");

  const connected = state === "connected";
  const box = $("stream-control");
  if (s.stream.active) {
    box.innerHTML = confirmingStop
      ? `<button class="danger" data-action="stop-stream">Confirmar: parar el directo</button>`
      : `<button class="on" data-action="ask-stop">${s.stream.reconnecting ? "Reconectando… " : "● En directo · "}Parar</button>`;
  } else {
    box.innerHTML = `<button class="live" data-action="start-stream" ${connected ? "" : "disabled"}>Iniciar directo</button>`;
  }
  const mic = $<HTMLButtonElement>("mic-button");
  mic.disabled = !connected || s.micMuted === null;
  mic.className = s.micMuted ? "on" : "";
  mic.textContent = s.micMuted === null ? "Micro (sin elegir)" : s.micMuted ? "🔇 Micro silenciado" : "🎙 Micro activo";
  $<HTMLButtonElement>("back-button").disabled = !connected || !s.previousScene;
}

function renderQuick(s: Snapshot) {
  const connected = s.connection.state === "connected";
  const items = s.settings.quickScenes
    .map((q) => {
      const missing = s.scenes.length > 0 && !s.scenes.includes(q.scene);
      const active = s.currentScene === q.scene;
      const note = missing ? "no existe en OBS" : q.muteMic ? "silencia el micro" : "";
      return `<button class="big ${active ? "active" : ""}" style="--c:${esc(q.color)}" data-action="launch" data-scene="${esc(q.scene)}" ${connected && !missing ? "" : "disabled"}>
        <span class="icon">${esc(q.icon)}</span><span>${esc(q.label || q.scene)}</span>${note ? `<small>${note}</small>` : ""}
      </button>`;
    })
    .join("");
  $("quick").innerHTML =
    items || `<p class="empty">Aún no hay escenas rápidas. Márcalas con ☆ en «Todas las escenas» o créalas en Ajustes.</p>`;
}

function renderCameras(s: Snapshot) {
  const connected = s.connection.state === "connected";
  const items = s.cameras
    .map(
      (c) => `<button class="big cam ${c.active ? "active" : ""}" data-action="camera" data-id="${esc(c.id)}" ${connected && c.inScene ? "" : "disabled"}>
        <span class="icon">📷</span><span>${esc(c.name)}</span><small>${c.inScene ? (c.active ? "en pantalla" : "disponible") : "no está en esta escena"}</small>
      </button>`,
    )
    .join("");
  $("cameras").innerHTML = items || `<p class="empty">Añade tus cámaras (fuentes de OBS) en Ajustes.</p>`;
}

function renderScenes(s: Snapshot) {
  const connected = s.connection.state === "connected";
  const query = $<HTMLInputElement>("search").value.trim().toLowerCase();
  const quick = new Set(s.settings.quickScenes.map((q) => q.scene));
  const rows = s.scenes
    .filter((name) => name.toLowerCase().includes(query))
    .map(
      (name) => `<div class="scene-row">
        <button class="name ${s.currentScene === name ? "active" : ""}" data-action="launch" data-scene="${esc(name)}" ${connected ? "" : "disabled"}>${esc(name)}</button>
        <button class="star ${quick.has(name) ? "on" : ""}" data-action="toggle-quick" data-scene="${esc(name)}" title="${quick.has(name) ? "Quitar de escenas rápidas" : "Marcar como escena rápida"}">${quick.has(name) ? "★" : "☆"}</button>
      </div>`,
    )
    .join("");
  $("scenes").innerHTML = rows || `<p class="empty">${s.scenes.length ? "Ninguna escena coincide." : "OBS no ha enviado escenas todavía."}</p>`;
}

function render() {
  if (!snap) return;
  renderHeader(snap);
  renderQuick(snap);
  renderCameras(snap);
  renderScenes(snap);
}

function toggleQuick(scene: string) {
  if (!snap) return;
  const settings = structuredClone(snap.settings);
  const at = settings.quickScenes.findIndex((q) => q.scene === scene);
  if (at >= 0) settings.quickScenes.splice(at, 1);
  else settings.quickScenes.push({ scene, label: scene, color: "#3b82f6", icon: "▶", muteMic: false });
  return run(api.saveSettings(settings));
}

// ---------- Ajustes ----------

function field(label: string, control: string) {
  return `<label>${label}${control}</label>`;
}

function renderSettings() {
  if (!draft || !snap) return;
  const d = draft;
  const inputOptions = (selected: string | null) =>
    `<option value="">(ninguno)</option>` +
    inputs.map((i) => `<option value="${esc(i.name)}" ${i.name === selected ? "selected" : ""}>${esc(i.name)}</option>`).join("") +
    (selected && !inputs.some((i) => i.name === selected) ? `<option value="${esc(selected)}" selected>${esc(selected)}</option>` : "");

  const sourceList = `<datalist id="sources">${inputs.map((i) => `<option value="${esc(i.name)}">`).join("")}</datalist>`;
  const sceneList = `<datalist id="scene-names">${snap.scenes.map((n) => `<option value="${esc(n)}">`).join("")}</datalist>`;

  $("settings-form").innerHTML = `<div class="form">
    <div class="card">
      <h3>Conexión con OBS</h3>
      <p>En OBS: Herramientas → Ajustes del servidor WebSocket (OBS 28 o superior). Puerto por defecto: 4455.</p>
      <div class="row">
        ${field("Dirección", `<input type="text" data-bind="host" value="${esc(d.obs.host)}" />`)}
        ${field("Puerto", `<input type="number" min="1" max="65535" data-bind="port" value="${d.obs.port}" />`)}
        ${field(
          "Contraseña",
          `<input type="password" id="password" autocomplete="off" placeholder="${snap.hasPassword ? "Guardada en el sistema" : "Sin contraseña"}" />`,
        )}
        <button data-action="save-password">Guardar contraseña</button>
        ${snap.hasPassword ? `<button data-action="clear-password" class="ghost">Borrar</button>` : ""}
      </div>
      <p>La contraseña se guarda en el almacén seguro del sistema, nunca en el archivo de ajustes.</p>
    </div>

    <div class="card">
      <h3>Micro</h3>
      <div class="row">
        ${field("Entrada de audio de OBS", `<select data-bind="micInput">${inputOptions(d.micInput)}</select>`)}
        <button data-action="refresh-inputs" class="ghost">Recargar lista de OBS</button>
      </div>
    </div>

    <div class="card">
      <h3>Cámaras</h3>
      <p>Cada cámara es una fuente de OBS dentro de la escena. Al cambiar, se muestra la nueva y después se oculta la anterior.</p>
      ${d.cameras
        .map(
          (c, i) => `<div class="row">
            ${field("Nombre", `<input type="text" data-cam="${i}" data-key="name" value="${esc(c.name)}" placeholder="Móvil" />`)}
            ${field("Fuente en OBS", `<input type="text" list="sources" data-cam="${i}" data-key="source" value="${esc(c.source)}" />`)}
            <button class="ghost" data-action="remove-camera" data-index="${i}">Quitar</button>
          </div>`,
        )
        .join("")}
      <div><button data-action="add-camera">+ Añadir cámara</button></div>
      ${sourceList}
    </div>

    <div class="card">
      <h3>Escenas rápidas</h3>
      <p>Referencias por nombre a escenas que ya existen en OBS. «Silenciar micro» lo silencia al entrar y lo reactiva al salir.</p>
      ${d.quickScenes
        .map(
          (q, i) => `<div class="row">
            ${field("Escena de OBS", `<input type="text" list="scene-names" data-quick="${i}" data-key="scene" value="${esc(q.scene)}" />`)}
            ${field("Nombre corto", `<input type="text" data-quick="${i}" data-key="label" value="${esc(q.label)}" />`)}
            ${field("Icono", `<input type="text" size="3" data-quick="${i}" data-key="icon" value="${esc(q.icon)}" />`)}
            ${field("Color", `<input type="color" data-quick="${i}" data-key="color" value="${esc(q.color)}" />`)}
            <label class="inline"><input type="checkbox" data-quick="${i}" data-key="muteMic" ${q.muteMic ? "checked" : ""} /> Silenciar micro</label>
            <button class="ghost" data-action="remove-quick" data-index="${i}">Quitar</button>
          </div>`,
        )
        .join("")}
      <div><button data-action="add-quick">+ Añadir escena rápida</button></div>
      ${sceneList}
    </div>
  </div>`;
}

async function openSettings() {
  if (!snap) return;
  draft = structuredClone(snap.settings);
  inputs = [];
  $("live").hidden = true;
  $("settings").hidden = false;
  renderSettings();
  await loadInputs(false);
}

async function loadInputs(report: boolean) {
  try {
    inputs = await api.listInputs();
    renderSettings();
  } catch (e) {
    if (report) toast(String(e));
  }
}

function closeSettings() {
  draft = null;
  $("settings").hidden = true;
  $("live").hidden = false;
  render();
}

function onSettingsInput(target: HTMLInputElement | HTMLSelectElement) {
  if (!draft) return;
  const d = draft;
  const bind = target.dataset.bind;
  if (bind === "host") d.obs.host = target.value;
  else if (bind === "port") d.obs.port = Number(target.value);
  else if (bind === "micInput") d.micInput = target.value || null;

  const key = target.dataset.key;
  if (!key) return;
  const value = target instanceof HTMLInputElement && target.type === "checkbox" ? target.checked : target.value;
  if (target.dataset.cam !== undefined) {
    (d.cameras[Number(target.dataset.cam)] as unknown as Record<string, unknown>)[key] = value;
  } else if (target.dataset.quick !== undefined) {
    (d.quickScenes[Number(target.dataset.quick)] as unknown as Record<string, unknown>)[key] = value;
  }
}

// ---------- Eventos ----------

document.addEventListener("input", (e) => {
  const t = e.target as HTMLElement;
  if (t.id === "search") return snap && renderScenes(snap);
  if (t instanceof HTMLInputElement || t instanceof HTMLSelectElement) onSettingsInput(t);
});

document.addEventListener("click", (e) => {
  const el = (e.target as HTMLElement).closest<HTMLElement>("[data-action]");
  if (!el) return;
  const { action, scene, id } = el.dataset;
  const index = Number(el.dataset.index);

  switch (action) {
    case "launch": return void run(api.launchScene(scene!));
    case "back": return void run(api.goBack());
    case "camera": return void run(api.switchCamera(id!));
    case "mic": return void run(api.toggleMic());
    case "toggle-quick": return void toggleQuick(scene!);
    case "start-stream": return void run(api.startStream());
    case "ask-stop":
      // Confirmación en línea, sin ventana emergente que tape la pantalla (RF-26, RNF-6).
      confirmingStop = true;
      window.clearTimeout(confirmTimer);
      confirmTimer = window.setTimeout(() => { confirmingStop = false; if (snap) renderHeader(snap); }, 4000);
      return snap && renderHeader(snap);
    case "stop-stream":
      confirmingStop = false;
      window.clearTimeout(confirmTimer);
      return void run(api.stopStream());
    case "open-settings": return void openSettings();
    case "close-settings": return closeSettings();
    case "refresh-inputs": return void loadInputs(true);
    case "add-camera": draft?.cameras.push({ id: "", name: "", source: "" }); return renderSettings();
    case "remove-camera": draft?.cameras.splice(index, 1); return renderSettings();
    case "add-quick": draft?.quickScenes.push({ scene: "", label: "", color: "#3b82f6", icon: "▶", muteMic: false } satisfies QuickScene); return renderSettings();
    case "remove-quick": draft?.quickScenes.splice(index, 1); return renderSettings();
    case "save-password": {
      const input = $<HTMLInputElement>("password");
      if (!input.value) return toast("Escribe la contraseña antes de guardarla");
      const value = input.value;
      input.value = "";
      return void run(api.setObsPassword(value).then(renderSettings));
    }
    case "clear-password": return void run(api.setObsPassword(null).then(renderSettings));
    case "save-settings":
      if (!draft) return;
      return void run(api.saveSettings(draft).then(closeSettings));
  }
});

function apply(s: Snapshot) {
  snap = s;
  if (draft) refreshSettingsIfPasswordChanged(s);
  render();
}

// Si cambia «hay contraseña» mientras se editan ajustes, solo se refresca el aviso, no los campos.
let lastHasPassword: boolean | null = null;
function refreshSettingsIfPasswordChanged(s: Snapshot) {
  if (lastHasPassword !== null && lastHasPassword !== s.hasPassword) renderSettings();
  lastHasPassword = s.hasPassword;
}

async function main() {
  await api.onState(apply);
  apply(await api.getSnapshot());
}

main().catch((e) => toast(String(e)));
