import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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

export interface Settings {
  obs: { host: string; port: number };
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

export interface InputView {
  name: string;
  kind: string;
}

export const api = {
  getSnapshot: () => invoke<Snapshot>("get_snapshot"),
  launchScene: (scene: string) => invoke<void>("launch_scene", { scene }),
  goBack: () => invoke<void>("go_back"),
  switchCamera: (cameraId: string) => invoke<void>("switch_camera", { cameraId }),
  toggleMic: () => invoke<void>("toggle_mic"),
  startStream: () => invoke<void>("start_stream"),
  stopStream: () => invoke<void>("stop_stream"),
  watchPreviews: (interest: Interest) => invoke<void>("watch_previews", { interest }),
  setSourceEnabled: (id: number, enabled: boolean) => invoke<void>("set_source_enabled", { id, enabled }),
  listInputs: () => invoke<InputView[]>("list_inputs"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  setObsPassword: (password: string | null) => invoke<void>("set_obs_password", { password }),
  onFrame: (handler: (f: { key: string; image: string }) => void) =>
    listen<{ key: string; image: string }>("frame", (e) => handler(e.payload)),
  onState: (handler: (s: Snapshot) => void) => listen<Snapshot>("state", (e) => handler(e.payload)),
};
