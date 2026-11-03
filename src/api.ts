import { getVersion } from "@tauri-apps/api/app";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import type { AvatarFrame } from "./avatar/mapping";

export type { AvatarFrame };
export type ConnState = "disconnected" | "connecting" | "connected" | "reconnecting";

export interface QuickScene {
  scene: string;
  label: string;
  color: string;
  icon: string;
  muteMic: boolean;
}

export interface CameraConfig {
  id: string;
  name: string;
  source: string;
}

export interface PanelSettings {
  enabled: boolean;
  port: number;
  allowStreamControl: boolean;
  onlyQuickScenes: boolean;
}

export interface AvatarSettings {
  enabled: boolean;
  port: number;
  character: string;
  /** Escena de OBS en la que el avatar sustituye a la cámara. */
  scene: string;
}

export interface AvatarSceneReport {
  scene: string;
  copied: number;
  failed: string[];
}

export interface Settings {
  obs: { host: string; port: number };
  panel: PanelSettings;
  avatar: AvatarSettings;
  micInput: string | null;
  quickScenes: QuickScene[];
  cameras: CameraConfig[];
}

export interface CameraView extends CameraConfig {
  inScene: boolean;
  active: boolean;
}

export interface SourceView {
  id: number;
  name: string;
  enabled: boolean;
  visual: boolean;
  kind: string | null;
}

export interface Health {
  cpu: number;
  fps: number;
  bitrateKbps: number | null;
  droppedFrames: number;
  totalFrames: number;
  renderSkippedFrames: number;
  renderTotalFrames: number;
  congestion: number;
  durationSecs: number;
}

export interface Interest {
  program: boolean;
  sources: string[];
}

export interface Snapshot {
  connection: { state: ConnState; error: string | null };
  scenes: string[];
  currentScene: string | null;
  previousScene: string | null;
  stream: { active: boolean; reconnecting: boolean };
  micMuted: boolean | null;
  cameras: CameraView[];
  sources: SourceView[];
  health: Health | null;
  settings: Settings;
  hasPassword: boolean;
}

export interface PanelDevice {
  id: string;
  name: string;
  createdAt: number;
  lastSeen: number;
  lastIp: string;
  online: boolean;
}

export interface PanelPairing {
  id: string;
  name: string;
  code: string;
  ip: string;
  expiresAt: number;
}

export interface PanelAuditEntry {
  at: number;
  device: string;
  ip: string;
  action: string;
  detail: string;
  ok: boolean;
}

export interface PanelStatus {
  enabled: boolean;
  running: boolean;
  port: number;
  urls: string[];
  error: string | null;
  hasPassword: boolean;
  qrSvg: string | null;
  devices: PanelDevice[];
  pairings: PanelPairing[];
  audit: PanelAuditEntry[];
}

export interface AvatarStatus {
  enabled: boolean;
  running: boolean;
  port: number;
  url: string;
  error: string | null;
  clients: number;
}

export interface InputView {
  name: string;
  kind: string;
}

/** `true` dentro de la app de escritorio. Fuera (navegador de desarrollo) se usa un OBS simulado en `mock.ts`. */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

let mockModule: Promise<typeof import("./mock")> | null = null;
const mock = () => (mockModule ??= import("./mock"));

function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return inTauri ? tauriInvoke<T>(cmd, args) : mock().then((m) => m.invoke(cmd, args) as Promise<T>);
}

async function listen<T>(event: string, handler: (payload: T) => void): Promise<void> {
  if (inTauri) {
    await tauriListen<T>(event, (e) => handler(e.payload));
  } else {
    (await mock()).listen(event, handler as (p: unknown) => void);
  }
}

export const api = {
  version: () => (inTauri ? getVersion() : Promise.resolve("dev")),
  getSnapshot: () => call<Snapshot>("get_snapshot"),
  launchScene: (scene: string) => call<void>("launch_scene", { scene }),
  goBack: () => call<void>("go_back"),
  switchCamera: (cameraId: string) => call<void>("switch_camera", { cameraId }),
  toggleMic: () => call<void>("toggle_mic"),
  startStream: () => call<void>("start_stream"),
  stopStream: () => call<void>("stop_stream"),
  watchPreviews: (interest: Interest) => call<void>("watch_previews", { interest }),
  setSourceEnabled: (id: number, enabled: boolean) => call<void>("set_source_enabled", { id, enabled }),
  listInputs: () => call<InputView[]>("list_inputs"),
  saveSettings: (settings: Settings) => call<void>("save_settings", { settings }),
  setObsPassword: (password: string | null) => call<void>("set_obs_password", { password }),
  getPanelStatus: () => call<PanelStatus>("get_panel_status"),
  setPanelPassword: (password: string | null) => call<void>("set_panel_password", { password }),
  revokeDevice: (id: string) => call<void>("revoke_device", { id }),
  getAvatarStatus: () => call<AvatarStatus>("get_avatar_status"),
  avatarPush: (frame: AvatarFrame) => call<void>("avatar_push", { frame }),
  createAvatarSource: () => call<string>("create_avatar_source"),
  createAvatarScene: (copySources: boolean, addQuick: boolean) => call<AvatarSceneReport>("create_avatar_scene", { copySources, addQuick }),
  onState: (h: (s: Snapshot) => void) => listen("state", h),
  onFrame: (h: (f: { key: string; image: string }) => void) => listen("frame", h),
  onPanel: (h: (s: PanelStatus) => void) => listen("panel", h),
  onAvatar: (h: (s: AvatarStatus) => void) => listen("avatar", h),
};
