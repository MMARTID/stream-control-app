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
- `src-tauri/src/app.rs`: comandos de Tauri (envoltorios finos sobre `Control` y `Panel`).
- `src/`: interfaz de escritorio sin framework (`main.ts`, `api.ts`, `styles.css`).
- `src-tauri/tests/`: pruebas contra un OBS simulado (`common/mod.rs`). `examples/mock_obs.rs` lo expone para pruebas manuales y `examples/panel_dev.rs` levanta el panel con ese OBS simulado.

## Comandos

```bash
npm install
npm run tauri dev                  # arranca la app (necesita Rust y Node)
cd src-tauri && cargo test         # unas 55 pruebas contra el OBS simulado (incluye TLS real del panel)
cd src-tauri && cargo test keyring_real -- --ignored   # almacén seguro REAL del sistema
cd src-tauri && cargo run --example mock_obs           # OBS simulado en 127.0.0.1:4455
cd src-tauri && cargo run --example panel_dev -- 8090 --plain   # panel web en http://127.0.0.1:8090 (solo pruebas)
npm run build                      # tsc + vite
```

En Windows hace falta Rust (toolchain MSVC), Visual Studio Build Tools con C++, Node y WebView2.

## Antes de dar algo por hecho

- Las pruebas con OBS simulado **no** demuestran que los nombres de campo coincidan con un OBS real: confirmarlo con OBS instalado.
- Al compilar para Windows no uses crates que exijan `aws-lc-rs`/NASM/cmake: el TLS del panel va con `ring` a propósito.
- No hagas push a `main` sin que el usuario lo pida en esa conversación.
