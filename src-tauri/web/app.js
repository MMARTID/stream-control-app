// Panel web de Stream Control para el móvil. JavaScript sin dependencias ni paso de compilación:
// lo sirve la app de escritorio tal cual. Todo el texto dinámico se inserta con textContent,
// nunca con innerHTML.

const TOKEN_KEY = "sc_token";
const NAME_KEY = "sc_name";

const store = {
  get: (k) => { try { return localStorage.getItem(k); } catch { return null; } },
  set: (k, v) => { try { localStorage.setItem(k, v); } catch { /* modo privado */ } },
  del: (k) => { try { localStorage.removeItem(k); } catch { /* modo privado */ } },
};

/** @type {string|null} */
let token = store.get(TOKEN_KEY);
/** @type {any} */
let state = null;
/** @type {WebSocket|null} */
let ws = null;
let link = "connecting"; // connecting | open | closed
let reconnectDelay = 1000;
let reconnectTimer = 0;
let deviceName = "";
let armed = null; // { action, confirm, until }
let busy = false;
const frames = new Map();
const ui = {};

const $app = document.getElementById("app");

// ---------- Utilidades ----------

function h(tag, props = {}, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (k === "class") el.className = v;
    else if (k === "text") el.textContent = v;
    else if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
    else if (v === true) el.setAttribute(k, "");
    else if (v !== false && v != null) el.setAttribute(k, v);
  }
  for (const c of children.flat()) if (c != null) el.append(c);
  return el;
}

/** Mantiene los elementos de `container` alineados con `items` sin recrearlos (así no parpadean las imágenes). */
function reconcile(container, items, keyOf, create, update) {
  const existing = new Map([...container.children].map((el) => [el.dataset.key, el]));
  let prev = null;
  for (const item of items) {
    const key = String(keyOf(item));
    let el = existing.get(key);
    if (el) existing.delete(key);
    else { el = create(item); el.dataset.key = key; }
    update(el, item);
    const target = prev ? prev.nextSibling : container.firstChild;
    if (el !== target) container.insertBefore(el, target);
    prev = el;
  }
  for (const el of existing.values()) el.remove();
}

const pad = (n) => String(n).padStart(2, "0");
const clock = (s) => `${pad(Math.floor(s / 3600))}:${pad(Math.floor((s % 3600) / 60))}:${pad(s % 60)}`;
const bitrate = (k) => (k >= 1000 ? `${(k / 1000).toFixed(1).replace(".", ",")} Mb/s` : `${k} kb/s`);
const buzz = (ms = 15) => { try { navigator.vibrate?.(ms); } catch { /* sin vibración */ } };

function guessDeviceName() {
  const ua = navigator.userAgent;
  const os = /Android/.test(ua) ? "Android" : /iPhone|iPad/.test(ua) ? "iPhone" : /Windows/.test(ua) ? "Windows" : /Mac/.test(ua) ? "Mac" : "Móvil";
  const br = /Firefox/.test(ua) ? "Firefox" : /Edg/.test(ua) ? "Edge" : /SamsungBrowser/.test(ua) ? "Samsung Internet" : /Chrome/.test(ua) ? "Chrome" : /Safari/.test(ua) ? "Safari" : "navegador";
  return `${os} · ${br}`;
}

let toastTimer = 0;
function toast(message) {
  if (!ui.toast) return;
  ui.toast.textContent = message;
  ui.toast.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (ui.toast.hidden = true), 5000);
  buzz(40);
}

// ---------- Emparejamiento ----------

function showPairing(message = "") {
  closeSocket();
  token = null;
  store.del(TOKEN_KEY);
  Object.keys(ui).forEach((k) => delete ui[k]);
  let pairingId = null;

  const error = h("p", { class: "error", role: "alert", text: message });
  const name = h("input", { type: "text", value: store.get(NAME_KEY) || guessDeviceName(), maxlength: "40", autocomplete: "off", "aria-label": "Nombre de este dispositivo" });
  const password = h("input", { type: "password", autocomplete: "current-password", placeholder: "Contraseña del panel", "aria-label": "Contraseña del panel" });
  const code = h("input", { type: "text", inputmode: "numeric", pattern: "[0-9]*", maxlength: "6", autocomplete: "one-time-code", class: "code", placeholder: "······", "aria-label": "Código de 6 dígitos" });
  const step1 = h("div", { class: "pair" },
    h("h1", { text: "Stream Control" }),
    h("p", { text: "Conecta este móvil con tu PC. Estás en la misma red local." }),
    name, password,
    h("button", { class: "primary", text: "Continuar", onclick: start }),
    error);
  const step2 = h("div", { class: "pair", hidden: true },
    h("h1", { text: "Introduce el código" }),
    h("p", { text: "Míralo en la app de escritorio de Stream Control, en Panel web → Dispositivos. Caduca en 2 minutos." }),
    code,
    h("button", { class: "primary", text: "Emparejar", onclick: finish }),
    h("button", { class: "link", text: "Empezar de nuevo", onclick: () => showPairing() }),
    error.cloneNode());
  const err2 = step2.lastChild;

  async function start() {
    error.textContent = "";
    if (!password.value) { error.textContent = "Escribe la contraseña del panel"; return; }
    const res = await api("/api/pair/start", { password: password.value, name: name.value }, false);
    password.value = "";
    if (res.error) { error.textContent = res.error; return; }
    store.set(NAME_KEY, name.value);
    pairingId = res.pairingId;
    step1.hidden = true;
    step2.hidden = false;
    code.focus();
  }

  async function finish() {
    err2.textContent = "";
    const res = await api("/api/pair/finish", { pairingId, code: code.value }, false);
    if (res.error) {
      err2.textContent = res.error;
      if (/caducado|Vuelve/.test(res.error)) setTimeout(() => showPairing(res.error), 1200);
      return;
    }
    token = res.token;
    store.set(TOKEN_KEY, token);
    buildApp();
    connect();
  }

  code.addEventListener("keydown", (e) => e.key === "Enter" && finish());
  password.addEventListener("keydown", (e) => e.key === "Enter" && start());
  $app.replaceChildren(step1, step2);
}

// ---------- Red ----------

async function api(path, body, auth = true) {
  try {
    const res = await fetch(path, {
      method: "POST",
      headers: { "Content-Type": "application/json", ...(auth && token ? { Authorization: `Bearer ${token}` } : {}) },
      body: JSON.stringify(body),
    });
    const data = await res.json().catch(() => ({}));
    if (res.status === 401 && auth) { showPairing("Este dispositivo ya no está autorizado. Vuelve a emparejarlo."); return { error: "no autorizado" }; }
    return res.ok ? data : { error: data.error || `Error ${res.status}` };
  } catch {
    return { error: "No se pudo contactar con el PC. ¿Sigue en la misma Wi‑Fi?" };
  }
}

function closeSocket() {
  clearTimeout(reconnectTimer);
  if (ws) { ws.onclose = null; ws.close(); ws = null; }
}

function connect() {
  closeSocket();
  link = "connecting";
  renderStatus();
  const socket = new WebSocket(`${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/ws`);
  ws = socket;
  socket.onopen = () => socket.send(JSON.stringify({ type: "hello", token }));
  socket.onmessage = (e) => {
    const msg = JSON.parse(e.data);
    switch (msg.type) {
      case "ready": link = "open"; reconnectDelay = 1000; deviceName = msg.name; sendWatch(true); break;
      case "state": state = msg.state; render(); break;
      case "frame": applyFrame(msg.key, msg.image); break;
      case "denied": case "revoked": showPairing("Este dispositivo ya no está autorizado. Vuelve a emparejarlo."); return;
    }
    renderStatus();
  };
  socket.onclose = () => {
    if (ws !== socket) return;
    link = "closed";
    armed = null;
    renderStatus();
    reconnectTimer = setTimeout(connect, reconnectDelay);
    reconnectDelay = Math.min(reconnectDelay * 2, 8000);
  };
}

async function command(body) {
  if (busy) return null;
  busy = true;
  buzz();
  const res = await api("/api/cmd", body);
  busy = false;
  if (res.error && res.error !== "no autorizado") toast(res.error);
  return res;
}

// ---------- Capturas ----------

function applyFrame(key, image) {
  frames.set(key, image);
  for (const img of document.querySelectorAll("img[data-frame]")) {
    if (img.dataset.frame !== key) continue;
    img.src = image;
    img.parentElement.classList.remove("empty");
  }
}

function thumb(key, alt) {
  const src = frames.get(key);
  const img = h("img", { "data-frame": key, alt, ...(src ? { src } : {}) });
  return h("div", { class: `thumb ${src ? "" : "empty"}` }, img);
}

let lastWatch = "";
/** Dice al PC qué capturas quiere ver este móvil. Con la página oculta no pide ninguna. */
function sendWatch(force = false) {
  if (!ws || ws.readyState !== WebSocket.OPEN || link !== "open") return;
  const visible = document.visibilityState === "visible";
  const sources = [];
  if (visible && state) {
    for (const c of state.cameras) if (c.inScene) sources.push(c.preview);
    if (ui.sourcesSection?.open) for (const s of state.sources) if (s.visual && !sources.includes(s.name)) sources.push(s.name);
  }
  const msg = { type: "watch", program: visible, sources };
  const key = JSON.stringify(msg);
  if (!force && key === lastWatch) return;
  lastWatch = key;
  ws.send(key);
}

// ---------- Interfaz principal ----------

function buildApp() {
  ui.dot = h("span", { class: "dot wait" });
  ui.live = h("span", { class: "live", hidden: true });
  ui.banner = h("div", { class: "banner", role: "status", hidden: true });
  ui.programImg = h("img", { "data-frame": "@program", alt: "Salida en programa" });
  ui.programFrame = h("div", { class: "frame empty" }, ui.programImg, (ui.programLabel = h("span", { class: "label" })));
  ui.quick = h("div", { class: "grid" });
  ui.cams = h("div", { class: "grid" });
  ui.camsSection = h("section", {}, h("h2", { text: "Cámaras" }), ui.cams);
  ui.health = h("div", { class: "stats" });
  ui.sources = h("div", { class: "sources" });
  ui.sourcesSection = h("details", { ontoggle: () => sendWatch() }, h("summary", { text: "Fuentes de la escena" }), ui.sources);
  ui.search = h("input", { type: "search", placeholder: "Buscar escena", autocomplete: "off", "aria-label": "Buscar escena", oninput: renderScenes });
  ui.scenes = h("div", { class: "scenes" });
  ui.scenesSection = h("details", {}, h("summary", { text: "Todas las escenas" }), ui.search, ui.scenes);
  ui.who = h("p", { class: "hint" });
  ui.toast = h("div", { class: "toast", role: "alert", hidden: true });
  ui.mic = h("button", { class: "mic act", onclick: () => command({ cmd: "toggle_mic" }) });
  ui.back = h("button", { class: "act", text: "↩ Anterior", onclick: () => command({ cmd: "back" }) });
  ui.stream = h("button", { class: "act", onclick: onStream });
  ui.main = h("main", {},
    h("section", {}, ui.programFrame, h("p", { class: "hint", text: "Capturas de OBS, no vídeo en directo." })),
    h("section", { class: "act" }, h("h2", { text: "Escenas rápidas" }), ui.quick),
    ui.camsSection,
    h("section", {}, h("h2", { text: "Salud del directo" }), ui.health),
    ui.sourcesSection,
    ui.scenesSection,
    h("section", {}, ui.who, h("button", { class: "link", text: "Desvincular este dispositivo", onclick: unlink })));

  $app.replaceChildren(
    h("header", { class: "bar" }, h("h1", { text: "Stream Control" }), ui.live, ui.dot),
    ui.banner,
    ui.main,
    h("nav", { class: "dock" }, ui.mic, ui.back, ui.stream),
    ui.toast);
  render();
}

function unlink() {
  if (confirm("¿Desvincular este móvil? Tendrás que emparejarlo de nuevo.")) showPairing();
}

function renderStatus() {
  if (!ui.dot) return;
  const online = link === "open" && state?.obsConnected;
  ui.dot.className = `dot ${online ? "ok" : link === "closed" ? "" : "wait"}`;
  ui.main.classList.toggle("offline", !online);
  ui.banner.hidden = !!online;
  ui.banner.className = `banner ${link === "closed" ? "bad" : ""}`;
  ui.banner.textContent =
    link === "connecting" ? "Conectando con el PC…" :
    link === "closed" ? "Sin conexión con el PC. Reintentando… (los botones no envían nada hasta que vuelva)" :
    !state ? "Esperando datos…" :
    "El PC responde, pero OBS no está conectado. Los botones están desactivados.";
  if (state) renderDock();
}

function renderScenes() {
  if (!state) return;
  const query = ui.search.value.trim().toLowerCase();
  const list = state.scenes.filter((s) => s.toLowerCase().includes(query) && (!state.onlyQuickScenes || state.quickScenes.some((q) => q.scene === s)));
  reconcile(ui.scenes, list, (s) => s, (s) => h("button", { class: "act", text: s, onclick: () => command({ cmd: "launch_scene", scene: s }) }), (el, s) => {
    el.classList.toggle("current", s === state.currentScene);
  });
}

function renderDock() {
  const s = state;
  ui.mic.disabled = !s.micConfigured;
  ui.mic.classList.toggle("muted", s.micMuted === true);
  ui.mic.textContent = !s.micConfigured ? "Micro sin elegir" : s.micMuted ? "🔇 Silenciado" : "🎙 Micro activo";
  ui.back.disabled = !s.previousScene;
  ui.stream.hidden = !s.allowStreamControl;
  if (armed && Date.now() < armed.until) {
    const left = Math.ceil((armed.until - Date.now()) / 1000);
    ui.stream.className = "confirm act";
    ui.stream.textContent = `Confirmar ${armed.action === "start" ? "inicio" : "parada"} (${left})`;
  } else {
    armed = null;
    ui.stream.className = `${s.streaming ? "stop" : "go"} act`;
    ui.stream.textContent = s.streaming ? "■ Parar directo" : "● Iniciar directo";
  }
}

function render() {
  if (!state || !ui.main) return;
  const s = state;

  ui.live.hidden = !s.streaming;
  ui.live.className = `live ${s.streamReconnecting ? "reconnecting" : ""}`;
  ui.live.textContent = s.streamReconnecting ? "RECONECTANDO" : `● ${s.health?.durationSecs != null ? clock(s.health.durationSecs) : "DIRECTO"}`;
  ui.programLabel.textContent = s.currentScene || "";

  reconcile(ui.quick, s.quickScenes, (q) => q.scene,
    (q) => {
      const el = h("button", { class: "big act", onclick: () => command({ cmd: "launch_scene", scene: el.dataset.scene }) }, h("span", { class: "icon" }), h("span", { class: "title" }), h("small"));
      return el;
    },
    (el, q) => {
      el.dataset.scene = q.scene;
      el.style.setProperty("--c", /^#[0-9a-f]{3,8}$/i.test(q.color) ? q.color : "#3b82f6");
      el.classList.toggle("active", q.scene === s.currentScene);
      el.children[0].textContent = q.icon;
      el.children[1].textContent = q.label || q.scene;
      const missing = !s.scenes.includes(q.scene);
      el.children[2].textContent = missing ? "no existe en OBS" : q.muteMic ? "silencia el micro" : "";
      el.disabled = missing;
    });
  if (!s.quickScenes.length && !ui.quick.children.length) ui.quick.textContent = "";

  ui.camsSection.hidden = s.cameras.length === 0;
  reconcile(ui.cams, s.cameras, (c) => c.id,
    (c) => {
      const el = h("button", { class: "big cam act", onclick: () => command({ cmd: "switch_camera", id: c.id }) }, thumb(c.preview, c.name), h("span"), h("small"));
      return el;
    },
    (el, c) => {
      el.classList.toggle("active", c.active);
      el.disabled = !c.inScene;
      el.children[1].textContent = c.name;
      el.children[2].textContent = c.inScene ? (c.active ? "en pantalla" : "disponible") : "no está en esta escena";
      const img = el.querySelector("img");
      if (img) img.dataset.frame = c.preview;
    });

  renderHealth();

  reconcile(ui.sources, s.sources, (x) => x.id,
    (x) => h("div", { class: "source" },
      x.visual ? thumb(x.name, x.name) : h("div", { class: "thumb audio", text: "🔊" }),
      h("span", { class: "name" }),
      h("button", { class: "act" })),
    (el, x) => {
      el.classList.toggle("off", !x.enabled);
      el.children[1].textContent = x.name;
      const btn = el.children[2];
      btn.textContent = x.enabled ? "Visible" : "Oculta";
      btn.onclick = () => command({ cmd: "source", id: x.id, enabled: !x.enabled });
    });

  renderScenes();
  ui.who.textContent = deviceName ? `Este dispositivo: ${deviceName}` : "";
  renderStatus();
  sendWatch();
}

function renderHealth() {
  const hl = state.health;
  if (!hl || !state.obsConnected) { ui.health.textContent = "Sin datos de OBS."; return; }
  const lost = hl.totalFrames > 0 ? (hl.droppedFrames / hl.totalFrames) * 100 : 0;
  const lvl = (bad, warn) => (bad ? "bad" : warn ? "warn" : "");
  const cells = [
    ["Bitrate", hl.bitrateKbps != null ? bitrate(hl.bitrateKbps) : "—", ""],
    ["Perdidos", hl.totalFrames > 0 ? `${lost.toFixed(1).replace(".", ",")} %` : "—", lvl(lost > 5, lost > 2)],
    ["FPS", hl.fps.toFixed(0), lvl(hl.fps < 20, hl.fps < 28)],
    ["CPU de OBS", `${hl.cpu.toFixed(0)} %`, lvl(hl.cpu > 90, hl.cpu > 75)],
  ];
  reconcile(ui.health, cells, (c) => c[0], () => h("div", { class: "stat" }, h("span"), h("strong")), (el, [label, value, level]) => {
    el.className = `stat ${level}`;
    el.children[0].textContent = label;
    el.children[1].textContent = value;
  });
}

// ---------- Iniciar y parar el directo: dos pasos ----------

async function onStream() {
  if (armed && Date.now() < armed.until) {
    const { confirm: token } = { confirm: armed.confirm };
    armed = null;
    renderDock();
    const res = await command({ cmd: "stream_confirm", confirm: token });
    if (res && !res.error) toast(state?.streaming ? "Parando el directo…" : "Iniciando el directo…");
    return;
  }
  const action = state.streaming ? "stop" : "start";
  const res = await command({ cmd: "stream_arm", action });
  if (res && res.confirm) {
    armed = { action, confirm: res.confirm, until: Date.now() + res.expiresIn * 1000 };
    renderDock();
  }
}

setInterval(() => { if (armed && state) renderDock(); }, 250);

// ---------- Ciclo de vida ----------

document.addEventListener("visibilitychange", async () => {
  sendWatch();
  if (document.visibilityState === "visible") {
    try { if (navigator.wakeLock) await navigator.wakeLock.request("screen"); } catch { /* no disponible */ }
    if (token && link === "closed") { reconnectDelay = 1000; connect(); }
  }
});

if (token) {
  buildApp();
  connect();
  try { navigator.wakeLock?.request("screen").catch(() => {}); } catch { /* no disponible */ }
} else {
  showPairing();
}
