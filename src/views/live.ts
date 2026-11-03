// Pestaña «Directo»: lo que se usa mientras emites. Botones grandes, nada de ventanas emergentes (RNF-6).
import { api, type Snapshot } from "../api";
import { connected, goTab, state } from "../state";
import { updateSettings } from "../ops";
import { $, bitrate, clock, esc, paint, paintHtml, run, store } from "../util";

const prefs = store("sc_live_prefs", { stepsHidden: false });

export function mountLive(root: HTMLElement) {
  root.innerHTML = `
    <div id="live-help"></div>
    <div id="live-steps"></div>

    <section class="top">
      <div>
        <h2>Salida en programa <small>Lo que ven tus espectadores ahora</small></h2>
        <div class="frame empty">
          <img data-frame="@program" alt="Salida en programa" />
          <span id="program-label" class="label"></span>
        </div>
        <p class="hint">Son capturas de OBS (unas 3 por segundo), no vídeo en directo.</p>
      </div>
      <div>
        <h2>Salud del directo <small>Se actualiza cada 2 segundos</small></h2>
        <div id="health" class="health"></div>
      </div>
    </section>

    <section>
      <div class="section-head">
        <h2>Escenas rápidas <small>Un toque para cambiar de escena</small></h2>
        <div class="tools">
          <button data-action="back" id="back-button">↩ Escena anterior</button>
          <button data-action="mic" id="mic-button">Micro</button>
        </div>
      </div>
      <div id="quick" class="grid"></div>
    </section>

    <section>
      <h2>Cámaras <small>Toca una para ponerla en pantalla; la anterior se oculta después, sin cortes</small></h2>
      <div id="cameras" class="grid cameras"></div>
    </section>

    <section>
      <h2>Fuentes de la escena <small>Muestra u oculta cada elemento</small></h2>
      <div id="sources" class="sources"></div>
    </section>

    <section>
      <div class="section-head">
        <h2>Todas las escenas <small>Busca cualquiera, aunque no sea rápida. La ★ la añade a las rápidas</small></h2>
        <input id="search" type="search" placeholder="Buscar escena por nombre" autocomplete="off" />
      </div>
      <div id="scenes" class="scene-list"></div>
    </section>`;

  root.addEventListener("click", (e) => {
    const el = (e.target as HTMLElement).closest<HTMLElement>("[data-action]");
    if (!el) return;
    const { action, scene, id } = el.dataset;
    switch (action) {
      case "launch": return void run(api.launchScene(scene!));
      case "back": return void run(api.goBack());
      case "camera": return void run(api.switchCamera(id!));
      case "mic": return void run(api.toggleMic());
      case "source": return void run(api.setSourceEnabled(Number(id), el.dataset.enabled !== "true"));
      case "toggle-quick": return void toggleQuick(scene!);
      case "go": return goTab(el.dataset.tab as "settings" | "avatar" | "mobile");
      case "hide-steps": prefs.set({ stepsHidden: true }); return updateLive();
    }
  });
  root.addEventListener("input", (e) => {
    if ((e.target as HTMLElement).id === "search") renderScenes();
  });
}

function toggleQuick(scene: string) {
  return updateSettings((s) => {
    const at = s.quickScenes.findIndex((q) => q.scene === scene);
    if (at >= 0) s.quickScenes.splice(at, 1);
    else s.quickScenes.push({ scene, label: scene, color: "#3b82f6", icon: "▶", muteMic: false });
  });
}

// ---- Ayuda cuando OBS no conecta ----

function renderHelp(s: Snapshot) {
  const waiting = s.connection.state !== "disconnected";
  paint("live-help", s.connection, () => s.connection.state === "connected" ? "" : `<div class="callout ${waiting ? "warn" : "bad"}">
    <h3>${waiting ? "Esperando a OBS…" : "No hay conexión con OBS"}</h3>
    ${s.connection.error ? `<p class="err">${esc(s.connection.error)}</p>` : ""}
    <ol>
      <li>Abre <b>OBS Studio</b> (versión 28 o superior).</li>
      <li>En OBS: <b>Herramientas → Ajustes del servidor WebSocket</b> y marca «Habilitar servidor WebSocket».</li>
      <li>Si le pusiste contraseña, escríbela en <b>Ajustes → Conexión con OBS</b>.</li>
    </ol>
    <p>${waiting ? "La app seguirá intentándolo sola: en cuanto OBS esté listo, conecta sin que hagas nada." : "Revisa la dirección y el puerto (por defecto 127.0.0.1 y 4455)."}</p>
    <button data-action="go" data-tab="settings">Ir a Ajustes</button>
  </div>`);
}

// ---- Primeros pasos ----

function renderSteps(s: Snapshot) {
  const panelOn = !!state.panel?.enabled && (state.panel?.devices.length ?? 0) > 0;
  const steps = [
    { done: connected(), title: "Conecta con OBS", text: "La app se conecta sola cuando OBS está abierto con el servidor WebSocket activado.", tab: "settings", cta: "Ver cómo", optional: false },
    { done: !!s.settings.micInput, title: "Elige tu micrófono", text: "Así puedes silenciarlo con un toque o al entrar en «Pausa / baño».", tab: "settings", cta: "Elegir micro", optional: false },
    { done: s.settings.cameras.length > 0, title: "Añade tus cámaras", text: "Móvil, webcam… para cambiar de una a otra sin cortes.", tab: "settings", cta: "Añadir cámaras", optional: false },
    { done: s.settings.quickScenes.length > 0, title: "Marca tus escenas rápidas", text: "Usa la ★ de la lista «Todas las escenas» o créalas en Ajustes.", tab: "settings", cta: "Crear escenas", optional: false },
    { done: panelOn, title: "Controla desde el móvil", text: "Un panel web dentro de tu Wi‑Fi, con contraseña y sin nube.", tab: "mobile", cta: "Activar", optional: true },
    { done: s.settings.avatar.enabled, title: "Prueba el avatar", text: "Un personaje que mueve la boca cuando hablas.", tab: "avatar", cta: "Probar", optional: true },
  ];
  const required = steps.filter((x) => !x.optional);
  const complete = required.every((x) => x.done);
  if (prefs.get().stepsHidden || (complete && steps.every((x) => x.done))) return paintHtml("live-steps", "");
  const doneCount = steps.filter((x) => x.done).length;
  paintHtml("live-steps", `<div class="callout steps">
    <div class="section-head">
      <h3>Primeros pasos <small>${doneCount} de ${steps.length}</small></h3>
      <button class="ghost" data-action="hide-steps">Ocultar</button>
    </div>
    <div class="progress"><span style="width:${(doneCount / steps.length) * 100}%"></span></div>
    <ul>${steps
      .map(
        (x) => `<li class="${x.done ? "done" : ""}">
        <span class="check" aria-hidden="true">${x.done ? "✓" : ""}</span>
        <span class="grow"><b>${x.title}</b>${x.optional ? ` <em>opcional</em>` : ""}<small>${x.text}</small></span>
        ${x.done ? "" : `<button data-action="go" data-tab="${x.tab}">${x.cta}</button>`}
      </li>`,
      )
      .join("")}</ul>
  </div>`);
}

// ---- Capturas ----

function thumb(key: string, alt: string) {
  const src = state.frames.get(key);
  return `<div class="thumb ${src ? "" : "empty"}"><img data-frame="${esc(key)}" alt="${esc(alt)}" ${src ? `src="${src}"` : ""} /></div>`;
}

export function applyFrame(key: string, image: string) {
  state.frames.set(key, image);
  document.querySelectorAll<HTMLImageElement>("img[data-frame]").forEach((img) => {
    if (img.dataset.frame !== key) return;
    img.src = image;
    img.parentElement?.classList.remove("empty");
  });
}

// ---- Secciones ----

function renderQuick(s: Snapshot) {
  const live = connected();
  const items = s.settings.quickScenes
    .map((q) => {
      const missing = s.scenes.length > 0 && !s.scenes.includes(q.scene);
      const active = s.currentScene === q.scene;
      const note = missing ? "no existe en OBS" : q.muteMic ? "silencia el micro" : "";
      return `<button class="big ${active ? "active" : ""}" style="--c:${esc(q.color)}" data-action="launch" data-scene="${esc(q.scene)}" ${live && !missing ? "" : "disabled"}>
        <span class="icon">${esc(q.icon)}</span><span>${esc(q.label || q.scene)}</span>${note ? `<small>${note}</small>` : ""}
      </button>`;
    })
    .join("");
  paintHtml("quick",
    items ||
    `<div class="empty-state"><p><b>Aún no tienes escenas rápidas.</b> Son botones grandes para cambiar de escena de un toque (por ejemplo «Pausa / baño» o «Cambio de tema»).</p>
      <button class="primary" data-action="go" data-tab="settings">Crear escenas rápidas</button></div>`);
}

function renderCameras(s: Snapshot) {
  const live = connected();
  const items = s.cameras
    .map(
      (c) => `<button class="big cam ${c.active ? "active" : ""}" data-action="camera" data-id="${esc(c.id)}" ${live && c.inScene ? "" : "disabled"}>
        ${c.inScene ? thumb(c.source, c.name) : ""}<span>${esc(c.name)}</span><small>${c.inScene ? (c.active ? "en pantalla" : "disponible") : "no está en esta escena"}</small>
      </button>`,
    )
    .join("");
  paintHtml("cameras",
    items ||
    `<div class="empty-state"><p><b>Aún no has añadido cámaras.</b> Dile a la app cuáles son las fuentes de cámara de OBS (por ejemplo tu móvil y tu webcam) para alternarlas con un toque.</p>
      <button class="primary" data-action="go" data-tab="settings">Añadir cámaras</button></div>`);
}

function renderSources(s: Snapshot) {
  const live = connected();
  const items = s.sources
    .map(
      (x) => `<div class="source ${x.enabled ? "" : "off"}">
        ${x.visual ? thumb(x.name, x.name) : `<div class="thumb audio">🔊</div>`}
        <div class="meta">
          <span class="name" title="${esc(x.name)}">${esc(x.name)}</span>
          <button data-action="source" data-id="${x.id}" data-enabled="${x.enabled}" ${live ? "" : "disabled"}>${x.enabled ? "Visible" : "Oculta"}</button>
        </div>
      </div>`,
    )
    .join("");
  paintHtml("sources", items || `<p class="hint">${live ? "La escena en programa no tiene fuentes." : "Se verán cuando haya conexión con OBS."}</p>`);
}

function renderHealth(s: Snapshot) {
  const h = s.health;
  if (!h || !connected()) {
    return paintHtml("health", `<p class="hint">Se verán cuando haya conexión con OBS.</p>`);
  }
  const lostPct = h.totalFrames > 0 ? (h.droppedFrames / h.totalFrames) * 100 : 0;
  const level = (bad: boolean, warn: boolean) => (bad ? "bad" : warn ? "warn" : "");
  const cells: [string, string, string, string][] = [
    ["En directo", s.stream.active ? clock(h.durationSecs) : "—", s.stream.reconnecting ? "warn" : "", ""],
    ["Bitrate", h.bitrateKbps != null ? bitrate(h.bitrateKbps) : "—", "", "Datos que envías por segundo"],
    ["Frames perdidos", h.totalFrames > 0 ? `${lostPct.toFixed(1).replace(".", ",")} %` : "—", level(lostPct > 5, lostPct > 2), "Si sube de un 2 %, la imagen se verá a saltos"],
    ["FPS", h.fps.toFixed(0), level(h.fps < 20, h.fps < 28), "Imágenes por segundo"],
    ["CPU de OBS", `${h.cpu.toFixed(0)} %`, level(h.cpu > 90, h.cpu > 75), "Carga del procesador"],
  ];
  paintHtml("health", cells
    .map(([label, value, lvl, tip]) => `<div class="stat ${lvl}" title="${esc(tip)}"><span>${label}</span><strong>${esc(value)}</strong></div>`)
    .join(""));
}

function renderScenes(s: Snapshot | null = state.snap) {
  if (!s) return;
  const live = connected();
  const query = $<HTMLInputElement>("search").value.trim().toLowerCase();
  const quick = new Set(s.settings.quickScenes.map((q) => q.scene));
  const rows = s.scenes
    .filter((name) => name.toLowerCase().includes(query))
    .map(
      (name) => `<div class="scene-row">
        <button class="name ${s.currentScene === name ? "active" : ""}" data-action="launch" data-scene="${esc(name)}" ${live ? "" : "disabled"}>${esc(name)}</button>
        <button class="star ${quick.has(name) ? "on" : ""}" data-action="toggle-quick" data-scene="${esc(name)}" title="${quick.has(name) ? "Quitar de escenas rápidas" : "Marcar como escena rápida"}">${quick.has(name) ? "★" : "☆"}</button>
      </div>`,
    )
    .join("");
  paintHtml("scenes", rows || `<p class="hint">${s.scenes.length ? "Ninguna escena coincide con la búsqueda." : "OBS no ha enviado escenas todavía."}</p>`);
}

function renderControls(s: Snapshot) {
  const live = connected();
  const mic = $<HTMLButtonElement>("mic-button");
  mic.disabled = !live || s.micMuted === null;
  mic.className = s.micMuted ? "on" : "";
  mic.textContent = s.micMuted === null ? "Micro (elígelo en Ajustes)" : s.micMuted ? "🔇 Micro silenciado" : "🎙 Micro activo";
  $<HTMLButtonElement>("back-button").disabled = !live || !s.previousScene;
}

export function updateLive() {
  const s = state.snap;
  if (!s) return;
  renderHelp(s);
  renderSteps(s);
  renderControls(s);
  renderQuick(s);
  renderCameras(s);
  renderSources(s);
  renderHealth(s);
  renderScenes(s);
  $("program-label").textContent = s.currentScene ?? "";
  syncPreviews();
}

/** Solo se piden capturas a OBS mientras esta pestaña se ve y la ventana está a la vista. */
let lastInterest = "";
export function syncPreviews() {
  const watching = state.tab === "live" && !document.hidden && connected();
  const s = state.snap;
  const interest = {
    program: watching,
    sources: watching && s ? s.sources.filter((x) => x.visual).map((x) => x.name) : [],
  };
  const key = JSON.stringify(interest);
  if (key === lastInterest) return;
  lastInterest = key;
  void run(api.watchPreviews(interest));
}
