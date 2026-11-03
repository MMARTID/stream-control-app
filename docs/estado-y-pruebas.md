# Estado del MVP y pruebas pendientes

Última actualización: 2026-11-03. El README es la especificación; este archivo dice qué hay hecho y qué falta comprobar.

## Qué hay

- **Cliente obs-websocket v5** (`src-tauri/src/obs/`): handshake, autenticación SHA-256, reconexión con espera creciente (1 s → 5 s), eventos y peticiones con tiempo límite.
- **Núcleo** (`src-tauri/src/control.rs`): refleja el estado de OBS (escenas, escena en programa, escena anterior, stream, micro, cámaras) y ejecuta las acciones: lanzar escena, escena rápida con silencio de micro (RF-37), volver a la anterior, cambiar de cámara (RF-6), micro, iniciar/parar stream.
- **Ajustes** (`src-tauri/src/settings/`): JSON local en la carpeta de configuración de la app; contraseña de OBS en el almacén seguro del sistema.
- **Interfaz** (`src/`): TypeScript sin framework. Panel de escenas rápidas, cámaras, búsqueda de escenas, micro, iniciar/parar stream (parar pide confirmación en línea) y ajustes.

Peticiones de obs-websocket usadas: `GetVersion`, `GetSceneList`, `SetCurrentProgramScene`, `GetInputList`, `GetInputMute`, `SetInputMute`, `GetSceneItemList`, `SetSceneItemEnabled`, `GetStreamStatus`, `StartStream`, `StopStream`. Eventos: `CurrentProgramSceneChanged`, `SceneListChanged`, `SceneCreated`, `SceneRemoved`, `SceneNameChanged`, `StreamStateChanged`, `InputMuteStateChanged`, `SceneItemEnableStateChanged`.

## Cómo se ha probado

- `cargo test` en `src-tauri/`: 22 pruebas (unitarias y de integración) contra un **OBS simulado** (`src-tauri/tests/common/mod.rs`). Cubren conexión, autenticación, contraseña incorrecta, reconexión, escenas rápidas, micro, cámaras (orden: se enciende la nueva antes de apagar la anterior), stream y que la contraseña no llega al JSON ni a la interfaz.
- La app se lanzó en macOS con `npm run tauri dev` contra ese OBS simulado (`cargo run --example mock_obs`): conectó y envió órdenes reales (escenas rápidas con silencio de micro y vuelta, cambio de cámara).

## Lo que NO se ha comprobado (hacerlo primero en el PC con Windows)

1. **Nada se ha probado en Windows.** Es la plataforma prioritaria.
2. **Nada se ha probado contra un OBS real.** El simulador usa los nombres de campo de la documentación (`docs/generated/protocol.md` de obs-websocket, rama `master`), no observados en un OBS en marcha. En concreto, la documentación no detalla los campos de los elementos de lista, y se asumen estos de obs-websocket v5: `sceneName`, `sourceName`, `sceneItemId`, `sceneItemEnabled`, `inputName`, `inputKind`. Hay que confirmarlos con la versión de OBS instalada (README, §9).
3. **Orden de las escenas:** se muestran en el orden en que las devuelve `GetSceneList`. No se ha comprobado si coincide con el de la interfaz de OBS o va al revés. El README (RF-13) dice que el orden definitivo lo guarda la app; aún no está hecho.
4. **Almacén seguro de Windows (Credential Manager):** solo se ha compilado la parte de macOS. En Windows ejecutar `cargo test keyring_real -- --ignored` y guardar una contraseña desde Ajustes.
5. **Imagen sin cortes al cambiar de cámara** (criterio de aceptación 7.1): hay que medirlo con OBS real y la transición de OBS.

## Pendiente del MVP (README §10, Fase 1)

- Miniaturas en vivo de las escenas (RF-9, `GetSourceScreenshot`), con límite de frecuencia.
- Estado de conexión de cada cámara y aviso si se desconecta (RF-7).
- Panel de salud del stream: bitrate, FPS, frames perdidos, CPU (RF-24, `GetStats`).
- Chat de Twitch y YouTube en solo lectura.
- Autoarranque con el ordenador (app y OBS), bandeja y ejecución portable (RF-27, RF-28).
- Atajos de teclado, incluidos los globales (RF-14, RF-35).
- Orden de escenas arrastrable (RF-13).
- Botón para abrir OBS (RF-3, opcional).

El panel web de red local es v1: no se ha empezado.

## Compilar para Windows

El repositorio no incluye ningún `.exe` compilado: se genera en Windows. Hay dos caminos.

**En el PC con Windows** (necesita Rust con toolchain MSVC, Visual Studio Build Tools con C++, Node y WebView2):

```powershell
npm install
npm run tauri build -- --bundles nsis
```

Resultado: `src-tauri\target\release\stream-control.exe` (portable, se ejecuta desde su carpeta sin instalar, RF-27) y el instalador en `src-tauri\target\release\bundle\nsis\`. Para solo probar: `npm run tauri dev`.

**En GitHub Actions** (sin instalar nada): pestaña Actions → «Build de Windows» → Run workflow. Ejecuta `cargo test` en Windows, la prueba del Credential Manager real y deja el `.exe` portable y el instalador como artefactos descargables (`.github/workflows/windows.yml`). El `.exe` no está firmado: Windows SmartScreen avisará la primera vez (README, §11).

## Prueba manual sin OBS

```bash
cd src-tauri
cargo run --example mock_obs        # OBS simulado en 127.0.0.1:4455 (sin contraseña)
```

En otra terminal, desde la raíz: `npm run tauri dev`. En Ajustes hay que elegir el micro («Micro»), añadir las cámaras («Cámara móvil», «Webcam») y marcar escenas rápidas.
