// Pestaña «Móvil»: el panel web de la red local, explicado paso a paso. Los cambios se aplican al instante.
import { api } from "../api";
import { state } from "../state";
import { updateSettings } from "../ops";
import { $, esc, paint, run, toast, when } from "../util";

let root: HTMLElement;

export function mountMobile(el: HTMLElement) {
  root = el;
  root.innerHTML = `
    <div class="page-head"><h1>Controla el directo desde el móvil</h1>
      <p>Un panel web que solo funciona dentro de tu Wi‑Fi: sin nube, sin cuentas y con contraseña. El PC debe estar encendido con OBS abierto.</p></div>
    <section class="card" id="m-switch"></section>
    <div id="m-body"></div>`;

  root.addEventListener("change", (e) => {
    const t = e.target as HTMLInputElement;
    switch (t.dataset.bind) {
      case "enabled": return void updateSettings((s) => (s.panel.enabled = t.checked));
      case "stream": return void updateSettings((s) => (s.panel.allowStreamControl = t.checked));
      case "onlyQuick": return void updateSettings((s) => (s.panel.onlyQuickScenes = t.checked));
    }
  });
  root.addEventListener("click", (e) => {
    const el = (e.target as HTMLElement).closest<HTMLElement>("[data-action]");
    if (!el) return;
    switch (el.dataset.action) {
      case "save-password": {
        const input = $<HTMLInputElement>("panel-password");
        if (!input.value) return toast("Escribe la contraseña antes de guardarla");
        const value = input.value;
        input.value = "";
        return void run(api.setPanelPassword(value).then(() => toast("Contraseña del panel guardada", "ok")));
      }
      case "clear-password": return void run(api.setPanelPassword(null));
      case "revoke": return void run(api.revokeDevice(el.dataset.id!));
      case "copy": return void copy(el.dataset.text!);
      case "apply-port": {
        const port = Number($<HTMLInputElement>("panel-port").value);
        return void updateSettings((s) => (s.panel.port = port), "Puerto cambiado");
      }
    }
  });
}

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast("Dirección copiada", "ok");
  } catch {
    toast("No se pudo copiar. Selecciónala y pulsa Ctrl+C");
  }
}

export function updateMobile() {
  const s = state.snap;
  const p = state.panel;
  if (!s || !p) return;
  const cfg = s.settings.panel;

  paint("m-switch", { cfg, p: { r: p.running, e: p.error, port: p.port } }, () => `
    <label class="switch big-switch"><input type="checkbox" data-bind="enabled" ${cfg.enabled ? "checked" : ""} /><span class="track"></span>
      <b>Activar el panel web</b></label>
    <p class="hint">${cfg.enabled ? (p.running ? `Activo en el puerto ${p.port}.` : "Arrancando…") : "Desactivado: nadie puede entrar."}</p>
    ${p.error ? `<p class="err">${esc(p.error)}</p>` : ""}`);

  const now = Date.now() / 1000;
  const pairings = p.pairings.filter((x) => x.expiresAt > now);
  if (!cfg.enabled) {
    paint("m-body", "off", () => `<div class="callout"><h3>Cómo funciona</h3><ol>
      <li>Activa el panel y elige una contraseña.</li>
      <li>Abre la dirección que te damos en el navegador del móvil (misma Wi‑Fi). La primera vez avisará del certificado: es normal, es propio de esta app.</li>
      <li>Escribe la contraseña y el código de 6 dígitos que verás aquí. Listo: escenas, cámaras, micro y más, con botones grandes.</li></ol></div>`);
    return;
  }

  paint("m-body", { cfg, p: { ...p, audit: p.audit.slice(0, 40), pairings } }, () => `
    <section class="card">
      <h3><span class="num">1</span> Contraseña del panel</h3>
      <p>La pedirá cada móvil la primera vez. Mínimo 8 caracteres. Se guarda cifrada en el almacén seguro del sistema.</p>
      <div class="row">
        <label class="field"><span>Contraseña</span><input type="password" id="panel-password" autocomplete="off" placeholder="${p.hasPassword ? "Guardada: escribe otra para cambiarla" : "Aún sin contraseña"}" /></label>
        <button class="primary" data-action="save-password">Guardar contraseña</button>
        ${p.hasPassword ? `<button class="ghost" data-action="clear-password">Borrar</button>` : ""}
      </div>
      ${!p.hasPassword ? `<p class="warn-text">Sin contraseña ningún móvil puede emparejarse.</p>` : ""}
    </section>

    <section class="card">
      <h3><span class="num">2</span> Abre esta dirección en el móvil</h3>
      ${
        p.running
          ? `<div class="panel-url">
              <div class="qr">${p.qrSvg ?? ""}</div>
              <div class="grow">
                ${p.urls.map((u) => `<div class="url-row"><code>${esc(u)}</code><button data-action="copy" data-text="${esc(u)}">Copiar</button></div>`).join("") || `<p class="warn-text">No se encontró una dirección de red local. ¿Estás conectado a una red?</p>`}
                <p class="hint">Escanea el QR o escribe la dirección. Si el navegador avisa «la conexión no es privada», pulsa <b>Avanzado → Continuar</b>: el certificado lo genera esta app, no un tercero.</p>
                <p class="hint">En Windows, si salta el Firewall, permite el acceso en <b>redes privadas</b>.</p>
              </div>
            </div>`
          : `<p class="hint">El panel está arrancando…</p>`
      }
    </section>

    <section class="card">
      <h3><span class="num">3</span> Escribe el código en el móvil</h3>
      ${
        pairings.length
          ? pairings.map((x) => `<div class="pairing-row"><span><b>${esc(x.name)}</b> <small>${esc(x.ip)} · caduca en 2 minutos</small></span><b class="code">${esc(x.code)}</b></div>`).join("")
          : `<p class="hint">Cuando un móvil ponga la contraseña, su código de 6 dígitos aparecerá aquí (y también en la pestaña Directo).</p>`
      }
    </section>

    <section class="card">
      <h3>Dispositivos emparejados</h3>
      ${
        p.devices.length
          ? p.devices
              .map(
                (d) => `<div class="device-row"><span class="dot ${d.online ? "on" : ""}"></span>
                  <span class="grow">${esc(d.name)}<small>${d.online ? "conectado ahora" : "visto " + esc(when(d.lastSeen))} · ${esc(d.lastIp)}</small></span>
                  <button data-action="revoke" data-id="${esc(d.id)}">Revocar</button></div>`,
              )
              .join("")
          : `<p class="hint">Ningún dispositivo todavía.</p>`
      }
      <p class="hint">«Revocar» corta al móvil al instante: tendrá que emparejarse de nuevo.</p>
    </section>

    <section class="card">
      <h3>Qué puede hacer el panel</h3>
      <label class="switch"><input type="checkbox" data-bind="stream" ${cfg.allowStreamControl ? "checked" : ""} /><span class="track"></span>
        Iniciar y parar el directo desde el móvil <small>Siempre pide confirmación en dos pasos. Viene desactivado.</small></label>
      <label class="switch"><input type="checkbox" data-bind="onlyQuick" ${cfg.onlyQuickScenes ? "checked" : ""} /><span class="track"></span>
        Solo escenas rápidas <small>Si lo activas, el móvil no podrá lanzar otras escenas de OBS.</small></label>
      <details><summary>Opciones avanzadas</summary>
        <div class="row"><label class="field"><span>Puerto</span><input type="number" id="panel-port" min="1024" max="65535" value="${cfg.port}" /></label>
        <button data-action="apply-port">Aplicar puerto</button></div></details>
    </section>

    <section class="card">
      <h3>Registro de acciones</h3>
      ${
        p.audit.length
          ? `<div class="audit">${p.audit.slice(0, 40).map((a) => `<div class="${a.ok ? "" : "fail"}"><time>${esc(when(a.at))}</time> <b>${esc(a.device)}</b> · ${esc(a.action)}: ${esc(a.detail)}</div>`).join("")}</div>`
          : `<p class="hint">Aquí quedará cada acción hecha desde un móvil: cuándo, desde qué dispositivo y qué se hizo.</p>`
      }
    </section>`);
}

/** Aviso en la parte de arriba con el código pendiente, visible en cualquier pestaña. */
export function renderPairingBanner() {
  const box = $("pairing");
  const now = Date.now() / 1000;
  const pending = (state.panel?.pairings ?? []).filter((x) => x.expiresAt > now);
  box.hidden = pending.length === 0;
  box.innerHTML = pending
    .map((x) => `<span>📱 <b>${esc(x.name)}</b> quiere emparejarse. Escribe este código en el móvil: <b class="code">${esc(x.code)}</b> <small>(caduca en ${Math.max(0, Math.round(x.expiresAt - now))} s)</small></span>`)
    .join("");
}
