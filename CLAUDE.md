# Stream Control

App de escritorio local (Tauri 2: núcleo en Rust, interfaz en TypeScript) que acompaña a OBS en los directos. **`README.md` es la especificación**: léelo entero antes de cambiar nada. `docs/estado-y-pruebas.md` dice qué hay hecho y qué falta comprobar.

Idioma: interfaz, documentación, mensajes de error y comentarios en español.

## Restricciones (del README)

- Local-first: sin servicios en la nube ni telemetría.
- No escribir ni editar los archivos de colección de OBS. Todo va por obs-websocket.
- Contraseñas y tokens de chat solo en el almacén seguro del sistema, nunca en texto plano ni en el JSON de ajustes ni en lo que ve la interfaz.
- Chat de Twitch y YouTube en solo lectura.
- Sin modo estudio.
- El panel web (v1) ya está hecho; sus límites y decisiones propias están en `docs/panel-web.md`. Mantén sus reglas de seguridad: solo red local, HTTPS, contraseña + código por dispositivo, dos pasos para iniciar o parar el stream, y que al móvil solo llegue `WebState`.
- Verificar nombres de peticiones y eventos de obs-websocket contra la documentación de la versión de OBS instalada.
- Windows es la prioridad; macOS opcional.

## Estructura

- `src-tauri/src/obs/`: cliente obs-websocket v5 (protocolo, autenticación, reconexión, peticiones).
- `src-tauri/src/control.rs`: núcleo. Refleja el estado de OBS y ejecuta acciones. La interfaz solo lee `Snapshot`.
- `src-tauri/src/settings/`: ajustes en JSON y almacén seguro (`SecretStore`).
- `src-tauri/src/preview.rs`: capturas de la salida y de las fuentes (`GetSourceScreenshot`); solo capturan mientras alguien mira.
- `src-tauri/src/panel/` y `src-tauri/web/`: panel web de red local para el móvil (HTTPS, emparejamiento, comandos, registro). Léelo en `docs/panel-web.md`.
- `src-tauri/src/avatar.rs` y `src-tauri/web/avatar*`: avatar animado (servidor solo en 127.0.0.1 y página para la fuente de navegador de OBS). Léelo en `docs/avatar.md`.
- `src-tauri/src/app.rs`: comandos de Tauri (envoltorios finos sobre `Control`, `Panel` y `Avatar`).
- `src/`: interfaz de escritorio sin framework: `main.ts` (cabecera y pestañas), `views/` (una por pestaña), `avatar/` (seguimiento con MediaPipe y sus conversiones, con pruebas), `api.ts`, `state.ts`, `mock.ts` (OBS simulado para `npm run dev` en un navegador).
- `src-tauri/tests/`: pruebas contra un OBS simulado (`common/mod.rs`). `examples/mock_obs.rs` lo expone para pruebas manuales y `examples/panel_dev.rs` levanta el panel con ese OBS simulado.

## Comandos

```bash
npm install
npm run tauri dev                  # arranca la app (necesita Rust y Node)
cd src-tauri && cargo test         # 65 pruebas contra el OBS simulado (incluye TLS real del panel y el avatar)
npm test                           # conversiones del seguimiento de la cara y la voz
cd src-tauri && cargo test keyring_real -- --ignored   # almacén seguro REAL del sistema
cd src-tauri && cargo run --example mock_obs           # OBS simulado en 127.0.0.1:4455
cd src-tauri && cargo run --example panel_dev -- 8090 --plain   # panel web en http://127.0.0.1:8090 (solo pruebas)
npm run build                      # tsc + vite (antes copia el motor de MediaPipe a public/mediapipe/wasm)
npm run dev                        # en un navegador normal usa el OBS simulado de src/mock.ts (?mock=empty|offline, ?pairing)
```

En Windows hace falta Rust (toolchain MSVC), Visual Studio Build Tools con C++, Node y WebView2.

## Antes de dar algo por hecho

- Las pruebas con OBS simulado **no** demuestran que los nombres de campo coincidan con un OBS real: confirmarlo con OBS instalado.
- La app es para Windows. Cambios nuevos: sube la versión en `package.json`, `src-tauri/Cargo.toml` y `src-tauri/tauri.conf.json`; `docs/actualizar.md` explica cómo actualiza el usuario y cómo se publica una versión.
- El seguimiento de la cara usa MediaPipe **dentro de la app** (modelo en `public/mediapipe/`, motor copiado desde `node_modules`). No añadas nada que descargue modelos o envíe imágenes por la red.
- Los personajes del avatar son dibujos propios inspirados en GTA VI, no arte oficial; no copies imágenes del juego.
- Al compilar para Windows no uses crates que exijan `aws-lc-rs`/NASM/cmake: el TLS del panel va con `ring` a propósito.
- No hagas push a `main` sin que el usuario lo pida en esa conversación.
