import type { TrackerState } from "./avatar/tracker";
import type { AvatarStatus, PanelStatus, Snapshot } from "./api";

export type Tab = "live" | "avatar" | "mobile" | "settings";

/** Estado compartido por todas las pestañas. OBS sigue siendo la fuente de verdad: esto es solo un reflejo. */
export const state = {
  snap: null as Snapshot | null,
  panel: null as PanelStatus | null,
  avatar: null as AvatarStatus | null,
  version: "",
  tab: "live" as Tab,
  tracker: { kind: "off" } as TrackerState,
  /** Últimas capturas recibidas, para que al repintar no parpadee la imagen. */
  frames: new Map<string, string>(),
};

type Listener = () => void;
const listeners = new Set<Listener>();
export const subscribe = (fn: Listener) => void listeners.add(fn);
export const notify = () => listeners.forEach((fn) => fn());

export const connected = () => state.snap?.connection.state === "connected";

export function goTab(tab: Tab) {
  state.tab = tab;
  notify();
}
