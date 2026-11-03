# Estado del MVP y pruebas pendientes

Última actualización: 2026-11-03. El README es la especificación; este archivo dice qué hay hecho y qué falta comprobar.

## Qué hay

- **Cliente obs-websocket v5** (`src-tauri/src/obs/`): handshake, autenticación SHA-256, reconexión con espera creciente (1 s → 5 s), eventos y peticiones con tiempo límite.
- **Núcleo** (`src-tauri/src/control.rs`): refleja el estado de OBS (escenas, escena en programa, escena anterior, stream, micro, cámaras) y ejecuta las acciones: lanzar escena, escena rápida con silencio de micro (RF-37), volver a la anterior, cambiar de cámara (RF-6), micro, iniciar/parar stream.
- **Ajustes** (`src-tauri/src/settings/`): JSON local en la carpeta de configuración de la app; contraseña de OBS en el almacén seguro del sistema.
- **Interfaz** (`src/`): TypeScript sin framework. Salida en programa y miniaturas de cámaras y fuentes, salud del directo, panel de escenas rápidas, cámaras, fuentes con interruptor, búsqueda de escenas, micro, iniciar/parar stream (parar pide confirmación en línea) y ajustes, incluida la gestión del panel web.
- **Capturas** (`src-tauri/src/preview.rs`): `GetSourceScreenshot` de la salida y de cada fuente, de una en una, a ~3 y 1 por segundo, solo mientras alguien mira. No es vídeo.
- **Panel web de red local** (`src-tauri/src/panel/`, `src-tauri/web/`): HTTPS, emparejamiento con código, comandos, registro y revocación. Detalle, decisiones y límites en `docs/panel-web.md`.

Peticiones de obs-websocket usadas: `GetVersion`, `GetSceneList`, `SetCurrentProgramScene`, `GetInputList`, `GetInputMute`, `SetInputMute`, `GetSceneItemList`, `SetSceneItemEnabled`, `GetStreamStatus`, `StartStream`, `StopStream`. Eventos: `CurrentProgramSceneChanged`, `SceneListChanged`, `SceneCreated`, `SceneRemoved`, `SceneNameChanged`, `StreamStateChanged`, `InputMuteStateChanged`, `SceneItemEnableStateChanged`.

## Cómo se ha probado

- `cargo test` en `src-tauri/`: unas 55 pruebas (unitarias y de integración) contra un **OBS simulado** (`src-tauri/tests/common/mod.rs`). Cubren conexión, autenticación, contraseña incorrecta, reconexión, escenas rápidas, micro, cámaras (orden: se enciende la nueva antes de apagar la anterior), stream, capturas y salud, y que la contraseña no llega al JSON ni a la interfaz. Del panel: emparejamiento, bloqueo de intentos, cabeceras de seguridad, `Host` ajeno, dos pasos del stream, revocación inmediata, que el estado del móvil no filtre datos internos, y un servidor HTTPS real.
- La app se lanzó en macOS con `npm run tauri dev` contra ese OBS simulado (`cargo run --example mock_obs`): conectó y envió órdenes reales (escenas rápidas con silencio de micro y vuelta, cambio de cámara).

- El panel web se probó en un navegador con tamaño de móvil (sin HTTPS, porque el navegador integrado rechaza el certificado autofirmado) contra el OBS simulado: emparejamiento, escena rápida con micro, volver, iniciar el stream en dos pasos, cámaras, salud y fuentes.

## Lo que NO se ha comprobado (hacerlo primero en el PC con Windows)

1. **Nada se ha probado en Windows.** Es la plataforma prioritaria.
2. **Nada se ha probado contra un OBS real.** El simulador usa los nombres de campo de la documentación (`docs/generated/protocol.md` de obs-websocket, rama `master`), no observados en un OBS en marcha. En concreto, la documentación no detalla los campos de los elementos de lista, y se asumen estos de obs-websocket v5: `sceneName`, `sourceName`, `sceneItemId`, `sceneItemEnabled`, `inputName`, `inputKind`. Hay que confirmarlos con la versión de OBS instalada (README, §9).
3. **Orden de las escenas:** se muestran en el orden en que las devuelve `GetSceneList`. No se ha comprobado si coincide con el de la interfaz de OBS o va al revés. El README (RF-13) dice que el orden definitivo lo guarda la app; aún no está hecho.
4. **Almacén seguro de Windows (Credential Manager):** solo se ha compilado la parte de macOS. En Windows ejecutar `cargo test keyring_real -- --ignored` y guardar una contraseña desde Ajustes.
5. **Imagen sin cortes al cambiar de cámara** (criterio de aceptación 7.1): hay que medirlo con OBS real y la transición de OBS.

6. **Panel web en un móvil real** (HTTPS con certificado autofirmado, WebSocket seguro, Firewall de Windows) y **capturas con OBS real** (imagen de fuentes ocultas, consumo de CPU de OBS). Lista completa en `docs/panel-web.md`, «Qué no se ha comprobado».

## Pendiente

- Miniaturas de *todas las escenas* de la lista (RF-9); hoy hay captura de la salida y de las fuentes de la escena en programa.
- Estado de conexión de cada cámara y aviso si se desconecta (RF-7).
- Aviso proactivo de frames perdidos o stream reconectando (RF-25): hoy solo se ve en los colores de la salud.
- Chat de Twitch y YouTube en solo lectura.
- Autoarranque con el ordenador (app y OBS), bandeja y ejecución portable (RF-27, RF-28).
- Atajos de teclado, incluidos los globales (RF-14, RF-35).
- Orden de escenas arrastrable (RF-13).
- Botón para abrir OBS (RF-3, opcional).

El chat en el panel web depende de que exista el chat de Twitch y YouTube.

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
