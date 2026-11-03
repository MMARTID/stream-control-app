import { api } from "./api";
import { connected, goTab, notify, state, subscribe, type Tab } from "./state";
import { $, esc, run, toast } from "./util";
import { guardTracker, maybeAutostart, mountAvatar, updateAvatar } from "./views/avatar";
import { applyFrame, mountLive, syncPreviews, updateLive } from "./views/live";
import { mountLights, updateLights } from "./views/lights";
import { mountMobile, renderPairingBanner, updateMobile } from "./views/mobile";
import { enterSettings, mountSettings, updateSettings } from "./views/settings";

const TABS: Tab[] = ["live", "avatar", "lights", "mobile", "settings"];
let confirmingStop = false;
let confirmTimer: number | undefined;
let shownTab: Tab | null = null;

const connLabels = {
  connected: "Conectado a OBS",
  connecting: "Conectando con OBS…",
  reconnecting: "Esperando a OBS…",
  disconnected: "Sin conexión con OBS",
} as const;

function renderHeader() {
  const s = state.snap;
  if (!s) return;
  const { state: conn, error } = s.connection;
  const pill = $("connection");
  pill.className = `pill ${conn}`;
  pill.innerHTML = `${esc(connLabels[conn])}${error && conn !== "connected" ? `<small title="${esc(error)}">${esc(error)}</small>` : ""}`;

  const box = $("stream-control");
  const html = s.stream.active
    ? confirmingStop
      ? `<button class="danger" data-action="stop-stream">Confirmar: parar el directo</button>`
      : `<button class="on-air" data-action="ask-stop">${s.stream.reconnecting ? "Reconectando… " : "● En directo · "}Parar</button>`
    : `<button class="live" data-action="start-stream" ${connected() ? "" : "disabled"}>● Iniciar directo</button>`;
  if (box.innerHTML !== html) box.innerHTML = html;
}

function renderTabs() {
  document.querySelectorAll<HTMLButtonElement>("#tabs button").forEach((b) => {
    const on = b.dataset.tab === state.tab;
    b.classList.toggle("on", on);
    b.setAttribute("aria-selected", String(on));
  });
  const avatarLive = state.snap?.settings.avatar.enabled && state.tracker.kind !== "off";
  document.querySelector<HTMLElement>('#tabs [data-tab="avatar"]')?.classList.toggle("active-dot", !!avatarLive);
}

function update() {
  if (!state.snap) return;
  if (state.tab !== shownTab) {
    for (const t of TABS) $(`view-${t}`).hidden = t !== state.tab;
    if (state.tab === "settings") enterSettings();
    shownTab = state.tab;
    syncPreviews();
  }
  renderHeader();
  renderTabs();
  renderPairingBanner();
  guardTracker();
  maybeAutostart();
  switch (state.tab) {
    case "live": return updateLive();
    case "avatar": return updateAvatar();
    case "lights": return updateLights();
    case "mobile": return updateMobile();
    case "settings": return updateSettings();
  }
}

document.addEventListener("click", (e) => {
  const el = (e.target as HTMLElement).closest<HTMLElement>("#tabs [data-tab], #stream-control [data-action]");
  if (!el) return;
  if (el.dataset.tab) return goTab(el.dataset.tab as Tab);
  switch (el.dataset.action) {
    case "start-stream": return void run(api.startStream());
    case "ask-stop":
      // Confirmación en línea, sin ventana emergente que tape la pantalla (RF-26, RNF-6).
      confirmingStop = true;
      window.clearTimeout(confirmTimer);
      confirmTimer = window.setTimeout(() => { confirmingStop = false; renderHeader(); }, 4000);
      return renderHeader();
    case "stop-stream":
      confirmingStop = false;
      window.clearTimeout(confirmTimer);
      return void run(api.stopStream());
  }
});

document.addEventListener("visibilitychange", syncPreviews);
setInterval(renderPairingBanner, 1000);

async function main() {
  mountLive($("view-live"));
  mountAvatar($("view-avatar"));
  mountLights($("view-lights"));
  mountMobile($("view-mobile"));
  mountSettings($("view-settings"));
  subscribe(update);

  await api.onFrame((f) => applyFrame(f.key, f.image));
  await api.onState((s) => { state.snap = s; notify(); });
  await api.onPanel((p) => { state.panel = p; notify(); });
  await api.onAvatar((a) => { state.avatar = a; notify(); });
  await api.onLights((l) => { state.lights = l; notify(); });

  [state.snap, state.panel, state.avatar, state.lights, state.lightModes, state.version] = await Promise.all([
    api.getSnapshot(),
    api.getPanelStatus(),
    api.getAvatarStatus(),
    api.getLightsStatus(),
    api.lightsModes(),
    api.version(),
  ]);
  $("version").textContent = state.version === "dev" ? "modo prueba" : state.version ? `v${state.version}` : "";
  notify();
}

main().catch((e) => toast(String(e)));
