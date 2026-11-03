// Pestaña «Luces»: busca tiras LED Bluetooth (protocolo Lotus Lamp), las conecta y las controla.
import { api, type LightDevice } from "../api";
import { state } from "../state";
import { $, esc, paint, paintHtml, run, throttle, toast } from "../util";

const SWATCHES: [string, number, number, number][] = [
  ["Rojo", 255, 0, 0],
  ["Naranja", 255, 120, 0],
  ["Amarillo", 255, 220, 0],
  ["Verde", 0, 220, 60],
  ["Cian", 0, 220, 220],
  ["Azul", 0, 70, 255],
  ["Morado", 150, 0, 255],
  ["Rosa", 255, 60, 160],
  ["Blanco cálido", 255, 170, 90],
  ["Blanco", 255, 255, 255],
];

const CATEGORIES: Record<string, string> = {
  basic: "Básicos",
  trans: "Transiciones",
  tail: "Estela",
  water: "Agua",
  curtain: "Cortina",
  run: "Carrera →",
  runback: "Carrera ←",
  flow: "Respiración",
};

const hex = (n: number) => n.toString(16).padStart(2, "0");
const toHex = (c: [number, number, number]) => `#${hex(c[0])}${hex(c[1])}${hex(c[2])}`;

const sendColor = throttle((r: number, g: number, b: number) => void run(api.lightsColor(r, g, b)), 120);
const sendBrightness = throttle((v: number) => void run(api.lightsBrightness(v)), 120);
const sendSpeed = throttle((v: number) => void run(api.lightsSpeed(v)), 120);

let root: HTMLElement;

export function mountLights(el: HTMLElement) {
  root = el;
  root.innerHTML = `
    <div class="page-head"><h1>Luces</h1>
      <p>Controla tus tiras LED Bluetooth desde la app. Funciona con las tiras de la app <b>Lotus Lamp X</b> (las que se anuncian como «MELK-…»). Todo va por Bluetooth directo del PC: sin nube.</p></div>
    <section class="card" id="l-find"></section>
    <section class="card" id="l-ctl" hidden>
      <h3><span class="num">2</span> Controla la tira <span class="chip" id="l-name"></span></h3>
      <div class="row">
        <button data-action="power" data-on="true" class="primary">Encender</button>
        <button data-action="power" data-on="false">Apagar</button>
      </div>

      <h4>Color</h4>
      <div class="row">
        <input type="color" id="l-color" value="#ff3c00" aria-label="Color" />
        <div class="swatches">${SWATCHES.map(([name, r, g, b]) => `<button class="swatch" title="${name}" aria-label="${name}" data-action="swatch" data-rgb="${r},${g},${b}" style="--c:rgb(${r},${g},${b})"></button>`).join("")}</div>
      </div>
      <p class="hint">La tira es de una sola zona: todos los LED toman el mismo color.</p>

      <div class="row">
        <label class="field grow"><span>Brillo</span><input type="range" id="l-bright" min="0" max="100" value="100" /></label>
        <label class="field grow"><span>Velocidad de la animación</span><input type="range" id="l-speed" min="0" max="100" value="50" /></label>
      </div>

      <h4>Animaciones <small id="l-count"></small></h4>
      <div class="row">
        <input type="search" id="l-search" placeholder="Buscar por nombre (p. ej. cyan, flow)" autocomplete="off" class="grow" />
        <select id="l-cat" aria-label="Categoría"><option value="">Todas las categorías</option>${Object.entries(CATEGORIES).map(([k, v]) => `<option value="${k}">${v}</option>`).join("")}</select>
      </div>
      <div id="l-modes" class="modes"></div>
    </section>`;

  root.addEventListener("click", (e) => {
    const el = (e.target as HTMLElement).closest<HTMLElement>("[data-action]");
    if (!el) return;
    switch (el.dataset.action) {
      case "scan": return void run(api.lightsScan());
      case "connect": return void connect(el.dataset.id!);
      case "disconnect": return void run(api.lightsDisconnect());
      case "power": return void run(api.lightsPower(el.dataset.on === "true"));
      case "swatch": {
        const [r, g, b] = el.dataset.rgb!.split(",").map(Number);
        $<HTMLInputElement>("l-color").value = toHex([r, g, b]);
        return void run(api.lightsColor(r, g, b));
      }
      case "mode": return void run(api.lightsMode(Number(el.dataset.mode)));
    }
  });
  root.addEventListener("input", (e) => {
    const t = e.target as HTMLInputElement;
    switch (t.id) {
      case "l-color": {
        const v = t.value;
        return sendColor(parseInt(v.slice(1, 3), 16), parseInt(v.slice(3, 5), 16), parseInt(v.slice(5, 7), 16));
      }
      case "l-bright": return sendBrightness(Number(t.value));
      case "l-speed": return sendSpeed(Number(t.value));
      case "l-search": return renderModes();
    }
  });
  root.addEventListener("change", (e) => {
    const t = e.target as HTMLInputElement | HTMLSelectElement;
    if (t.id === "l-cat") renderModes();
    if (t.dataset.bind === "auto") void run(api.lightsAutoConnect((t as HTMLInputElement).checked));
  });
}

async function connect(id: string) {
  const ok = await run(api.lightsConnect(id).then(() => true));
  if (ok) toast("Tira conectada", "ok");
}

const signal = (rssi: number | null) => (rssi == null ? "" : rssi > -55 ? "señal muy buena" : rssi > -75 ? "señal buena" : "lejos");

function deviceRow(d: LightDevice, busy: boolean) {
  return `<div class="device-row">
    <span class="grow"><b>${esc(d.name)}</b><small>${d.likely ? "Parece una tira Lotus Lamp" : "Otro dispositivo Bluetooth"}${signal(d.rssi) ? " · " + signal(d.rssi) : ""}</small></span>
    <button class="${d.likely ? "primary" : ""}" data-action="connect" data-id="${esc(d.id)}" ${busy ? "disabled" : ""}>Conectar</button>
  </div>`;
}

function renderFind() {
  const l = state.lights;
  const s = state.snap;
  if (!l) return;
  const auto = !!s?.settings.lights.autoConnect;
  paint("l-find", { l: { ...l, power: null, color: null, brightness: null, speed: null, mode: null }, auto }, () => {
    const busy = l.scanning || l.connecting;
    const likely = l.devices.filter((d) => d.likely);
    const others = l.devices.filter((d) => !d.likely);
    const bluetooth =
      l.adapter === "off"
        ? `<p class="status bad">El Bluetooth de este equipo está apagado. Enciéndelo y vuelve a buscar.</p>`
        : l.adapter === "missing"
          ? `<p class="status bad">Este equipo no tiene Bluetooth (o no se puede usar). Hace falta un adaptador Bluetooth 4.0 o superior.</p>`
          : "";
    const found = l.connected
      ? `<div class="device-row"><span class="dot on"></span><span class="grow"><b>${esc(l.connected.name)}</b><small>Conectada</small></span>
          <button data-action="disconnect">Desconectar</button></div>
         <label class="check-inline"><input type="checkbox" data-bind="auto" ${auto ? "checked" : ""} /> Conectarse sola a esta tira al abrir la app</label>`
      : l.devices.length === 0
        ? `<p class="hint">${l.scanning ? "Buscando dispositivos cerca (unos 5 segundos)…" : "Pulsa «Buscar tiras» con la tira encendida y cerca del PC."}</p>`
        : `${likely.map((d) => deviceRow(d, busy)).join("") || `<p class="hint">No se ha visto ninguna tira «MELK-…». Comprueba que está encendida, cerca, y que <b>la app Lotus Lamp X del móvil está cerrada</b> (la tira solo admite una conexión).</p>`}
           ${others.length ? `<details><summary>Otros dispositivos Bluetooth (${others.length})</summary>${others.map((d) => deviceRow(d, busy)).join("")}<p class="hint">Si tu tira usa otro nombre, pruébala aquí. Si no es de esta familia, la app lo dirá al conectar.</p></details>` : ""}`;
    return `
      <h3><span class="num">1</span> Busca tu tira</h3>
      <p>Enciende la tira, acércala al PC y <b>cierra la app Lotus Lamp X del móvil</b>: la tira solo admite una conexión a la vez.</p>
      ${bluetooth}
      <div class="row"><button class="primary" data-action="scan" ${busy ? "disabled" : ""}>${l.scanning ? "Buscando…" : l.connecting ? "Conectando…" : "Buscar tiras"}</button></div>
      ${l.error ? `<p class="err">${esc(l.error)}</p>` : ""}
      ${found}`;
  });
}

function renderModes() {
  const l = state.lights;
  if (!l?.connected) return;
  const q = $<HTMLInputElement>("l-search").value.trim().toLowerCase();
  const cat = $<HTMLSelectElement>("l-cat").value;
  const list = state.lightModes.filter((m) => (!cat || m.category === cat) && (!q || m.name.toLowerCase().includes(q)));
  $("l-count").textContent = `${list.length} de ${state.lightModes.length}`;
  paintHtml(
    "l-modes",
    list.length
      ? list.map((m) => `<button class="mode ${l.mode === m.id ? "on" : ""}" data-action="mode" data-mode="${m.id}" title="${esc(CATEGORIES[m.category] ?? m.category)}">${esc(m.name)}</button>`).join("")
      : `<p class="hint">Ninguna animación coincide.</p>`,
  );
}

export function updateLights() {
  const l = state.lights;
  if (!l) return;
  renderFind();
  const ctl = $("l-ctl");
  ctl.hidden = !l.connected;
  if (!l.connected) return;
  $("l-name").textContent = l.connected.name;
  // Los controles siguen lo último que se pidió, salvo el que se está arrastrando ahora.
  const set = (id: string, value: string) => {
    const el = $<HTMLInputElement>(id);
    if (document.activeElement !== el && el.value !== value) el.value = value;
  };
  if (l.color) set("l-color", toHex(l.color));
  if (l.brightness != null) set("l-bright", String(l.brightness));
  if (l.speed != null) set("l-speed", String(l.speed));
  renderModes();
}

