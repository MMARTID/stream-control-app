export const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

/** Todo texto que viene de OBS o de los ajustes pasa por aquí antes de ir a innerHTML. */
export const esc = (s: string) =>
  s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

const pad = (n: number) => String(n).padStart(2, "0");
export const clock = (secs: number) => `${pad(Math.floor(secs / 3600))}:${pad(Math.floor((secs % 3600) / 60))}:${pad(secs % 60)}`;
export const bitrate = (kbps: number) => (kbps >= 1000 ? `${(kbps / 1000).toFixed(1).replace(".", ",")} Mb/s` : `${kbps} kb/s`);
export const when = (secs: number) => new Date(secs * 1000).toLocaleString("es-ES", { dateStyle: "short", timeStyle: "medium" });

const signatures = new Map<string, string>();

/** Repinta un bloque solo si lo que lo define ha cambiado: así no se pierde el foco ni parpadean las imágenes. */
export function paint(id: string, signature: unknown, html: () => string) {
  const sig = JSON.stringify(signature);
  if (signatures.get(id) === sig) return;
  signatures.set(id, sig);
  $(id).innerHTML = html();
}

/** Como `paint`, pero la firma es el propio HTML sin las imágenes de las capturas (esas se actualizan aparte). */
export function paintHtml(id: string, html: string) {
  paint(id, html.replace(/ src="data:[^"]*"/g, ""), () => html);
}

export const repaintAll = () => signatures.clear();

/** Llama a `fn` como mucho cada `ms`, y al final con el último valor (para deslizadores y selectores de color). */
export function throttle<A extends unknown[]>(fn: (...args: A) => void, ms: number) {
  let last = 0;
  let timer: number | undefined;
  let pending: A | null = null;
  return (...args: A) => {
    const now = Date.now();
    if (now - last >= ms) {
      last = now;
      fn(...args);
      return;
    }
    pending = args;
    if (timer === undefined) {
      timer = window.setTimeout(() => {
        timer = undefined;
        last = Date.now();
        if (pending) fn(...pending);
        pending = null;
      }, ms - (now - last));
    }
  };
}

let toastTimer: number | undefined;
export function toast(message: string, kind: "error" | "ok" = "error") {
  const el = $("toast");
  el.textContent = message;
  el.className = `toast ${kind}`;
  el.hidden = false;
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (el.hidden = true), kind === "ok" ? 3500 : 7000);
}

/** Ejecuta una acción y muestra el error, si lo hay, sin ventanas emergentes (RNF-6). */
export async function run<T>(action: Promise<T>): Promise<T | undefined> {
  try {
    return await action;
  } catch (e) {
    toast(String(e instanceof Error ? e.message : e));
    return undefined;
  }
}

export function store<T>(key: string, fallback: T): { get(): T; set(v: T): void } {
  return {
    get() {
      try {
        const raw = localStorage.getItem(key);
        return raw ? { ...fallback, ...JSON.parse(raw) } : fallback;
      } catch {
        return fallback;
      }
    },
    set(v) {
      try {
        localStorage.setItem(key, JSON.stringify(v));
      } catch {
        /* sin almacenamiento: se pierde la preferencia, nada más */
      }
    },
  };
}
