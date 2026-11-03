# Panel web en la red local

Diseño y uso del panel (README §5.10, RF-41 a RF-50). Lo sirve la app de escritorio; no hay nube ni cuentas.

## Cómo se usa

1. En la app: **Ajustes → Panel web para el móvil**. Marca «Activar el panel», elige la contraseña (mínimo 8 caracteres) y pulsa **Guardar**.
2. La app muestra una dirección del estilo `https://192.168.1.12:8443` y un código QR. Ábrela en el móvil, conectado a la **misma Wi‑Fi**.
3. El navegador avisará del certificado (es propio de la app): «Avanzado» → «Continuar». Solo la primera vez en cada dispositivo.
4. Escribe la contraseña y el nombre del móvil. En el escritorio aparece un **código de 6 dígitos** (también en un aviso en la pantalla principal). Escríbelo en el móvil.
5. Listo. En Chrome/Samsung Internet se puede «Añadir a la pantalla de inicio» para abrirlo como una app.

**Windows:** la primera vez que se activa el panel, el Firewall de Windows pregunta si permite que la app reciba conexiones. Hay que permitirlo en **redes privadas**. Si el móvil no llega, comprueba que la Wi‑Fi no tiene activado el «aislamiento de clientes» (AP isolation).

## Qué hace el panel

- **Salida en programa** y miniaturas de cada cámara y de cada fuente de la escena. Son *capturas* de OBS (unas 3 por segundo la salida, 1 por segundo cada fuente), no vídeo.
- **Escenas rápidas** (un toque, con el silencio de micro configurado), volver a la anterior, buscar y lanzar cualquier escena.
- **Cámaras**: cambiar con un toque (se muestra la nueva y después se oculta la anterior).
- **Fuentes** de la escena: mostrar u ocultar.
- **Micro**: silenciar o activar. **Salud**: bitrate, frames perdidos, FPS, CPU de OBS, tiempo en directo.
- **Iniciar y parar el directo**, en dos pasos (ver abajo) y solo si se permite en Ajustes.
- Barra inferior con micro, escena anterior y directo, al alcance del pulgar. Si se pierde la conexión con el PC o con OBS, los botones se desactivan y el panel lo dice: **los comandos no se guardan para enviarlos después** (RF-42).

Cuando el panel está en segundo plano o la pantalla se apaga, deja de pedir capturas, así que no gasta recursos de OBS ni del móvil.

## Seguridad

| Requisito | Cómo se cumple |
|---|---|
| RF-43 contraseña + código por dispositivo | Contraseña (hash Argon2id en el almacén seguro del sistema) → código de 6 dígitos que **solo se ve en el escritorio** → token propio del dispositivo (solo se guarda su hash). Cinco fallos seguidos bloquean esa IP un minuto, duplicándose hasta 15 minutos. El código caduca a los 2 minutos y se anula tras 5 intentos. |
| RF-43 revocar | «Revocar» en el escritorio borra el dispositivo, corta su conexión abierta y sus comandos dejan de funcionar al instante. |
| RF-44 solo red local | Se rechazan las conexiones que no vienen de una IP local (10/8, 172.16/12, 192.168/16, enlace local, loopback). Además la cabecera `Host` debe ser una IP, `localhost` o el nombre del equipo (contra *DNS rebinding*) y el `Origin`, si viene, debe coincidir. **La app no abre puertos en el router**: no hagas redirección de puertos hacia ella. |
| RF-45 sin credenciales | El móvil solo recibe `WebState` (`src-tauri/src/panel/web_state.rs`), copiado campo a campo. Una prueba comprueba que no contiene contraseñas, dirección de OBS ni códigos. |
| RF-46 iniciar/parar en dos pasos | El servidor lo exige: `stream_arm` devuelve una confirmación de un solo uso (10 s, ligada al dispositivo) y solo `stream_confirm` ejecuta la orden. |
| RF-47 registro | Cada acción (y cada fallo) con fecha, dispositivo, IP y detalle, en `panel-audit.jsonl` y en el escritorio. |
| RF-48 HTTPS | Certificado autofirmado generado por la app; la clave privada va al almacén seguro del sistema, no a un archivo. Si cambia la IP del PC, se genera uno nuevo y los móviles volverán a avisar. |
| RF-49 desactivar el stream | Casilla en Ajustes. **Viene desactivada**: hay que activarla expresamente. |

Los ajustes del panel y los dispositivos están en la carpeta de configuración de la app (`settings.json`, `panel-devices.json`, `panel-audit.jsonl`, `panel-cert.pem`).

## Decisiones propias (no vienen en el README)

- El panel viene **desactivado** y con el control del stream **desactivado** hasta que se activen en Ajustes.
- Además de las escenas rápidas, el panel puede lanzar **cualquier escena de OBS** (buscador). Hay una casilla «solo escenas rápidas» para restringirlo.
- Puede ocultar/mostrar fuentes de la escena (RF-12), que el README no lista entre los comandos del panel (RF-46).
- No se ha implementado el **chat** en el panel (RF-41), porque el chat de Twitch y YouTube aún no existe.

## Qué no se ha comprobado

- **Nada de esto se ha probado en un móvil real ni en Windows.** Se probó con pruebas automáticas (TLS real incluido) y con el navegador en tamaño de móvil contra un OBS simulado, sin HTTPS (el navegador integrado rechaza el certificado autofirmado).
- **WebSocket seguro con certificado autofirmado en el móvil.** Chrome en Android debería aceptarlo tras aceptar el aviso de la página; Safari en iPhone a veces lo rechaza. Si el panel entra pero se queda en «Conectando con el PC…», es lo primero que hay que mirar.
- **Capturas de OBS reales**: formato JPEG, fuentes ocultas o cámaras desconectadas (que pueden dar imagen negra o error) y el consumo de CPU de OBS con el panel abierto (README §11) están sin medir.
- **Rendimiento de las capturas**: con muchas fuentes visibles el ritmo baja solo (se piden de una en una), pero no se ha medido.
- **Duplicidad de nombres de fuente**: las capturas se piden por nombre; dos fuentes con el mismo nombre en distintas escenas no se distinguen.
- **Pantalla dividida con OpenCam en el Samsung** (criterio 7.2): sin probar.

## Archivos

- `src-tauri/src/panel/`: `mod.rs` (ciclo de vida y estado para el escritorio), `http.rs` (rutas, WebSocket, comandos), `auth.rs`, `audit.rs`, `tls.rs`, `web_state.rs`.
- `src-tauri/web/`: la página del móvil (HTML, CSS y JavaScript sin dependencias ni paso de compilación; se incrusta en el ejecutable).
- `src-tauri/src/preview.rs`: capturas. `src-tauri/tests/panel.rs`: pruebas del panel.
- `cargo run --example panel_dev -- 8090 --plain`: panel con un OBS simulado en `http://127.0.0.1:8090` (solo para pruebas; contraseña `contraseña-demo`, los códigos se imprimen en la terminal).
