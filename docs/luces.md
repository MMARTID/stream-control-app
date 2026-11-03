# Luces: tiras LED Bluetooth

Pestaña **Luces**: busca tiras LED Bluetooth cercanas, se conecta a una y la controla (encender y apagar, color, brillo, velocidad y las 213 animaciones con su nombre oficial). Funciona con las tiras de la app **Lotus Lamp X** (`com.szelk.ledlamppro`), que se anuncian como `MELK-…` (por ejemplo `MELK-OA10   5F`, con espacios). Todo va por Bluetooth directo del PC: sin nube.

## Cómo se usa
1. Enciende la tira, acércala al PC y **cierra la app Lotus Lamp X del móvil**: la tira solo admite **una conexión a la vez**.
2. **Buscar tiras** (tarda unos 5 segundos). Las que parecen de esta familia salen primero; el resto de dispositivos Bluetooth van en un desplegable por si tu tira usa otro nombre.
3. **Conectar**. Con «Conectarse sola a esta tira al abrir la app» la app la recuerda y se reconecta al arrancar.
4. Controla: encender/apagar, color (selector o colores rápidos), brillo, velocidad y animaciones (con buscador y categorías).

## Cómo está hecho
- El protocolo viene del proyecto [wporter82/lotus-lamp-python](https://github.com/wporter82/lotus-lamp-python) (MIT), que lo obtuvo de la app oficial: servicio `FFF0`, característica de escritura `FFF3` (sin respuesta) y comandos de **9 bytes** (`7E 07 05 03 RR GG BB 10 EF` para un color). Se ha **reescrito en Rust** (`src-tauri/src/lights/protocol.rs`): no se usa Python. Los nombres y categorías de las animaciones (`src-tauri/data/lotus_*.json`) y el aviso de licencia (`src-tauri/data/LICENSE-lotus-lamp-python.txt`) proceden de ese proyecto.
- El Bluetooth lo gestiona la librería `btleplug` (Windows, macOS y Linux): `src-tauri/src/lights/bluetooth.rs`.
- El controlador (`src-tauri/src/lights/mod.rs`) **espacia las órdenes 100 ms** (la tira admite ~10 por segundo) y **junta las seguidas del mismo tipo en la última**: un deslizador, o una futura sincronización con la imagen, pueden pedir valores sin límite sin saturar la tira. Está detrás de dos *traits* para probarlo sin hardware.

## Qué se ha comprobado y qué no
Comprobado: los bytes de cada comando contra la documentación del protocolo; la búsqueda y su orden; el ritmo y la unión de órdenes; que el orden se respeta; la pérdida de conexión; los fallos de conexión; reconectar la tira guardada (11 pruebas + 4 del protocolo, con una tira simulada); y la interfaz en un navegador con una tira simulada.

**No comprobado: nada con una tira ni con Bluetooth real.** No tengo ninguna tira y la capa Bluetooth (`btleplug`) solo se ha compilado, no usado. Lo que hay que mirar primero:
1. Que la tira aparezca en la búsqueda con su nombre (en Windows, el aviso de «dispositivos cercanos» y que el Bluetooth esté encendido).
2. Que conecte y exista el canal `FFF3`. Si no, la app lo dice («no parece una tira Lotus Lamp»). El protocolo se probó con un solo modelo (`MELK-OA10 5F`); la propia librería avisa de que en otros modelos los números de las animaciones pueden variar.
3. En macOS, el aviso de permiso de Bluetooth (`src-tauri/Info.plist` lleva el texto).
4. La reconexión automática al abrir la app.

## Sincronizar la tira con una fuente de OBS (no hecho)
Es viable y encaja bien: la app ya pide capturas a OBS (`GetSourceScreenshot`). Ver la conversación de diseño: se pide una captura minúscula (unos 32 × 18 píxeles) de la fuente elegida, se calcula el color medio, se suaviza y se manda a la tira. Límites: la tira es de **una sola zona** (todo el mismo color, sin zonas por lado), admite ~10 órdenes por segundo y las capturas llegan a unas 3–5 por segundo, así que sirve como luz ambiente suave (no para efectos rápidos); y hay unos 200–400 ms de retraso.
