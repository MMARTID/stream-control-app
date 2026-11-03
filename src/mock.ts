// OBS y núcleo simulados para ver y probar la interfaz en un navegador normal (`npm run dev`),
// sin Tauri ni OBS. Solo se carga cuando no existe `__TAURI_INTERNALS__`.
// Modos por la URL: ?mock=full (por defecto) | empty (primer uso, sin configurar) | offline (OBS cerrado),
// y ?pairing para ver un emparejamiento pendiente.
import type { AvatarStatus, InputView, Interest, PanelStatus, Settings, Snapshot } from "./api";

const params = new URLSearchParams(location.search);
const mode = params.get("mock") ?? "full";

const handlers = new Map<string, Set<(p: unknown) => void>>();
export function listen(event: string, handler: (p: unknown) => void) {
  if (!handlers.has(event)) handlers.set(event, new Set());
  handlers.get(event)!.add(handler);
}
const emit = (event: string, payload: unknown) => handlers.get(event)?.forEach((h) => h(structuredClone(payload)));

const full: Settings = {
  obs: { host: "127.0.0.1", port: 4455 },
  panel: { enabled: true, port: 8443, allowStreamControl: false, onlyQuickScenes: false },
  avatar: { enabled: true, port: 4488, character: "jason" },
  micInput: "Micro",
  quickScenes: [
    { scene: "Pausa / baño", label: "Pausa / baño", color: "#f59e0b", icon: "☕", muteMic: true },
    { scene: "Charla", label: "Cambio de tema", color: "#8b5cf6", icon: "💬", muteMic: false },
  ],
  cameras: [
    { id: "movil", name: "Móvil", source: "Cámara móvil" },
    { id: "webcam", name: "Webcam", source: "Webcam" },
  ],
};
const empty: Settings = {
  obs: { host: "127.0.0.1", port: 4455 },
  panel: { enabled: false, port: 8443, allowStreamControl: false, onlyQuickScenes: false },
  avatar: { enabled: false, port: 4488, character: "jason" },
  micInput: null,
  quickScenes: [],
  cameras: [],
};

const items = (scene: string) =>
  scene === "Pausa / baño"
    ? []
    : [
        { id: 1, name: "Cámara móvil", enabled: cams.movil, visual: true, kind: "dshow_input" },
        { id: 2, name: "Webcam", enabled: cams.webcam, visual: true, kind: "dshow_input" },
        { id: 3, name: "Marco", enabled: true, visual: true, kind: "image_source" },
        { id: 4, name: "Micro", enabled: true, visual: false, kind: "wasapi_input_capture" },
      ];
const cams = { movil: true, webcam: false };

const state: Snapshot = {
  connection: { state: mode === "offline" ? "reconnecting" : mode === "empty" ? "connecting" : "connected", error: mode === "offline" ? "No se pudo conectar con OBS en 127.0.0.1:4455 (Connection refused)" : null },
  scenes: mode === "full" ? ["Juego", "Charla", "Pausa / baño", "Intro", "Final"] : mode === "empty" ? [] : [],
  currentScene: mode === "full" ? "Juego" : null,
  previousScene: null,
  stream: { active: false, reconnecting: false },
  micMuted: mode === "full" ? false : null,
  cameras: [],
  sources: [],
  health: null,
  settings: structuredClone(mode === "empty" ? empty : full),
  hasPassword: false,
};
if (mode === "empty") state.connection = { state: "disconnected", error: null };

function refreshScene() {
  const live = state.connection.state === "connected";
  state.sources = live && state.currentScene ? items(state.currentScene) : [];
  state.cameras = state.settings.cameras.map((c) => {
    const item = state.sources.find((s) => s.name === c.source);
    return { ...c, inScene: !!item, active: !!item?.enabled };
  });
}
refreshScene();

const qr = (() => {
  let seed = 7;
  const rnd = () => ((seed = (seed * 1103515245 + 12345) & 0x7fffffff) / 0x7fffffff);
  let cells = "";
  for (let y = 0; y < 25; y++) for (let x = 0; x < 25; x++) if (rnd() > 0.5) cells += `<rect x="${x + 2}" y="${y + 2}" width="1" height="1"/>`;
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 29 29"><rect width="29" height="29" fill="#fff"/><g fill="#000">${cells}</g></svg>`;
})();

const now = () => Math.floor(Date.now() / 1000);
const panel: PanelStatus = {
  enabled: state.settings.panel.enabled,
  running: state.settings.panel.enabled,
  port: 8443,
  urls: state.settings.panel.enabled ? ["https://192.168.1.12:8443"] : [],
  error: null,
  hasPassword: mode === "full",
  qrSvg: state.settings.panel.enabled ? qr : null,
  devices:
    mode === "full"
      ? [
          { id: "d1", name: "Samsung de Miguel", createdAt: now() - 86400, lastSeen: now() - 30, lastIp: "192.168.1.50", online: true },
          { id: "d2", name: "Tablet salón", createdAt: now() - 400000, lastSeen: now() - 90000, lastIp: "192.168.1.61", online: false },
        ]
      : [],
  pairings: params.has("pairing") ? [{ id: "p1", name: "Android · Chrome", code: "482916", ip: "192.168.1.50", expiresAt: now() + 110 }] : [],
  audit: mode === "full"
    ? [
        { at: now() - 20, device: "Samsung de Miguel", ip: "192.168.1.50", action: "Escena", detail: "Pausa / baño", ok: true },
        { at: now() - 140, device: "Samsung de Miguel", ip: "192.168.1.50", action: "Micro", detail: "silenciar", ok: true },
        { at: now() - 600, device: "—", ip: "192.168.1.77", action: "Emparejamiento", detail: "Contraseña incorrecta", ok: false },
      ]
    : [],
};

const avatar: AvatarStatus = {
  enabled: state.settings.avatar.enabled,
  running: state.settings.avatar.enabled,
  port: 4488,
  url: "http://127.0.0.1:4488/avatar",
  error: null,
  clients: state.settings.avatar.enabled ? 1 : 0,
};

// ---- Capturas falsas ----
const colors: Record<string, [string, string]> = { "@program": ["#1d4ed8", "#0f172a"], "Cámara móvil": ["#0f766e", "#042f2e"], Webcam: ["#a16207", "#1c1917"], Marco: ["#7e22ce", "#1e1b4b"] };
function fakeFrame(key: string): string {
  const [a, b] = colors[key] ?? ["#475569", "#0f172a"];
  const t = (Date.now() / 1000) % 10;
  const label = key === "@program" ? (state.currentScene ?? "") : key;
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 180"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs><rect width="320" height="180" fill="url(#g)"/><circle cx="${40 + t * 24}" cy="90" r="22" fill="#fff" opacity=".18"/><text x="14" y="168" font-family="sans-serif" font-size="14" fill="#fff" opacity=".85">${label.replace(/[<&]/g, "")}</text></svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}
let interest: Interest = { program: false, sources: [] };
setInterval(() => {
  if (interest.program) emit("frame", { key: "@program", image: fakeFrame("@program") });
}, 400);
setInterval(() => {
  for (const s of interest.sources) if (state.sources.find((x) => x.name === s)?.enabled) emit("frame", { key: s, image: fakeFrame(s) });
}, 1000);

// ---- Salud ----
let bytes = 0;
setInterval(() => {
  if (state.connection.state !== "connected") return;
  if (state.stream.active) bytes += 1;
  state.health = {
    cpu: 8 + Math.round(Math.random() * 30) / 10,
    fps: 59.9,
    bitrateKbps: state.stream.active ? 5800 + Math.round(Math.random() * 300) : null,
    droppedFrames: state.stream.active ? 12 : 0,
    totalFrames: state.stream.active ? 1200 : 0,
    renderSkippedFrames: 0,
    renderTotalFrames: 5000,
    congestion: 0,
    durationSecs: state.stream.active ? 3720 + bytes * 2 : 0,
  };
  push();
}, 2000);

const push = () => emit("state", state);

// El estado de «sin conexión» pasa a conectado solo en el modo offline tras unos segundos, para ver la reconexión.
if (mode === "offline" && params.has("recover")) {
  setTimeout(() => {
    state.connection = { state: "connected", error: null };
    state.scenes = full.quickScenes.map((q) => q.scene).concat(["Juego"]);
    state.currentScene = "Juego";
    refreshScene();
    push();
  }, 6000);
}

function saveSettings(next: Settings) {
  if (next.avatar.port < 1024 && next.avatar.port !== 0) throw "El puerto del avatar debe ser 1024 o mayor";
  if (next.obs.host.trim() === "") throw "Falta la dirección de OBS";
  state.settings = structuredClone(next);
  panel.enabled = next.panel.enabled;
  panel.running = next.panel.enabled;
  panel.urls = next.panel.enabled ? [`https://192.168.1.12:${next.panel.port}`] : [];
  panel.qrSvg = next.panel.enabled ? qr : null;
  avatar.enabled = next.avatar.enabled;
  avatar.running = next.avatar.enabled;
  avatar.clients = next.avatar.enabled ? 1 : 0;
  refreshScene();
  push();
  emit("panel", panel);
  emit("avatar", avatar);
}

export async function invoke(cmd: string, args: Record<string, unknown> = {}): Promise<unknown> {
  await new Promise((r) => setTimeout(r, 30));
  switch (cmd) {
    case "get_snapshot": return state;
    case "get_panel_status": return panel;
    case "get_avatar_status": return avatar;
    case "list_inputs": return [{ name: "Micro", kind: "wasapi_input_capture" }, { name: "Escritorio", kind: "wasapi_output_capture" }, { name: "Micro del móvil", kind: "wasapi_input_capture" }] satisfies InputView[];
    case "launch_scene": {
      const scene = args.scene as string;
      if (!state.scenes.includes(scene)) throw "Esa escena no existe en OBS";
      const q = state.settings.quickScenes.find((x) => x.scene === scene);
      state.previousScene = state.currentScene;
      state.currentScene = scene;
      if (q?.muteMic && state.settings.micInput) state.micMuted = true;
      else if (state.micMuted && state.previousScene && state.settings.quickScenes.find((x) => x.scene === state.previousScene)?.muteMic) state.micMuted = false;
      refreshScene();
      push();
      return;
    }
    case "go_back": return invoke("launch_scene", { scene: state.previousScene });
    case "switch_camera": {
      const c = state.settings.cameras.find((x) => x.id === args.cameraId);
      if (!c) throw "Esa cámara ya no existe en los ajustes";
      cams.movil = c.id === "movil";
      cams.webcam = c.id === "webcam";
      refreshScene();
      push();
      return;
    }
    case "toggle_mic": state.micMuted = !state.micMuted; push(); return;
    case "start_stream": state.stream = { active: true, reconnecting: false }; push(); return;
    case "stop_stream": state.stream = { active: false, reconnecting: false }; push(); return;
    case "watch_previews": interest = args.interest as Interest; return;
    case "set_source_enabled": {
      const s = state.sources.find((x) => x.id === args.id);
      if (!s) throw "Esa fuente ya no está en la escena";
      if (s.name === "Cámara móvil") cams.movil = args.enabled as boolean;
      if (s.name === "Webcam") cams.webcam = args.enabled as boolean;
      s.enabled = args.enabled as boolean;
      refreshScene();
      push();
      return;
    }
    case "save_settings": saveSettings(args.settings as Settings); return;
    case "set_obs_password": state.hasPassword = !!args.password; push(); return;
    case "set_panel_password": panel.hasPassword = !!args.password; emit("panel", panel); return;
    case "revoke_device": panel.devices = panel.devices.filter((d) => d.id !== args.id); emit("panel", panel); return;
    case "avatar_push": return;
    case "create_avatar_source": return state.currentScene ?? "Juego";
    default: throw `Comando simulado desconocido: ${cmd}`;
  }
}
