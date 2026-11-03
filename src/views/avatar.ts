// Pestaña «Avatar»: un personaje que mueve la boca (y la cara) como tú. Se ve en OBS como fuente de navegador.
import { api, type AvatarStatus } from "../api";
import { AvatarTracker, askPermission, listDevices, type TrackerMode, type TrackerState } from "../avatar/tracker";
import { connected, notify, state } from "../state";
import { updateSettings } from "../ops";
import { $, esc, run, store, toast } from "../util";

const CHARACTERS = [
  { id: "jason", name: "Jason", text: "Pelo corto, barba cuidada y camisa estampada. Aire de Vice City." },
  { id: "cal", name: "Cal", text: "Sombrero de pescador, barbón y tatuajes. El colega de siempre." },
] as const;

interface Prefs {
  mode: TrackerMode;
  cameraId: string;
  micId: string;
  sensitivity: number;
  mirror: boolean;
  invertPitch: boolean;
  autostart: boolean;
}
const prefs = store<Prefs>("sc_avatar_prefs", { mode: "camera", cameraId: "", micId: "", sensitivity: 1, mirror: true, invertPitch: false, autostart: false });
let current: Prefs = prefs.get();

const tracker = new AvatarTracker(
  (frame) => void api.avatarPush(frame).catch(() => {}),
  (st) => {
    state.tracker = st;
    notify();
  },
);

let cameras: { id: string; label: string }[] = [];
let mics: { id: string; label: string }[] = [];
let root: HTMLElement;
const signatures = new Map<string, string>();
let autostarted = false;

/** Repinta un bloque solo si lo que lo define ha cambiado: así no se pierde el foco ni se corta un deslizador. */
function section(id: string, signature: unknown, html: () => string) {
  const sig = JSON.stringify(signature);
  if (signatures.get(id) === sig) return;
  signatures.set(id, sig);
  $(id).innerHTML = html();
}

export function mountAvatar(el: HTMLElement) {
  root = el;
  root.innerHTML = `
    <div class="page-head"><h1>Avatar</h1>
      <p>Un personaje que mueve la boca cuando hablas y copia tu cara. Se muestra en OBS como una fuente más, con fondo transparente. Todo se calcula en tu PC.</p></div>
    <div class="avatar-grid">
      <section class="card preview-card">
        <h3>Vista previa <small>Así lo verá OBS</small></h3>
        <div class="avatar-stage" id="a-stage"></div>
      </section>
      <div class="steps-col">
        <section class="card" id="a-switch"></section>
        <section class="card" id="a-chars"></section>
        <section class="card" id="a-track"></section>
        <section class="card" id="a-obs"></section>
      </div>
    </div>`;

  root.addEventListener("click", (e) => {
    const el = (e.target as HTMLElement).closest<HTMLElement>("[data-action]");
    if (!el) return;
    switch (el.dataset.action) {
      case "character": return void updateSettings((s) => (s.avatar.character = el.dataset.id!));
      case "mode": return setPrefs({ mode: el.dataset.mode as TrackerMode }, true);
      case "find-devices": return void findDevices();
      case "start": return void startTracking();
      case "stop": return void stopTracking();
      case "calibrate": tracker.calibrate(); return toast("Calibrado: ahora «mirar al frente» es tu posición actual", "ok");
      case "copy": return void copy(el.dataset.text!);
      case "create-source": return void createSource();
    }
  });
  root.addEventListener("change", (e) => {
    const t = e.target as HTMLInputElement | HTMLSelectElement;
    switch (t.dataset.bind) {
      case "avatar-enabled": return void toggleEnabled((t as HTMLInputElement).checked);
      case "camera": return setPrefs({ cameraId: t.value }, true);
      case "mic": return setPrefs({ micId: t.value }, true);
      case "mirror": return setPrefs({ mirror: (t as HTMLInputElement).checked });
      case "invert-pitch": return setPrefs({ invertPitch: (t as HTMLInputElement).checked });
      case "autostart": return setPrefs({ autostart: (t as HTMLInputElement).checked });
    }
  });
  root.addEventListener("input", (e) => {
    const t = e.target as HTMLInputElement;
    if (t.dataset.bind === "sensitivity") setPrefs({ sensitivity: Number(t.value) });
  });
}

function setPrefs(patch: Partial<Prefs>, restart = false) {
  const changedDevice = patch.cameraId !== undefined || patch.micId !== undefined || patch.mode !== undefined;
  current = { ...current, ...patch };
  prefs.set(current);
  tracker.setOptions({ sensitivity: current.sensitivity, mirror: current.mirror, invertPitch: current.invertPitch });
  if (restart && changedDevice && tracker.active) void startTracking();
  else updateAvatar(true);
}

async function toggleEnabled(on: boolean) {
  if (!on) await tracker.stop();
  await updateSettings((s) => (s.avatar.enabled = on));
}

async function findDevices() {
  try {
    await askPermission(current.mode === "camera" ? "video" : "audio");
    [cameras, mics] = await Promise.all([listDevices("videoinput"), listDevices("audioinput")]);
    signatures.delete("a-track");
    updateAvatar();
  } catch (e) {
    toast(e instanceof DOMException && e.name === "NotAllowedError" ? "No hay permiso para usar el dispositivo. Permítelo en Windows (Privacidad) y en el aviso de la app." : String(e));
  }
}

async function startTracking() {
  tracker.setOptions({ sensitivity: current.sensitivity, mirror: current.mirror, invertPitch: current.invertPitch });
  await tracker.start({
    mode: current.mode,
    deviceId: current.mode === "camera" ? current.cameraId : current.micId,
    options: { sensitivity: current.sensitivity, mirror: current.mirror, invertPitch: current.invertPitch },
  });
  // Una vez concedido el permiso, los nombres de los dispositivos ya se pueden leer.
  if (tracker.state.kind !== "error") {
    [cameras, mics] = await Promise.all([listDevices("videoinput"), listDevices("audioinput")]).catch(() => [cameras, mics]);
    signatures.delete("a-track");
    updateAvatar();
  }
}

async function stopTracking() {
  await tracker.stop();
}

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast("Dirección copiada", "ok");
  } catch {
    toast("No se pudo copiar. Selecciónala y pulsa Ctrl+C");
  }
}

async function createSource() {
  const scene = await run(api.createAvatarSource());
  if (scene) toast(`Añadida la fuente «Avatar (Stream Control)» a la escena «${scene}». Colócala y ajusta su tamaño en OBS.`, "ok");
}

const trackerText = (st: TrackerState, mode: TrackerMode): { text: string; kind: string } => {
  switch (st.kind) {
    case "off": return { text: "Parado: el avatar se mueve solo (respira y parpadea).", kind: "" };
    case "starting": return { text: st.detail, kind: "wait" };
    case "tracking": return { text: mode === "camera" ? "Siguiendo tu cara ✓" : "Escuchando el micrófono ✓", kind: "ok" };
    case "no-face": return { text: "No veo tu cara. Colócate frente a la cámara con buena luz.", kind: "wait" };
    case "error": return { text: st.message, kind: "bad" };
  }
};

export function updateAvatar(force = false) {
  const s = state.snap;
  const a: AvatarStatus | null = state.avatar;
  if (!s || !a) return;
  if (force) signatures.clear();
  const cfg = s.settings.avatar;
  const running = a.running;

  // Vista previa: el iframe solo se crea cuando cambia la dirección y la pestaña está a la vista.
  const stage = $("a-stage");
  const wantPreview = running && state.tab === "avatar";
  const src = `${a.url}?preview=1`;
  const iframe = stage.querySelector("iframe");
  if (wantPreview && (!iframe || iframe.getAttribute("src") !== src)) {
    stage.innerHTML = `<iframe src="${esc(src)}" title="Vista previa del avatar" allowtransparency="true"></iframe>`;
  } else if (!wantPreview && !running) {
    stage.innerHTML = `<p class="hint">${a.error ? esc(a.error) : "Activa el avatar para ver aquí la vista previa."}</p>`;
  }

  section("a-switch", { on: cfg.enabled, running, error: a.error }, () => `
    <label class="switch big-switch"><input type="checkbox" data-bind="avatar-enabled" ${cfg.enabled ? "checked" : ""} /><span class="track"></span><b>Activar el avatar</b></label>
    <p class="hint">${cfg.enabled ? (running ? "Activo." : a.error ? "" : "Arrancando…") : "Desactivado."}</p>
    ${a.error ? `<p class="err">${esc(a.error)}</p>` : ""}`);

  section("a-chars", { c: cfg.character, running, url: a.url }, () => `
    <h3><span class="num">1</span> Elige tu personaje</h3>
    <div class="char-grid">${CHARACTERS.map(
      (c) => `<button class="char ${c.id === cfg.character ? "selected" : ""}" data-action="character" data-id="${c.id}" ${cfg.enabled ? "" : "disabled"}>
        <span class="char-img">${running ? `<img src="${esc(a.url.replace(/\/avatar$/, ""))}/avatars/${c.id}.svg" alt="${c.name}" />` : "🎭"}</span>
        <b>${c.name}</b><small>${c.text}</small></button>`,
    ).join("")}</div>
    <p class="hint">Dibujos propios, inspirados en el aspecto de personajes de GTA VI. No son arte oficial.</p>`);

  const live = tracker.active;
  const t = trackerText(state.tracker, current.mode);
  section("a-track", { mode: current.mode, cameras, mics, st: state.tracker, live, enabled: cfg.enabled, cam: current.cameraId, mic: current.micId, mirror: current.mirror, inv: current.invertPitch, auto: current.autostart }, () => {
    const devices = current.mode === "camera" ? cameras : mics;
    const selected = current.mode === "camera" ? current.cameraId : current.micId;
    return `
    <h3><span class="num">2</span> Que se mueva como tú</h3>
    <div class="segmented" role="group" aria-label="Qué seguir">
      <button data-action="mode" data-mode="camera" class="${current.mode === "camera" ? "on" : ""}">📷 Mi cara (cámara)</button>
      <button data-action="mode" data-mode="voice" class="${current.mode === "voice" ? "on" : ""}">🎙 Mi voz (micrófono)</button>
    </div>
    <p class="hint">${
      current.mode === "camera"
        ? "Sigue la boca, los ojos, las cejas y la cabeza. Necesita una cámara <b>que OBS no esté usando</b> (en Windows una cámara no se comparte). Si solo tienes una, usa «Mi voz»."
        : "La boca se abre con el volumen de tu voz. No usa cámara, así que no choca con OBS."
    }</p>
    <div class="row">
      <label class="field grow"><span>${current.mode === "camera" ? "Cámara" : "Micrófono"}</span>
        <select data-bind="${current.mode === "camera" ? "camera" : "mic"}">
          <option value="">Predeterminado</option>
          ${devices.map((d) => `<option value="${esc(d.id)}" ${d.id === selected ? "selected" : ""}>${esc(d.label)}</option>`).join("")}
        </select></label>
      <button class="ghost" data-action="find-devices">Buscar dispositivos</button>
    </div>
    <div class="row">
      ${live ? `<button class="danger" data-action="stop">Parar seguimiento</button>` : `<button class="primary" data-action="start" ${cfg.enabled ? "" : "disabled"}>Empezar seguimiento</button>`}
      ${live && current.mode === "camera" ? `<button data-action="calibrate">Calibrar (mirando al frente)</button>` : ""}
    </div>
    <p class="status ${t.kind}">${esc(t.text)}</p>
    <label class="field"><span>Sensibilidad de la boca</span><input type="range" min="0.5" max="2" step="0.1" value="${current.sensitivity}" data-bind="sensitivity" /></label>
    <details><summary>Más opciones</summary>
      <label class="check-inline"><input type="checkbox" data-bind="mirror" ${current.mirror ? "checked" : ""} /> Efecto espejo (tu lado izquierdo es el izquierdo de la pantalla)</label>
      <label class="check-inline"><input type="checkbox" data-bind="invert-pitch" ${current.invertPitch ? "checked" : ""} /> Invertir arriba/abajo de la cabeza</label>
      <label class="check-inline"><input type="checkbox" data-bind="autostart" ${current.autostart ? "checked" : ""} /> Empezar el seguimiento al abrir la app</label>
    </details>`;
  });

  // La preview también es una página conectada: no cuenta como «OBS la está viendo».
  const preview = wantPreview ? 1 : 0;
  const obsSeen = Math.max(0, a.clients - preview) > 0;
  section("a-obs", { url: a.url, running, obsSeen, live: connected(), enabled: cfg.enabled }, () => `
    <h3><span class="num">3</span> Ponlo en OBS</h3>
    <p>El avatar es una <b>fuente de navegador</b> con fondo transparente. Tienes dos formas:</p>
    <div class="row"><button class="primary" data-action="create-source" ${cfg.enabled && running && connected() ? "" : "disabled"}>Añadirlo a la escena actual de OBS</button>
      ${!connected() ? `<small class="hint">Conecta con OBS primero.</small>` : ""}</div>
    <p class="hint">O a mano: en OBS, <b>+ → Navegador</b>, pega esta dirección y pon 800 × 800:</p>
    <div class="url-row"><code>${esc(a.url)}</code><button data-action="copy" data-text="${esc(a.url)}" ${running ? "" : "disabled"}>Copiar</button></div>
    <p class="status ${obsSeen ? "ok" : ""}">${obsSeen ? "OBS está viendo el avatar ✓" : running ? "Todavía ninguna fuente de OBS está conectada al avatar." : "Activa el avatar para obtener la dirección."}</p>`);
}

/** Se llama al arrancar: si el usuario lo pidió, empieza a seguir la cara o la voz sin tener que abrir la pestaña. */
export function maybeAutostart() {
  if (autostarted) return;
  const cfg = state.snap?.settings.avatar;
  if (!cfg?.enabled || !state.avatar?.running) return;
  autostarted = true;
  if (current.autostart) void startTracking();
}

/** Si se desactiva el avatar desde otro sitio (p. ej. los ajustes), se suelta la cámara. */
export function guardTracker() {
  if (state.snap && !state.snap.settings.avatar.enabled && tracker.active) void tracker.stop();
}

