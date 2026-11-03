// Conversión de lo que detecta la cámara o el micrófono a los valores del avatar.
// Son funciones puras, sin DOM, para poder probarlas con `npm test`.

export interface AvatarFrame {
  tracking: boolean;
  mouth: number;
  smile: number;
  blinkLeft: number;
  blinkRight: number;
  brow: number;
  yaw: number;
  pitch: number;
  roll: number;
}

export const IDLE_FRAME: AvatarFrame = {
  tracking: false,
  mouth: 0,
  smile: 0,
  blinkLeft: 0,
  blinkRight: 0,
  brow: 0,
  yaw: 0,
  pitch: 0,
  roll: 0,
};

export interface TrackingOptions {
  /** 0.5 (poco sensible) a 2 (muy sensible). */
  sensitivity: number;
  /** Como un espejo: tu lado izquierdo es el izquierdo de la pantalla. */
  mirror: boolean;
  /** Invierte el cabeceo (arriba/abajo) si el avatar mira al revés. */
  invertPitch: boolean;
}

export const DEFAULT_OPTIONS: TrackingOptions = { sensitivity: 1, mirror: true, invertPitch: false };

/** Calibración: la pose «neutra» del usuario, en grados, que se resta a la pose medida. */
export interface Pose {
  yaw: number;
  pitch: number;
  roll: number;
}

export const clamp = (v: number, lo: number, hi: number) => (Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : 0);
const avg = (a: number, b: number) => (a + b) / 2;

/** Ángulos de Euler (grados) de una matriz 4×4 de transformación de MediaPipe (orden de columnas, como OpenGL). */
export function eulerFromMatrix(m: ArrayLike<number>): Pose {
  const m11 = m[0], m12 = m[4], m13 = m[8];
  const m22 = m[5], m23 = m[9];
  const m32 = m[6], m33 = m[10];
  const y = Math.asin(clamp(m13, -1, 1));
  let x: number;
  let z: number;
  if (Math.abs(m13) < 0.9999999) {
    x = Math.atan2(-m23, m33);
    z = Math.atan2(-m12, m11);
  } else {
    x = Math.atan2(m32, m22);
    z = 0;
  }
  const deg = (r: number) => (r * 180) / Math.PI;
  const fin = (v: number) => (Number.isFinite(v) ? v : 0);
  return { pitch: fin(deg(x)), yaw: fin(deg(y)), roll: fin(deg(z)) };
}

/**
 * De los «blendshapes» de MediaPipe (nombre → 0..1) y la pose de la cabeza a un fotograma del avatar.
 * `shapes` usa los nombres de ARKit: jawOpen, mouthSmileLeft, eyeBlinkLeft, browInnerUp…
 */
export function faceToFrame(
  shapes: Record<string, number>,
  pose: Pose | null,
  calibration: Pose,
  opts: TrackingOptions = DEFAULT_OPTIONS,
): AvatarFrame {
  const s = (name: string) => shapes[name] ?? 0;
  const sens = clamp(opts.sensitivity, 0.3, 3);

  const mouth = clamp(((s("jawOpen") - 0.03) / 0.42) * sens, 0, 1);
  const smile = clamp((avg(s("mouthSmileLeft"), s("mouthSmileRight")) - avg(s("mouthFrownLeft"), s("mouthFrownRight"))) * 1.8, -1, 1);
  // El parpadeo se endurece para no dejar el ojo a medias por ruido de la cámara.
  const blink = (v: number) => clamp((v - 0.25) / 0.45, 0, 1);
  const left = blink(s("eyeBlinkLeft"));
  const right = blink(s("eyeBlinkRight"));
  const brow = clamp((s("browInnerUp") * 0.6 + avg(s("browOuterUpLeft"), s("browOuterUpRight")) * 0.4) * 1.6 - avg(s("browDownLeft"), s("browDownRight")) * 1.6, -1, 1);

  // Signos comprobados con imágenes sintéticas (dibujos del propio avatar girados, desplazados e inclinados)
  // pasadas por MediaPipe, sobre la imagen SIN reflejar de la cámara:
  //  · roll: gira en sentido horario cuando la imagen gira en sentido antihorario (cabeza inclinada hacia SU hombro derecho).
  //  · yaw: positivo cuando los rasgos se desplazan hacia la derecha de la imagen (gira hacia SU izquierda).
  //  · pitch: positivo cuando los rasgos bajan (mira abajo). Evidencia débil: se puede invertir en las opciones.
  //  · eyeBlinkLeft/Right: son los ojos del sujeto (el izquierdo del sujeto está a la derecha de la imagen).
  // En modo espejo el avatar se comporta como un espejo: si giras a tu izquierda, mira hacia la izquierda de la pantalla.
  const side = opts.mirror ? 1 : -1;
  const p = pose ?? { yaw: 0, pitch: 0, roll: 0 };
  return {
    tracking: true,
    mouth,
    smile,
    blinkLeft: opts.mirror ? left : right,
    blinkRight: opts.mirror ? right : left,
    brow,
    yaw: clamp((-(p.yaw - calibration.yaw) / 30) * side, -1, 1),
    pitch: clamp(((p.pitch - calibration.pitch) / 22) * (opts.invertPitch ? 1 : -1), -1, 1),
    roll: clamp((p.roll - calibration.roll) * side, -35, 35),
  };
}

/** Estado del modo voz: lo que hace falta recordar entre fotogramas. */
export interface VoiceState {
  /** Nivel de fondo estimado (ruido de la habitación). */
  floor: number;
  /** Nivel suavizado, para que la cabeza se mueva al hablar. */
  level: number;
}

export const newVoiceState = (): VoiceState => ({ floor: 0.004, level: 0 });

/**
 * Modo voz: el volumen del micrófono (RMS 0..1) abre la boca. El ruido de fondo se aprende solo
 * (baja deprisa y sube despacio), así que no hace falta ajustar un umbral.
 */
export function voiceToFrame(rms: number, state: VoiceState, blink: number, opts: TrackingOptions = DEFAULT_OPTIONS): AvatarFrame {
  const sens = clamp(opts.sensitivity, 0.3, 3);
  state.floor = rms < state.floor ? rms : state.floor + (rms - state.floor) * 0.002;
  const speech = Math.max(0, rms - state.floor * 1.6 - 0.004);
  // La raíz hace que las voces suaves también muevan la boca y las fuertes no la deformen.
  const mouth = clamp(Math.sqrt(speech * 14 * sens), 0, 1);
  state.level += (mouth - state.level) * 0.2;
  return {
    tracking: true,
    mouth,
    smile: 0.2,
    blinkLeft: blink,
    blinkRight: blink,
    brow: clamp(state.level * 0.35, 0, 1),
    yaw: 0,
    pitch: clamp(state.level * 0.12, 0, 1),
    roll: 0,
  };
}

/** Parpadeo automático para el modo voz: una curva suave cada 2 a 6 segundos. */
export class AutoBlink {
  private start = -1;
  private next: number;
  private rand: () => number;

  constructor(now: number, rand: () => number = Math.random) {
    this.rand = rand;
    this.next = now + 2200 + this.rand() * 3800;
  }

  /** Devuelve 0 (abierto) a 1 (cerrado). */
  at(now: number): number {
    if (this.start < 0 && now >= this.next) this.start = now;
    if (this.start < 0) return 0;
    const k = (now - this.start) / 160;
    if (k >= 1) {
      this.start = -1;
      this.next = now + 2200 + this.rand() * 3800;
      return 0;
    }
    return Math.sin(k * Math.PI);
  }
}

/** ¿Cambió lo bastante como para valer la pena enviarlo? Evita inundar de mensajes idénticos. */
export function differs(a: AvatarFrame, b: AvatarFrame, eps = 0.01): boolean {
  if (a.tracking !== b.tracking) return true;
  return (
    Math.abs(a.mouth - b.mouth) > eps ||
    Math.abs(a.smile - b.smile) > eps ||
    Math.abs(a.blinkLeft - b.blinkLeft) > eps ||
    Math.abs(a.blinkRight - b.blinkRight) > eps ||
    Math.abs(a.brow - b.brow) > eps ||
    Math.abs(a.yaw - b.yaw) > eps ||
    Math.abs(a.pitch - b.pitch) > eps ||
    Math.abs(a.roll - b.roll) > eps * 10
  );
}
