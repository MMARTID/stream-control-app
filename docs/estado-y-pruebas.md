# Estado del MVP y pruebas pendientes

Última actualización: 2026-11-03 (versión 0.2.0). El README es la especificación; este archivo dice qué hay hecho y qué falta comprobar.

## Qué hay

- **Cliente obs-websocket v5** (`src-tauri/src/obs/`): handshake, autenticación SHA-256, reconexión con espera creciente (1 s → 5 s), eventos y peticiones con tiempo límite.
- **Núcleo** (`src-tauri/src/control.rs`): refleja el estado de OBS (escenas, escena en programa, anterior, stream, micro, cámaras, fuentes, salud) y ejecuta las acciones: escena rápida con silencio de micro (RF-37), volver, cámara (RF-6), micro, fuentes (RF-12), iniciar/parar stream.
- **Capturas** (`src-tauri/src/preview.rs`): `GetSourceScreenshot` de la salida y de cada fuente, de una en una (~3 y 1 por segundo), solo mientras alguien mira. No es vídeo.
- **Panel web de red local** (`src-tauri/src/panel/`, `src-tauri/web/`): HTTPS, contraseña + código por dispositivo, comandos, registro y revocación. Ver `docs/panel-web.md`.
- **Avatar animado** (`src-tauri/src/avatar.rs`, `src-tauri/web/avatar*`, `src/avatar/`): dos personajes, seguimiento de cara (MediaPipe en local) o de voz, y una **escena «Avatar»** que la app crea en OBS (avatar abajo a la izquierda en lugar de la cámara, con las demás fuentes copiadas) y que activa el seguimiento solo mientras está en pantalla. Ver `docs/avatar.md`.
- **Interfaz de escritorio** (`src/`, TypeScript sin framework): cuatro pestañas (Directo, Avatar, Móvil, Ajustes), «Primeros pasos», ayuda cuando OBS no conecta y estados vacíos con botones. `npm run dev` en un navegador usa un OBS simulado (`src/mock.ts`: `?mock=empty`, `?mock=offline`, `?pairing`).
- **Ajustes** (`src-tauri/src/settings/`): JSON local; contraseñas y la clave del certificado en el almacén seguro del sistema.

Peticiones de obs-websocket usadas: `GetVersion`, `GetSceneList`, `SetCurrentProgramScene`, `GetInputList`, `GetInputMute`, `SetInputMute`, `GetSceneItemList`, `SetSceneItemEnabled`, `GetStreamStatus`, `GetStats`, `GetSourceScreenshot`, `CreateInput`, `StartStream`, `StopStream`. Eventos: `CurrentProgramSceneChanged`, `SceneListChanged`, `SceneCreated`, `SceneRemoved`, `SceneNameChanged`, `StreamStateChanged`, `InputMuteStateChanged`, `SceneItemEnableStateChanged`, `SceneItemCreated`, `SceneItemRemoved`, `SceneItemListReindexed`.

## Cómo se ha probado

- `cargo test` en `src-tauri/`: 69 pruebas contra un **OBS simulado** (`src-tauri/tests/common/mod.rs`): cliente, núcleo, capturas y salud, panel web (incluido un servidor HTTPS real) y avatar. `npm test`: 23 pruebas (conversiones del seguimiento y cuándo seguir según la escena).
- **En Windows (GitHub Actions, `windows-latest`)**: las 22 pruebas de la primera versión pasaron, **la prueba del almacén seguro real de Windows (Credential Manager) pasó**, y la compilación generó el `.exe` portable y el instalador NSIS. Es un servidor de GitHub, no el PC de tu equipo. Las pruebas de las versiones nuevas (panel, avatar) aún no se han ejecutado allí.
- La app completa se lanzó en macOS contra el OBS simulado y envió órdenes reales.
- La interfaz de escritorio, el panel del móvil y el avatar se vieron en un navegador (el de escritorio y el avatar con el OBS/núcleo simulados; el panel del móvil con tamaño de móvil).
- El modelo facial y su motor cargan y detectan la cara de los dibujos del avatar (ver `docs/avatar.md`).

## Lo que NO se ha comprobado (hacerlo primero en el PC con Windows y OBS)

1. **Nada se ha probado con un OBS real.** El simulador usa los nombres de campo de la documentación (`docs/generated/protocol.md` de obs-websocket, rama `master`), no observados en un OBS en marcha. La documentación no detalla los campos de las listas; se asumen estos de obs-websocket v5: `sceneName`, `sourceName`, `sceneItemId`, `sceneItemEnabled`, `inputKind`, `inputName`, `imageData`, `sceneItemId` (de `CreateInput`). Y los ajustes de la fuente de navegador: `url`, `width`, `height`, `fps`.
2. **Orden de las escenas:** se muestran en el orden en que las devuelve `GetSceneList`; no se sabe si coincide con el de OBS (RF-13).
3. **Imagen sin cortes al cambiar de cámara** (criterio 7.1) y **capturas con OBS real** (fuentes ocultas, consumo de CPU de OBS).
4. **Panel web en un móvil real**: HTTPS con certificado autofirmado, WebSocket seguro (Safari en iPhone es el más dudoso) y el Firewall de Windows. Ver `docs/panel-web.md`.
5. **Avatar con una cara y una cámara reales, permisos de WebView2, seguimiento con la ventana minimizada, consumo de CPU, y la creación de la escena «Avatar» en OBS real** (posición y escala de la fuente, orden de las fuentes copiadas). Ver `docs/avatar.md`.
6. **El parecido de Jason y Cal con los personajes del juego.** Salen de descripciones, no de imágenes.

## Pendiente

- Miniaturas de *todas las escenas* de la lista (RF-9); hoy hay captura de la salida y de las fuentes de la escena en programa.
- Estado de conexión de cada cámara y aviso si se desconecta (RF-7); aviso proactivo de frames perdidos (RF-25).
- Chat de Twitch y YouTube en solo lectura (y en el panel web).
- Autoarranque con el ordenador (app y OBS), bandeja y datos junto al ejecutable (modo portable de verdad, RF-27, RF-28).
- Atajos de teclado, incluidos los globales (RF-14, RF-35); orden de escenas arrastrable (RF-13); botón para abrir OBS (RF-3).
- Actualizaciones automáticas firmadas (v2). Hoy se actualiza a mano: ver `docs/actualizar.md`.

## Compilar para Windows

El repositorio no incluye ningún `.exe`: se genera en Windows. Hay dos caminos.

**En GitHub Actions (sin instalar nada):** pestaña Actions → «Build de Windows» → Run workflow. Ejecuta las pruebas, compila y deja el `.exe` portable y el instalador como artefacto descargable. Si subes una etiqueta (`git tag v0.3.0 && git push origin v0.3.0`), además publica una Release. El `.exe` no está firmado: SmartScreen avisará la primera vez.

**En el PC con Windows** (necesita Rust con toolchain MSVC, Visual Studio Build Tools con C++, Node y WebView2):

```powershell
npm install
npm run tauri build -- --bundles nsis
```

Resultado: `src-tauri\target\release\stream-control.exe` (portable) y el instalador en `src-tauri\target\release\bundle\nsis\`. Para solo probar: `npm run tauri dev`.

## Prueba manual sin OBS

```bash
cd src-tauri
cargo run --example mock_obs        # OBS simulado en 127.0.0.1:4455 (sin contraseña)
```

En otra terminal, desde la raíz: `npm run tauri dev`. En Ajustes hay que elegir el micro («Micro»), añadir las cámaras («Cámara móvil», «Webcam») y marcar escenas rápidas. Otros ejemplos: `panel_dev` (panel web en `http://127.0.0.1:8090`) y `avatar_dev` (avatar en `http://127.0.0.1:4488/avatar?preview=1`).
