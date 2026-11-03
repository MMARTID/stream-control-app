# Stream Control

App de escritorio local (Tauri 2: núcleo en Rust, interfaz en TypeScript) que acompaña a OBS en los directos. **`README.md` es la especificación**: léelo entero antes de cambiar nada. `docs/estado-y-pruebas.md` dice qué hay hecho y qué falta comprobar.

Idioma: interfaz, documentación, mensajes de error y comentarios en español.

## Restricciones (del README)

- Local-first: sin servicios en la nube ni telemetría.
- No escribir ni editar los archivos de colección de OBS. Todo va por obs-websocket.
- Contraseñas y tokens de chat solo en el almacén seguro del sistema, nunca en texto plano ni en el JSON de ajustes ni en lo que ve la interfaz.
- Chat de Twitch y YouTube en solo lectura.
- Sin modo estudio. El panel web de red local es v1: no empezarlo todavía.
- Verificar nombres de peticiones y eventos de obs-websocket contra la documentación de la versión de OBS instalada.
- Windows es la prioridad; macOS opcional.

## Estructura

- `src-tauri/src/obs/`: cliente obs-websocket v5 (protocolo, autenticación, reconexión, peticiones).
- `src-tauri/src/control.rs`: núcleo. Refleja el estado de OBS y ejecuta acciones. La interfaz solo lee `Snapshot`.
- `src-tauri/src/settings/`: ajustes en JSON y almacén seguro (`SecretStore`).
- `src-tauri/src/app.rs`: comandos de Tauri (envoltorios finos sobre `Control`).
- `src/`: interfaz sin framework (`main.ts`, `api.ts`, `styles.css`).
- `src-tauri/tests/`: pruebas contra un OBS simulado (`common/mod.rs`). `examples/mock_obs.rs` lo expone para pruebas manuales.

## Comandos

```bash
npm install
npm run tauri dev                  # arranca la app (necesita Rust y Node)
cd src-tauri && cargo test         # 22 pruebas contra el OBS simulado
cd src-tauri && cargo test keyring_real -- --ignored   # almacén seguro REAL del sistema
cd src-tauri && cargo run --example mock_obs           # OBS simulado en 127.0.0.1:4455
npm run build                      # tsc + vite
```

En Windows hace falta Rust (toolchain MSVC), Visual Studio Build Tools con C++, Node y WebView2.

## Antes de dar algo por hecho

- Las pruebas con OBS simulado **no** demuestran que los nombres de campo coincidan con un OBS real: confirmarlo con OBS instalado.
- No hagas push a `main` sin que el usuario lo pida en esa conversación.
