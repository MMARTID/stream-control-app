# Avatar animado

Un personaje de dibujo que mueve la boca cuando hablas y, si quieres, copia tu cara (ojos, cejas, cabeza). Se muestra en OBS como una **fuente de navegador** con fondo transparente. Todo se calcula en tu PC: no se envía ninguna imagen ni audio a ningún sitio.

## Para qué sirve

Es una **escena aparte** («Avatar»). En ella el avatar queda **pequeño, abajo a la izquierda**, en lugar de tu cámara: para una cinemática o cuando no quieres ocupar tanto espacio en el stream. Cambias a esa escena con un toque (también desde el móvil, porque se añade a las escenas rápidas) y, al salir de ella, vuelves a tu cámara.

## Cómo se usa

1. Pestaña **Avatar** → «Activar el avatar».
2. Elige el personaje (Jason o Cal).
3. Elige **qué seguir**:
   - **Mi cara (cámara):** sigue la boca, los ojos, las cejas y la inclinación y giro de la cabeza. Necesita una cámara que **OBS no esté usando**: en Windows, una cámara normal no se puede compartir entre dos programas. Si solo tienes la cámara que usas en OBS, usa el modo de voz.
   - **Mi voz (micrófono):** la boca se abre con el volumen de tu voz, parpadea sola y mueve un poco la cabeza al hablar. No usa cámara.
4. **«Crear la escena «Avatar» en OBS»**. Parte de la escena que tengas en pantalla: copia sus fuentes **menos las cámaras** (el juego, marcos, textos…) con su posición y tamaño, y añade el avatar abajo a la izquierda (a un 34 % del alto de la pantalla, con un 2 % de margen). Además la añade a tus escenas rápidas. Nunca modifica una escena que ya exista: si ya hay una llamada «Avatar», lo dice. El nombre se puede cambiar antes de crearla. Hay otras dos formas en «Otras formas de añadirlo»: poner solo el avatar en la escena actual, o crear la fuente de navegador a mano en OBS con **+ → Navegador**, la dirección `http://127.0.0.1:4488/avatar` y tamaño **800 × 880**.
5. **Seguimiento.** Por defecto la app **solo sigue tu cara o tu voz mientras la escena «Avatar» está en pantalla** (casilla recomendada): empieza sola al cambiar a ella y se para al salir. Así la cámara queda libre para OBS en el resto de escenas y no se gasta procesador de más. Si usas la misma cámara en OBS y en el avatar, en OBS abre las propiedades de esa cámara y marca **«Desactivar cuando no se muestra»**; si OBS aún la tiene al cambiar de escena, la app reintenta unos segundos. «Probar el seguimiento ahora» lo arranca a mano (y no se para solo). Con «Calibrar» (modo cámara) se toma tu posición actual como «mirando al frente».

Si no hay seguimiento, el personaje no se queda congelado: respira, se balancea y parpadea.

## Cómo funciona

```
cámara o micro ──► ventana de la app (MediaPipe en local) ──► núcleo en Rust ──► WebSocket ──► página /avatar ──► OBS
```

- El seguimiento se hace **en la ventana de la app** (`src/avatar/tracker.ts`) con el modelo *Face Landmarker* de MediaPipe, que va dentro de la app (`public/mediapipe/`).
- Los valores (boca, sonrisa, ojos, cejas, giro, cabeceo, inclinación) viajan al núcleo en Rust (`src-tauri/src/avatar.rs`), que los acota y los reparte por WebSocket.
- Un servidor **solo en 127.0.0.1** (puerto 4488 por defecto) sirve la página `/avatar` y los personajes. Comprueba `Host` y `Origin`, y el WebSocket es de solo lectura: los datos solo entran por la app.
- La página (`src-tauri/web/avatar.js`) dibuja el SVG y lo anima. Es la misma que ve OBS y la vista previa de la app.

**Mientras sigues la cara, la app no debe dormirse.** WebView2 frena los temporizadores de una ventana minimizada, así que la ventana se crea con `--disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows` y el seguidor mantiene un *Web Lock*. Esto es lo que más conviene vigilar en Windows (ver abajo).

## Los personajes

Son dibujos propios, en estilo de caricatura realista, **inspirados** en el aspecto de dos personajes masculinos de GTA VI:

- **Jason**: pelo corto y oscuro, barba corta, físico atlético, camisa estampada abierta sobre camiseta blanca.
- **Cal**: sombrero de pescador, barba grande y pelirroja, ojos azules y un tatuaje de un pez y una estrella en el brazo.

Los rasgos salen de descripciones públicas del juego (rol, complexión, barba, sombrero, tatuajes en el brazo izquierdo, camisa estampada), **no de imágenes del juego**, que no he visto. Es una aproximación: para acercarlos más, pásale a Claude capturas de referencia de cada personaje y los retoca. No son arte oficial de Rockstar Games ni de Take-Two; si vas a emitir o repartir la app con ellos, valora tú el uso de la imagen de unos personajes con derechos de autor.

Los personajes son archivos SVG (`src-tauri/web/avatars/jason.svg` y `cal.svg`). Para poner otro dibujo basta con respetar los identificadores que se animan (explicados en un comentario al principio de cada SVG) y añadir su nombre a `AVATAR_CHARACTERS` en `src-tauri/src/settings/mod.rs`, a `character_svg` en `src-tauri/src/avatar.rs` y a `CHARACTERS` en `src/views/avatar.ts`.

## Qué se ha comprobado y qué no

Comprobado:
- El servidor, la página, el WebSocket, el cambio de personaje, el contador de páginas conectadas, el cierre limpio y las protecciones: pruebas automáticas (`src-tauri/tests/avatar.rs`, 13, incluida la creación de la escena con su orden de fuentes y su posición) y a mano en un navegador.
- El dibujo y la animación de los dos personajes (boca con dientes y lengua, ojos, cejas, giro, inclinación) en un navegador.
- El modelo y el motor WebAssembly **cargan y ejecutan** (con GPU y con CPU) y detectan la cara de los propios dibujos: devuelven los 52 gestos con los nombres que usa el código (`jawOpen`, `eyeBlinkLeft`…) y la matriz de la cabeza (16 valores).
- Los signos de la inclinación y de los ojos, y el del giro (que estaba al revés y se corrigió), con imágenes sintéticas: dibujos del avatar girados o desplazados pasados por MediaPipe. Las conversiones numéricas tienen 16 pruebas (`npm test`).

**No comprobado:**
1. **Seguimiento con una cara y una cámara reales.** No tenía cámara. Todo lo anterior se ha hecho con imágenes sintéticas. El sentido del cabeceo (arriba/abajo) es la medición más débil: si el avatar mira al revés, hay una casilla «Invertir arriba/abajo».
2. **Permisos de cámara y micrófono en WebView2** (el aviso de Windows y de la ventana) y los mensajes de error asociados.
3. **Que la ventana siga siguiendo la cara minimizada** en Windows, con los argumentos de WebView2 y el *Web Lock* descritos arriba.
4. **Consumo de CPU y GPU** del seguimiento junto a OBS (README, RNF-1). Se ejecuta a unos 30 fotogramas por segundo.
5. **La escena del avatar en OBS real.** La app usa `CreateScene`, `GetVideoSettings`, `CreateInput` (`browser_source`, con los ajustes `url`, `width`, `height`, `fps`), `CreateSceneItem`, `GetSceneItemTransform` y `SetSceneItemTransform`. La documentación de obs-websocket no detalla los campos de la transformación (`positionX`, `positionY`, `scaleX`, `scaleY`…) ni dice en qué orden devuelve `GetSceneItemList` las fuentes; se asume que los elementos nuevos nacen con el ancla arriba a la izquierda y que la lista va de abajo arriba. Si el avatar sale en otro sitio o la escena copiada tiene las fuentes en otro orden, es lo primero que hay que revisar. También: que el fondo sea transparente y se vea nítido.
5b. **El seguimiento automático por escena**: que OBS suelte la cámara al salir de la escena (casilla «Desactivar cuando no se muestra») y el tiempo que tarda. Está probada la lógica de cuándo empezar y parar (7 pruebas), no con una cámara real.
6. **El modo de voz con un micrófono real**: la sensibilidad y el aprendizaje del ruido de fondo se han probado solo con números.
7. **macOS**: `src-tauri/Info.plist` declara los textos de permiso de cámara y micrófono, que Tauri incrusta en el modo desarrollo y en el paquete. Si la app se lanza desde otra aplicación (no desde la Terminal), macOS puede no mostrar el aviso de permiso. Sin probar con una cámara real.
