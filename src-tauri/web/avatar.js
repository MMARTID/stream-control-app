// Página del avatar: la carga OBS como fuente de navegador y la vista previa de la app.
// Dibuja un personaje SVG (web/avatars/*.svg) y lo anima con los valores que llegan por WebSocket.
// Sin dependencias ni paso de compilación.

const query = new URLSearchParams(location.search);
const preview = query.has("preview");
// `?hold` ignora los fotogramas del servidor: solo para probar poses a mano desde la consola.
const hold = query.has("hold");
if (preview) document.body.classList.add("preview");

const stage = document.getElementById("stage");
const note = document.getElementById("note");

const NEUTRAL = { mouth: 0, smile: 0.25, blinkL: 0, blinkR: 0, brow: 0, yaw: 0, pitch: 0, roll: 0 };
const target = { ...NEUTRAL };
const cur = { ...NEUTRAL };
let tracking = false;
let lastFrameAt = 0;
let parts = null;
let loaded = "";
let blinkStart = -1;
let nextBlink = 2500;

const OUT_OF_DATE_MS = 1500;
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const num = (v, d = 0) => (typeof v === "number" && Number.isFinite(v) ? v : d);

async function load(name) {
  if (!/^[a-z0-9_-]+$/.test(name) || name === loaded) return;
  const res = await fetch(`/avatars/${name}.svg`);
  if (!res.ok) return;
  stage.innerHTML = await res.text();
  const $ = (id) => stage.querySelector(`#${id}`);
  parts = {
    body: $("avatar-body"), head: $("avatar-head"), face: $("avatar-face"),
    eyeL: $("eye-l"), eyeR: $("eye-r"), pupilL: $("pupil-l"), pupilR: $("pupil-r"),
    browL: $("brow-l"), browR: $("brow-r"),
    lips: $("mouth-lips"), cavity: $("mouth-cavity"), clip: $("mouth-clip-path"), line: $("mouth-line"),
    teeth: $("mouth-teeth"), tongue: $("mouth-tongue"),
  };
  loaded = name;
}

/** Boca: centro (200,250). `open` 0..1 abre la mandíbula; `smile` -1..1 sube o baja las comisuras. */
function mouthPath(open, smile) {
  const w = 36 - 6 * open;
  const corner = -smile * 9;
  const top = corner + 6 + smile * 8 - open * 7;
  const bottom = top + open * 50 * 1.9;
  const x0 = (200 - w).toFixed(1), x1 = (200 + w).toFixed(1);
  const cy = (250 + corner).toFixed(1);
  return {
    d: `M ${x0} ${cy} Q 200 ${(250 + top).toFixed(1)} ${x1} ${cy} Q 200 ${(250 + bottom).toFixed(1)} ${x0} ${cy} Z`,
    topMid: 250 + (corner + top) / 2,
    bottomMid: 250 + (corner + bottom) / 2,
    w,
  };
}

function applyPose(p, now) {
  if (!parts) return;
  const bob = Math.sin(now / 1000 * 1.7) * 2.5;
  parts.body.setAttribute("transform", `translate(0 ${bob.toFixed(2)})`);
  parts.head.setAttribute("transform", `rotate(${p.roll.toFixed(2)} 200 300) translate(${(p.yaw * 6).toFixed(2)} ${(-p.pitch * 5).toFixed(2)})`);
  parts.face.setAttribute("transform", `translate(${(p.yaw * 15).toFixed(2)} ${(-p.pitch * 9).toFixed(2)})`);

  const eye = (el, blink) => {
    const s = 1 - 0.93 * clamp(blink, 0, 1);
    el.setAttribute("transform", `translate(${el.dataset.cx} ${el.dataset.cy}) scale(1 ${s.toFixed(3)})`);
  };
  eye(parts.eyeL, p.blinkL);
  eye(parts.eyeR, p.blinkR);
  const gaze = `translate(${(p.yaw * 4).toFixed(2)} ${(-p.pitch * 3).toFixed(2)})`;
  parts.pupilL?.setAttribute("transform", gaze);
  parts.pupilR?.setAttribute("transform", gaze);
  const brow = `translate(0 ${(-p.brow * 11).toFixed(2)})`;
  parts.browL.setAttribute("transform", brow);
  parts.browR.setAttribute("transform", brow);

  const m = mouthPath(clamp(p.mouth, 0, 1), clamp(p.smile, -1, 1));
  parts.lips?.setAttribute("d", m.d);
  parts.cavity.setAttribute("d", m.d);
  parts.clip.setAttribute("d", m.d);
  parts.line.setAttribute("d", m.d);
  const depth = m.bottomMid - m.topMid;
  parts.teeth.setAttribute("x", (200 - m.w + 4).toFixed(1));
  parts.teeth.setAttribute("width", (2 * m.w - 8).toFixed(1));
  parts.teeth.setAttribute("y", (m.topMid - 6).toFixed(1));
  parts.teeth.setAttribute("height", depth > 3 ? Math.min(depth * 0.4 + 6, 24).toFixed(1) : "0");
  parts.tongue.setAttribute("cy", (m.bottomMid - 4).toFixed(1));
  parts.tongue.setAttribute("rx", (14 + p.mouth * 8).toFixed(1));
  parts.tongue.setAttribute("ry", depth > 6 ? Math.min(depth * 0.35, 14).toFixed(1) : "0");
}

let last = performance.now();
function frame(now) {
  const dt = Math.min((now - last) / 1000, 0.1);
  last = now;
  const live = tracking && now - lastFrameAt < OUT_OF_DATE_MS;
  let goal = target;

  if (!live) {
    // Sin seguimiento el personaje sigue vivo: respira, se balancea y parpadea de vez en cuando.
    const t = now / 1000;
    let blink = 0;
    if (blinkStart < 0 && now > nextBlink) blinkStart = now;
    if (blinkStart >= 0) {
      const k = (now - blinkStart) / 170;
      blink = k >= 1 ? 0 : Math.sin(k * Math.PI);
      if (k >= 1) { blinkStart = -1; nextBlink = now + 2200 + Math.random() * 3800; }
    }
    goal = { ...NEUTRAL, blinkL: blink, blinkR: blink, roll: Math.sin(t * 0.6) * 1.6, yaw: Math.sin(t * 0.37) * 0.08 };
  }

  const ease = (k) => 1 - Math.exp(-dt * k);
  cur.mouth += (goal.mouth - cur.mouth) * ease(30);
  cur.blinkL += (goal.blinkL - cur.blinkL) * ease(45);
  cur.blinkR += (goal.blinkR - cur.blinkR) * ease(45);
  for (const k of ["smile", "brow", "yaw", "pitch", "roll"]) cur[k] += (goal[k] - cur[k]) * ease(16);

  applyPose(cur, now);
  requestAnimationFrame(frame);
}

function setFrame(f) {
  tracking = !!f.tracking;
  lastFrameAt = performance.now();
  target.mouth = clamp(num(f.mouth), 0, 1);
  target.smile = clamp(num(f.smile, 0.25), -1, 1);
  target.blinkL = clamp(num(f.blinkLeft), 0, 1);
  target.blinkR = clamp(num(f.blinkRight), 0, 1);
  target.brow = clamp(num(f.brow), -1, 1);
  target.yaw = clamp(num(f.yaw), -1, 1);
  target.pitch = clamp(num(f.pitch), -1, 1);
  target.roll = clamp(num(f.roll), -35, 35);
}

function connect() {
  const ws = new WebSocket(`ws://${location.host}/avatar/ws`);
  ws.onopen = () => { if (preview) note.hidden = true; };
  ws.onmessage = (e) => {
    let msg;
    try { msg = JSON.parse(e.data); } catch { return; }
    if (msg.type === "config" && typeof msg.character === "string") load(msg.character);
    else if (msg.type === "frame" && !hold) setFrame(msg);
  };
  ws.onclose = () => {
    if (preview) { note.textContent = "Sin conexión con la app. Reintentando…"; note.hidden = false; }
    setTimeout(connect, 1500);
  };
}

window.__avatar = { setFrame, load };
connect();
requestAnimationFrame(frame);
