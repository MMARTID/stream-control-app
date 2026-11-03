// Pruebas de las funciones puras del seguimiento. Se ejecutan con `npm test` (node --test).
import assert from "node:assert/strict";
import { test } from "node:test";
import { AutoBlink, DEFAULT_OPTIONS, differs, eulerFromMatrix, faceToFrame, IDLE_FRAME, newVoiceState, voiceToFrame } from "./mapping.ts";

const ZERO = { yaw: 0, pitch: 0, roll: 0 };
const near = (a: number, b: number, eps = 1e-6) => assert.ok(Math.abs(a - b) < eps, `${a} no está cerca de ${b}`);

// Matriz de rotación sobre un eje, en orden de columnas (4×4).
const rotY = (deg: number) => {
  const c = Math.cos((deg * Math.PI) / 180), s = Math.sin((deg * Math.PI) / 180);
  return [c, 0, -s, 0, 0, 1, 0, 0, s, 0, c, 0, 0, 0, 0, 1];
};
const rotX = (deg: number) => {
  const c = Math.cos((deg * Math.PI) / 180), s = Math.sin((deg * Math.PI) / 180);
  return [1, 0, 0, 0, 0, c, s, 0, 0, -s, c, 0, 0, 0, 0, 1];
};
const rotZ = (deg: number) => {
  const c = Math.cos((deg * Math.PI) / 180), s = Math.sin((deg * Math.PI) / 180);
  return [c, s, 0, 0, -s, c, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
};

test("la matriz identidad no gira nada", () => {
  const p = eulerFromMatrix([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
  near(p.yaw, 0); near(p.pitch, 0); near(p.roll, 0);
});

test("recupera giros sueltos sobre cada eje", () => {
  near(eulerFromMatrix(rotY(25)).yaw, 25);
  near(eulerFromMatrix(rotX(-15)).pitch, -15);
  near(eulerFromMatrix(rotZ(10)).roll, 10);
  const p = eulerFromMatrix(rotY(-40));
  near(p.yaw, -40); near(p.pitch, 0); near(p.roll, 0);
});

test("una matriz degenerada no produce NaN", () => {
  const p = eulerFromMatrix(new Array(16).fill(NaN));
  assert.ok([p.yaw, p.pitch, p.roll].every(Number.isFinite));
  const q = eulerFromMatrix(new Array(16).fill(0));
  assert.ok([q.yaw, q.pitch, q.roll].every(Number.isFinite));
});

test("la boca se abre con jawOpen y se cierra con la cara en reposo", () => {
  assert.equal(faceToFrame({}, null, ZERO).mouth, 0);
  assert.equal(faceToFrame({ jawOpen: 0.02 }, null, ZERO).mouth, 0);
  const half = faceToFrame({ jawOpen: 0.24 }, null, ZERO).mouth;
  assert.ok(half > 0.4 && half < 0.6, String(half));
  assert.equal(faceToFrame({ jawOpen: 1 }, null, ZERO).mouth, 1);
});

test("la sensibilidad escala la apertura de la boca", () => {
  const low = faceToFrame({ jawOpen: 0.2 }, null, ZERO, { ...DEFAULT_OPTIONS, sensitivity: 0.5 }).mouth;
  const high = faceToFrame({ jawOpen: 0.2 }, null, ZERO, { ...DEFAULT_OPTIONS, sensitivity: 2 }).mouth;
  assert.ok(high > low * 2);
});

test("sonrisa y ceño fruncido de la boca", () => {
  assert.ok(faceToFrame({ mouthSmileLeft: 0.5, mouthSmileRight: 0.5 }, null, ZERO).smile > 0.5);
  assert.ok(faceToFrame({ mouthFrownLeft: 0.5, mouthFrownRight: 0.5 }, null, ZERO).smile < -0.5);
});

test("el parpadeo ignora el ruido pequeño y cierra del todo con valores altos", () => {
  assert.equal(faceToFrame({ eyeBlinkLeft: 0.15 }, null, ZERO).blinkLeft, 0);
  assert.equal(faceToFrame({ eyeBlinkLeft: 0.8 }, null, ZERO).blinkLeft, 1);
});

test("el modo espejo intercambia los ojos y el giro", () => {
  const shapes = { eyeBlinkLeft: 1 };
  const mirror = faceToFrame(shapes, { yaw: 15, pitch: 0, roll: 5 }, ZERO, { ...DEFAULT_OPTIONS, mirror: true });
  const plain = faceToFrame(shapes, { yaw: 15, pitch: 0, roll: 5 }, ZERO, { ...DEFAULT_OPTIONS, mirror: false });
  assert.equal(mirror.blinkLeft, 1); assert.equal(mirror.blinkRight, 0);
  assert.equal(plain.blinkLeft, 0); assert.equal(plain.blinkRight, 1);
  // Girar a tu izquierda (yaw medido positivo) hace que en espejo el avatar mire a la izquierda de la pantalla (yaw -).
  near(mirror.yaw, -0.5); near(plain.yaw, 0.5);
  near(mirror.roll, 5); near(plain.roll, -5);
});

test("la calibración resta la pose neutra", () => {
  const f = faceToFrame({}, { yaw: 12, pitch: -6, roll: 3 }, { yaw: 10, pitch: -6, roll: 3 });
  near(f.yaw, -2 / 30); near(f.pitch, 0); near(f.roll, 0);
});

test("invertir el cabeceo cambia el signo", () => {
  const a = faceToFrame({}, { yaw: 0, pitch: 11, roll: 0 }, ZERO, DEFAULT_OPTIONS).pitch;
  const b = faceToFrame({}, { yaw: 0, pitch: 11, roll: 0 }, ZERO, { ...DEFAULT_OPTIONS, invertPitch: true }).pitch;
  near(a, -b); assert.ok(a < 0, "por defecto, pitch medido positivo (mirar abajo) baja la cabeza del avatar");
});

test("todo queda acotado aunque entren valores locos", () => {
  const f = faceToFrame({ jawOpen: 9, eyeBlinkLeft: 9, browInnerUp: 9, mouthSmileLeft: 9, mouthSmileRight: 9 }, { yaw: 500, pitch: -500, roll: 500 }, ZERO);
  assert.ok(f.mouth <= 1 && f.blinkLeft <= 1 && f.brow <= 1 && f.smile <= 1);
  assert.ok(Math.abs(f.yaw) <= 1 && Math.abs(f.pitch) <= 1 && Math.abs(f.roll) <= 35);
  const g = faceToFrame({ jawOpen: NaN }, { yaw: NaN, pitch: 0, roll: 0 }, ZERO);
  assert.equal(g.mouth, 0); assert.equal(g.yaw, 0);
});

test("voz: el silencio cierra la boca y el habla la abre", () => {
  const st = newVoiceState();
  for (let i = 0; i < 50; i++) voiceToFrame(0.004, st, 0); // ruido de fondo
  assert.equal(voiceToFrame(0.004, st, 0).mouth, 0);
  const speech = voiceToFrame(0.08, st, 0);
  assert.ok(speech.mouth > 0.5, String(speech.mouth));
  assert.equal(speech.tracking, true);
});

test("voz: el ruido de fondo se aprende y deja de abrir la boca", () => {
  const st = newVoiceState();
  let last = 1;
  for (let i = 0; i < 4000; i++) last = voiceToFrame(0.02, st, 0).mouth; // habitación ruidosa constante
  assert.ok(last < 0.35, `la boca sigue abierta con ruido constante: ${last}`);
});

test("voz: la boca nunca pasa de 1", () => {
  assert.equal(voiceToFrame(5, newVoiceState(), 0).mouth, 1);
});

test("parpadeo automático: cierra y abre en unos 160 ms, y espacia los parpadeos", () => {
  const blink = new AutoBlink(0, () => 0); // siguiente a los 2200 ms
  assert.equal(blink.at(1000), 0);
  assert.equal(blink.at(2200), 0);
  const mid = blink.at(2280);
  assert.ok(mid > 0.9, String(mid));
  assert.equal(blink.at(2400), 0);
  assert.equal(blink.at(3000), 0);
});

test("differs avisa solo de cambios apreciables", () => {
  assert.equal(differs(IDLE_FRAME, { ...IDLE_FRAME, mouth: 0.005 }), false);
  assert.equal(differs(IDLE_FRAME, { ...IDLE_FRAME, mouth: 0.2 }), true);
  assert.equal(differs(IDLE_FRAME, { ...IDLE_FRAME, tracking: true }), true);
});
