// Seguimiento de la cara (cámara) o de la voz (micrófono) para animar el avatar. Todo en local:
// el modelo de MediaPipe va dentro de la app y no se envía ninguna imagen ni audio a ningún sitio.
import type { FaceLandmarker } from "@mediapipe/tasks-vision";
import {
  AutoBlink,
  DEFAULT_OPTIONS,
  differs,
  eulerFromMatrix,
  faceToFrame,
  IDLE_FRAME,
  newVoiceState,
  voiceToFrame,
  type AvatarFrame,
  type Pose,
  type TrackingOptions,
} from "./mapping";

export type TrackerMode = "camera" | "voice";
export type TrackerState =
  | { kind: "off" }
  | { kind: "starting"; detail: string }
  | { kind: "tracking" } // cámara: se ve la cara · voz: se escucha el micro
  | { kind: "no-face" }
  | { kind: "error"; message: string };

export interface TrackerConfig {
  mode: TrackerMode;
  /** `deviceId` de la cámara o del micro; vacío = el predeterminado. */
  deviceId: string;
  options: TrackingOptions;
}

const MODEL_URL = "/mediapipe/face_landmarker.task";
const WASM_DIR = "/mediapipe/wasm";
const FRAME_MS = 33;
const NO_FACE_AFTER_MS = 700;
const HEARTBEAT_MS = 500;

/** Dispositivos de entrada que ve el navegador. Los nombres solo aparecen tras dar permiso. */
export async function listDevices(kind: "videoinput" | "audioinput"): Promise<{ id: string; label: string }[]> {
  const all = await navigator.mediaDevices.enumerateDevices();
  return all.filter((d) => d.kind === kind).map((d, i) => ({ id: d.deviceId, label: d.label || (kind === "videoinput" ? `Cámara ${i + 1}` : `Micrófono ${i + 1}`) }));
}

/** Pide permiso (y suelta el dispositivo enseguida) para que `listDevices` devuelva los nombres. */
export async function askPermission(kind: "video" | "audio"): Promise<void> {
  const stream = await navigator.mediaDevices.getUserMedia(kind === "video" ? { video: true } : { audio: true });
  stream.getTracks().forEach((t) => t.stop());
}

export function friendlyError(e: unknown, mode: TrackerMode): string {
  const name = e instanceof DOMException ? e.name : "";
  const what = mode === "camera" ? "la cámara" : "el micrófono";
  if (name === "NotAllowedError" || name === "SecurityError") return `No hay permiso para usar ${what}. Permítelo en Windows (Configuración → Privacidad) y en el aviso de la app.`;
  if (name === "NotFoundError" || name === "OverconstrainedError") return `No se encuentra ${what}. Comprueba que está conectado o elige otro.`;
  if (name === "NotReadableError" || name === "AbortError")
    return `${mode === "camera" ? "La cámara" : "El micrófono"} está en uso por otra aplicación (¿OBS?). Cierra esa fuente en OBS, usa otra cámara o cambia al modo voz.`;
  return e instanceof Error ? e.message : String(e);
}

export class AvatarTracker {
  state: TrackerState = { kind: "off" };
  /** Pose neutra medida con «Calibrar». */
  calibration: Pose = { yaw: 0, pitch: 0, roll: 0 };

  private stream: MediaStream | null = null;
  private video: HTMLVideoElement | null = null;
  private landmarker: FaceLandmarker | null = null;
  private audio: { ctx: AudioContext; analyser: AnalyserNode; buf: Float32Array<ArrayBuffer> } | null = null;
  private timer = 0;
  private running = false;
  private releaseLock: (() => void) | null = null;
  private lastPose: Pose = { yaw: 0, pitch: 0, roll: 0 };
  private lastFaceAt = 0;
  private lastSent: AvatarFrame = IDLE_FRAME;
  private lastSentAt = 0;
  private config: TrackerConfig = { mode: "voice", deviceId: "", options: DEFAULT_OPTIONS };

  constructor(
    private send: (frame: AvatarFrame) => void,
    private onState: (state: TrackerState) => void,
  ) {}

  get active() {
    return this.running;
  }

  setOptions(options: TrackingOptions) {
    this.config = { ...this.config, options };
  }

  /** Toma la pose actual como «mirando al frente». */
  calibrate() {
    this.calibration = { ...this.lastPose };
  }

  private set(state: TrackerState) {
    this.state = state;
    this.onState(state);
  }

  async start(config: TrackerConfig): Promise<void> {
    await this.stop();
    this.config = config;
    this.running = true;
    this.set({ kind: "starting", detail: config.mode === "camera" ? "Abriendo la cámara…" : "Abriendo el micrófono…" });
    try {
      // Mientras se sigue, la ventana no debe dormirse aunque se minimice (WebView2 frena los temporizadores).
      void navigator.locks?.request("stream-control-tracker", { mode: "shared" }, () => new Promise<void>((resolve) => (this.releaseLock = resolve)));
      if (config.mode === "camera") await this.startCamera(config);
      else await this.startVoice(config);
    } catch (e) {
      await this.stop();
      this.set({ kind: "error", message: friendlyError(e, config.mode) });
    }
  }

  async stop(): Promise<void> {
    const wasRunning = this.running;
    this.running = false;
    window.clearTimeout(this.timer);
    this.releaseLock?.();
    this.releaseLock = null;
    this.stream?.getTracks().forEach((t) => t.stop());
    this.stream = null;
    if (this.video) {
      this.video.srcObject = null;
      this.video = null;
    }
    this.landmarker?.close();
    this.landmarker = null;
    if (this.audio) {
      void this.audio.ctx.close();
      this.audio = null;
    }
    if (wasRunning || this.state.kind !== "off") {
      this.emit({ ...IDLE_FRAME }, true);
      if (this.state.kind !== "error") this.set({ kind: "off" });
    }
  }

  private emit(frame: AvatarFrame, force = false) {
    const now = performance.now();
    if (!force && !differs(frame, this.lastSent) && now - this.lastSentAt < HEARTBEAT_MS) return;
    this.lastSent = frame;
    this.lastSentAt = now;
    this.send(frame);
  }

  // ---- Cámara ----

  private async startCamera(config: TrackerConfig) {
    const video: MediaTrackConstraints = { width: { ideal: 640 }, height: { ideal: 480 }, frameRate: { ideal: 30 } };
    if (config.deviceId) video.deviceId = { exact: config.deviceId };
    this.stream = await navigator.mediaDevices.getUserMedia({ video, audio: false });

    const el = document.createElement("video");
    el.muted = true;
    el.playsInline = true;
    el.srcObject = this.stream;
    await el.play();
    this.video = el;

    this.set({ kind: "starting", detail: "Cargando el modelo de reconocimiento (en local)…" });
    this.landmarker = await this.createLandmarker();
    if (!this.running) return;
    this.lastFaceAt = performance.now();
    this.set({ kind: "no-face" });
    this.loopCamera();
  }

  private async createLandmarker(): Promise<FaceLandmarker> {
    const { FaceLandmarker, FilesetResolver } = await import("@mediapipe/tasks-vision");
    const fileset = await FilesetResolver.forVisionTasks(WASM_DIR);
    const make = (delegate: "GPU" | "CPU") =>
      FaceLandmarker.createFromOptions(fileset, {
        baseOptions: { modelAssetPath: MODEL_URL, delegate },
        runningMode: "VIDEO",
        numFaces: 1,
        outputFaceBlendshapes: true,
        outputFacialTransformationMatrixes: true,
      });
    try {
      return await make("GPU");
    } catch {
      return await make("CPU");
    }
  }

  private loopCamera = () => {
    if (!this.running || !this.video || !this.landmarker) return;
    const started = performance.now();
    let delay = FRAME_MS;
    try {
      if (this.video.readyState >= 2) {
        const result = this.landmarker.detectForVideo(this.video, started);
        const categories = result.faceBlendshapes?.[0]?.categories;
        if (categories?.length) {
          const shapes: Record<string, number> = {};
          for (const c of categories) shapes[c.categoryName] = c.score;
          const matrix = result.facialTransformationMatrixes?.[0]?.data;
          const pose = matrix ? eulerFromMatrix(matrix) : null;
          if (pose) this.lastPose = pose;
          this.lastFaceAt = started;
          if (this.state.kind !== "tracking") this.set({ kind: "tracking" });
          this.emit(faceToFrame(shapes, pose, this.calibration, this.config.options));
        } else if (started - this.lastFaceAt > NO_FACE_AFTER_MS) {
          if (this.state.kind !== "no-face") this.set({ kind: "no-face" });
          this.emit({ ...IDLE_FRAME });
        }
      }
    } catch (e) {
      this.set({ kind: "error", message: e instanceof Error ? e.message : String(e) });
      void this.stop();
      return;
    }
    // Si la detección tarda más que un fotograma, se descuenta del siguiente aviso.
    delay = Math.max(4, FRAME_MS - (performance.now() - started));
    this.timer = window.setTimeout(this.loopCamera, delay);
  };

  // ---- Voz ----

  private async startVoice(config: TrackerConfig) {
    const audio: MediaTrackConstraints = { echoCancellation: false, noiseSuppression: false, autoGainControl: false };
    if (config.deviceId) audio.deviceId = { exact: config.deviceId };
    this.stream = await navigator.mediaDevices.getUserMedia({ audio, video: false });
    const ctx = new AudioContext();
    const source = ctx.createMediaStreamSource(this.stream);
    const analyser = ctx.createAnalyser();
    analyser.fftSize = 1024;
    source.connect(analyser);
    this.audio = { ctx, analyser, buf: new Float32Array(analyser.fftSize) };
    if (ctx.state === "suspended") await ctx.resume();
    this.set({ kind: "tracking" });
    const voice = newVoiceState();
    const blink = new AutoBlink(performance.now());
    const tick = () => {
      if (!this.running || !this.audio) return;
      const { analyser, buf } = this.audio;
      analyser.getFloatTimeDomainData(buf);
      let sum = 0;
      for (const v of buf) sum += v * v;
      const rms = Math.sqrt(sum / buf.length);
      this.emit(voiceToFrame(rms, voice, blink.at(performance.now()), this.config.options));
      this.timer = window.setTimeout(tick, FRAME_MS);
    };
    tick();
  }
}
