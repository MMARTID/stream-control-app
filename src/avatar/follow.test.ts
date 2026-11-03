import assert from "node:assert/strict";
import { test } from "node:test";
import { decideFollow, type FollowInput } from "./follow.ts";

const base: FollowInput = { follow: true, ready: true, onAir: false, active: false, startedByFollow: false, suppressed: false, starting: false };
const with_ = (patch: Partial<FollowInput>) => decideFollow({ ...base, ...patch });

test("al poner la escena del avatar en pantalla empieza a seguir", () => {
  assert.equal(with_({ onAir: true }), "start");
});

test("al salir de la escena del avatar para el seguimiento automático y libera la cámara", () => {
  assert.equal(with_({ onAir: false, active: true, startedByFollow: true }), "stop");
});

test("no toca un seguimiento arrancado a mano con «Probar»", () => {
  assert.equal(with_({ onAir: false, active: true, startedByFollow: false }), "none");
  assert.equal(with_({ onAir: true, active: true, startedByFollow: false }), "none");
});

test("si ya está siguiendo en la escena del avatar no hace nada", () => {
  assert.equal(with_({ onAir: true, active: true, startedByFollow: true }), "none");
});

test("no arranca dos veces mientras está arrancando", () => {
  assert.equal(with_({ onAir: true, starting: true }), "none");
});

test("tras parar a mano o fallar no reintenta hasta cambiar de escena", () => {
  assert.equal(with_({ onAir: true, suppressed: true }), "none");
});

test("sin la opción activada o con el avatar apagado, solo se para lo que arrancó el automático", () => {
  assert.equal(with_({ follow: false, onAir: true }), "none");
  assert.equal(with_({ follow: false, onAir: true, active: true, startedByFollow: true }), "stop");
  assert.equal(with_({ ready: false, onAir: true, active: true, startedByFollow: true }), "stop");
  assert.equal(with_({ ready: false, onAir: true }), "none");
});
