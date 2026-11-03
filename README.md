# Stream Control (nombre provisional)

> Documento base del proyecto. Define qué construimos, para quién y con qué límites. Lo marcado como **[pendiente]** se decide antes de programar.

## 1. Resumen

App de escritorio **local** que acompaña a **OBS** durante los directos. OBS sigue haciendo el trabajo pesado (captura, codificación y envío del stream). La app añade el control en directo:

- Cambiar entre varias **fuentes de cámara** (móvil, webcam USB, cámara integrada) con un clic.
- Lanzar **escenas rápidas** (p. ej. "Pausa / baño" o "Cambio de tema") con un toque.
- Ver el **chat** del directo dentro de la app.
- Usar el mismo control desde el **móvil y el ordenador**, con un panel web dentro de tu red local, que también puede iniciar y parar el stream (v1).

- **Plataformas:** Windows (prioritario) y macOS (opcional).
- **Arranque:** se abre desde su carpeta sin instalar, o arranca sola al encender el ordenador, junto con OBS.

## 2. Objetivos y no objetivos

**Objetivos**
1. Cambiar de cámara en directo sin cortar el stream.
2. Tener a mano las escenas rápidas y lanzarlas en un toque.
3. Seguir el chat sin salir de la app de control.
4. Arrancar con el ordenador (app y OBS) o ejecutarse desde una carpeta.
5. Usar el mismo control desde el móvil y el ordenador, dentro de la red local (v1).

**No objetivos (por ahora)**
- Sustituir a OBS, codificar vídeo dentro de la app o editar los archivos de OBS.
- Gestionar los drivers de las cámaras: eso lo hace cada app o cada cámara (p. ej. OpenCam para el móvil).
- Overlays complejos, alertas de donaciones o edición de vídeo.
- Acceso desde internet o servicios en la nube: el panel solo funciona dentro de tu red local.
- Enviar mensajes de chat desde la app en la primera versión (solo lectura).

## 3. Supuestos

- OBS es el motor del directo. La app se conecta a él por su WebSocket.
- Todo el directo sale de un mismo ordenador con Windows (OBS + app).
- OBS 28 o superior (incluye el WebSocket de serie).
- Sin modo estudio: la escena cambia en directo con la transición por defecto de OBS.
- Al encender el ordenador se abren OBS y la app, y la app conecta sola con OBS.
- Solo una cámara emite a la vez. La **webcam sustituye al móvil**: no van juntas en el directo, aunque ambas puedan estar conectadas. Cualquier cámara que OBS reconozca sirve como fuente.
- El móvil es **Android** (11 o superior) y usa **OpenCam** ([DerpcatMusic/OpenCam](https://github.com/DerpcatMusic/OpenCam), licencia MIT), que ya funciona bien para el usuario.
- OpenCam se conecta por **USB**, con la depuración USB activada en el móvil. El cable también puede cargar el móvil mientras emite; hay que medir si eso añade calor.
- La cámara virtual de OpenCam en Windows necesita el driver de terceros [Unity Capture](https://github.com/schellingb/UnityCapture).
- El chat es de Twitch y YouTube, y se lee en solo lectura desde el primer día.
- No se usa ningún mando físico (p. ej. Stream Deck); el control es con ratón, teclado y el móvil.
- Un solo usuario, tú, usa el móvil y el ordenador.
- Hay un **panel web en la red local** (v1), servido por la app de escritorio, al que se accede desde el móvil y el ordenador conectados a la misma Wi‑Fi. No hay servicios en la nube ni cuentas externas. El PC debe estar encendido con OBS abierto para que el panel pueda actuar.

## 4. Escenario de uso

1. Enciendes el ordenador. La app y OBS arrancan solos; la app queda en la bandeja.
2. La app se conecta a OBS sola.
3. Empiezas el directo desde la app y la interfaz pasa a **modo directo**.
4. Durante el directo lanzas "Pausa / baño" o "Cambio de tema" desde el panel de escenas rápidas, o desde el móvil con el panel web (v1). Cambias de cámara (móvil o webcam), silencias el micro si hace falta y lees el chat sin mirar OBS.

## 5. Requisitos funcionales

### 5.1 Conexión con OBS
- **RF-1** Conexión con obs-websocket v5. Host, puerto (4455 por defecto) y contraseña configurables.
- **RF-2** Reconexión automática si OBS se cierra o reinicia, sin bloquear la interfaz.
- **RF-3** Si OBS no está abierto, la app espera en segundo plano y conecta cuando aparezca. Opcional: botón para abrir OBS.
- **RF-4** Estado de conexión siempre visible: conectado, reconectando o desconectado.

### 5.2 Fuentes de cámara (móvil, webcam o integrada)
- **RF-5** Listar las cámaras que ve OBS y asignar a cada una un nombre propio (p. ej. "Móvil", "Webcam", "Integrada"). No está limitado al móvil.
- **RF-6** Cambiar de fuente de cámara con un botón o atajo. Diseño recomendado: cada cámara es una fuente separada dentro de la escena y se alterna su visibilidad con transición. Así se evitan cortes al cambiar de dispositivo.
- **RF-7** Mostrar si cada cámara asignada está conectada y avisar al instante si una se desconecta en directo.
- **RF-8** Opcional, desactivado por defecto: si se pierde la cámara activa, pasar a otra cámara asignada.

### 5.3 Escenas: gestión dinámica y visual
- **RF-9** Escenas como tarjetas con **miniatura en vivo** (1–2 actualizaciones por segundo; se pausa cuando no está visible).
- **RF-10** Marcar qué escena está **en programa** (en emisión).
- **RF-11** Cambiar de escena con un clic o atajo. El cambio usa la transición de OBS.
- **RF-12** Ver las fuentes de la escena actual y activarlas o desactivarlas (cámara, pantalla, textos, overlays), y silenciar audios.
- **RF-13** Ordenar las escenas arrastrando. El orden lo guarda la app, no depende del de OBS.
- **RF-14** Atajos de teclado configurables, también globales (funcionan con la app en segundo plano).
- **RF-15** Acciones rápidas en directo: silenciar micro, "BRB" y volver a la escena anterior.

### 5.4 Chat en directo
- **RF-16** Ver el chat con autor, color y distintivos (moderador, suscriptor).
- **RF-17** Autoscroll al último mensaje. Si el usuario sube para leer, se pausa y avisa de los nuevos.
- **RF-18** Filtros: solo menciones, solo moderadores o suscriptores.
- **RF-19** Límite de mensajes en memoria (p. ej. 500) para no degradar la app en directos largos.
- **RF-20** Chat de Twitch y YouTube, ambos en el MVP. Twitch se implementa primero. Solo lectura desde el primer día.
- **RF-21** (v2) Enviar mensajes desde la app. Hasta entonces, el chat es solo de lectura.
- **RF-22** (v2) Destacar un mensaje y mostrarlo en OBS mediante una fuente de navegador.

### 5.5 Modo directo
- **RF-23** Al empezar el stream, la interfaz pasa a un modo grande y legible: escenas rápidas, cámaras, chat y estado visibles a la vez.
- **RF-24** Panel de salud: tiempo en directo, bitrate, FPS, frames perdidos y CPU de OBS.
- **RF-25** Avisos cuando hay problemas: frames perdidos por encima de un umbral, cámara desconectada, stream reconectando.
- **RF-26** Iniciar y parar el stream desde la app. Parar pide confirmación.

### 5.6 Arranque
- **RF-27** **Portable:** se ejecuta desde su carpeta sin instalar.
- **RF-28** **Autoarranque:** opción "Iniciar con el ordenador". Al arrancar, la app va a la bandeja sin abrir ventana y, si está activado, también abre OBS. La ruta de OBS es configurable.
- **RF-29** Instalador opcional en Windows, con acceso en el menú Inicio.

### 5.7 Ajustes
- **RF-30** Ajustes guardados en local.
- **RF-31** Contraseña de OBS y tokens de chat en el almacén seguro del sistema (Credential Manager en Windows, Llavero en macOS), nunca en texto plano.
- **RF-32** (v1) Perfiles, p. ej. "Juego" o "Charla", con escenas y cámaras preseleccionadas.

### 5.8 Escenas rápidas
- **RF-33** Marcar escenas de OBS como **rápidas**, con nombre corto, color e icono. Ejemplos: "Pausa / baño", "Cambio de tema".
- **RF-34** Panel de escenas rápidas siempre visible en modo directo: botones grandes, un toque para lanzar y la escena activa resaltada.
- **RF-35** Atajo de teclado global por cada escena rápida.
- **RF-36** Buscar cualquier escena de OBS por nombre, aunque no sea rápida, para cambiar de tema sin tenerla preparada.
- **RF-37** Acciones asociadas a una escena rápida, p. ej. silenciar el micro al entrar en "Pausa / baño" y reactivarlo al volver a la escena anterior.

### 5.9 Enlace con OBS
- **RF-38** La app lee las escenas de OBS en vivo por obs-websocket. Las escenas rápidas son referencias por nombre a escenas que ya existen en OBS; no se duplican.
- **RF-39** La app no escribe los archivos de colección de OBS, porque OBS los sobrescribe al guardar. La app solo guarda sus propios ajustes (escenas rápidas, atajos y acciones).
- **RF-40** (v1) Exportar e importar la lista de escenas rápidas como archivo (JSON), para copia de seguridad o para llevarla a otro PC.

### 5.10 Panel web en la red local (v1)
- **RF-41** Panel web servido por la app de escritorio, accesible desde el móvil y el ordenador conectados a la misma red local. Muestra las escenas rápidas, las cámaras, el estado y el chat en solo lectura.
- **RF-42** Estado en el panel: si la app y OBS están conectados. Sin conexión, los botones se desactivan y el panel lo dice. Los comandos no se guardan para enviarlos después.
- **RF-43** Acceso con contraseña y un código por dispositivo. El dispositivo puede revocarse desde el escritorio.
- **RF-44** El panel solo acepta conexiones de la red local. No se abre ningún puerto del router ni se expone a internet.
- **RF-45** Lo que muestra el panel sale de la app: lista de escenas rápidas, cámaras, estado y mensajes del chat. Nunca credenciales de OBS ni tokens de chat.
- **RF-46** Comandos desde el panel: lanzar escena rápida, cambiar de cámara, silenciar o activar el micro, volver a la escena anterior, e **iniciar o parar el stream**. Iniciar y parar piden confirmación en dos pasos desde el panel.
- **RF-47** Registro de acciones: cuándo, desde qué dispositivo y qué escena o acción. Visible en el escritorio.
- **RF-48** Conexión cifrada (HTTPS) con certificado local. El navegador avisará la primera vez en cada dispositivo.
- **RF-49** Ajuste en la app para desactivar el inicio y la parada del stream desde el panel. Desactivado, el panel no puede iniciarlo ni pararlo.

## 6. Requisitos no funcionales

- **RNF-1 Rendimiento:** la app no debe quitarle recursos a OBS. Medir el consumo de CPU de OBS con y sin la app.
- **RNF-2 Respuesta:** la orden de cambio de escena o cámara llega a OBS en menos de ~100 ms en red local.
- **RNF-3 Aislamiento:** si la app falla o se cierra, el stream de OBS no se corta.
- **RNF-4 Local-first:** sin telemetría y sin servicios en la nube. Todo se queda en el PC y en tu red local.
- **RNF-5 Seguridad:** la conexión a OBS va a localhost. OBS y el panel web nunca se exponen a internet. Si se usa un host remoto para OBS, es obligatoria la contraseña.
- **RNF-6 Uso en directo:** botones grandes, sin ventanas emergentes que tapen la pantalla.
- **RNF-7 Idioma:** interfaz y documentación en español.

## 7. Criterios de aceptación

### 7.1 MVP
- [ ] Cambio de fuente de cámara en directo (móvil ↔ webcam) sin pantalla negra visible durante más de un segundo.
- [ ] Lanzar "Pausa / baño" desde el panel de escenas rápidas en un toque, con el micro silenciado, y volver a la escena anterior.
- [ ] Buscar y lanzar una escena que no es rápida durante el directo.
- [ ] Si OBS se cierra y se vuelve a abrir, la app se reconecta sola sin reiniciarse.
- [ ] Todas las escenas muestran una miniatura actualizada.
- [ ] El chat de Twitch y de YouTube se ve en directo y la app sigue fluida tras 2 horas de chat activo.
- [ ] Al encender el ordenador, OBS y la app arrancan solos y la app conecta con OBS sin intervención.
- [ ] La app se ejecuta desde su carpeta sin instalación.
- [ ] El consumo de CPU de OBS no sube de forma apreciable con la app abierta.
- [ ] La cámara de OpenCam por USB funciona en OBS en Windows durante una hora de directo.
- [ ] La webcam USB funciona en directo como sustituta del móvil durante una hora.

### 7.2 Panel web en la red local (v1)
- [ ] Lanzar "Pausa / baño" desde el móvil, en la misma Wi‑Fi, en menos de 2 segundos.
- [ ] Desde el ordenador, el mismo panel muestra y lanza las mismas escenas.
- [ ] Con el PC apagado o sin OBS, el panel muestra "sin conexión" y no envía comandos.
- [ ] Iniciar o parar el stream desde el móvil pide confirmación en dos pasos.
- [ ] Con el control de stream desactivado en la app, el panel no puede iniciar ni parar el directo.
- [ ] Un dispositivo revocado deja de poder enviar comandos al instante.
- [ ] Cada acción aparece en el registro con fecha, dispositivo y escena.

## 8. Arquitectura propuesta

```
┌──────────────────────── Stream Control ──────────────────────────┐
│  Interfaz: escenas rápidas · cámaras · chat · salud · ajustes     │
│                              │                                    │
│  Núcleo: estado · acciones · atajos · arranque · bandeja          │
│     ├── Adaptador OBS ─── WebSocket (obs-websocket v5) ───► OBS   │
│     ├── Adaptador chat ── Twitch / YouTube (API o IRC)            │
│     ├── Panel web (v1) ── red local ── móvil y ordenador          │
│     └── Almacén ───────── ajustes (JSON) + almacén seguro del SO  │
└──────────────────────────────────────────────────────────────────┘
```

Principios:
- **OBS es la fuente de verdad del directo.** La app refleja su estado y le envía órdenes; no lleva su propia copia del stream ni edita sus archivos.
- **Cada integración es un adaptador aislado.** Si falla el chat, las escenas siguen funcionando.
- **Las cámaras son fuentes genéricas.** La app no distingue el tipo de cámara; solo el nombre que le asigna el usuario. Así una webcam y un móvil se tratan igual.
- **Todo queda en tu red.** El panel web solo se sirve dentro de la red local; no hay nada en la nube.

**Stack: Tauri 2** (decidido). Núcleo en Rust, interfaz web en TypeScript. Consume menos memoria y CPU que Electron, algo importante con OBS en el mismo PC. Ofrece plugins para autoarranque, bandeja, atajos globales y empaquetado para Windows y macOS. El escritorio de OpenCam también está en Rust, lo que facilita reutilizar código o ideas.

## 9. Integraciones

| Integración | Para qué | Notas |
|---|---|---|
| OBS (obs-websocket v5) | Escenas, fuentes, audio, stream, miniaturas | Incluido en OBS 28+. Puerto 4455 y contraseña por defecto. Se puede abrir desde la app al arrancar. |
| Twitch | Chat | IRC o EventSub. Requiere autorizar la cuenta (OAuth). |
| YouTube | Chat | YouTube Data API v3 (liveChatMessages). Tiene cuota diaria y necesita OAuth. |
| OpenCam (Android → PC, MIT) | Entrega la cámara del móvil a OBS como cámara virtual | Externa. Conexión por USB con depuración USB activada. En Windows usa Unity Capture. Sin audio ni streaming en segundo plano todavía. |
| Webcam USB (u otra cámara que OBS reconozca) | Fuente de cámara | Sin integración propia: OBS la gestiona como dispositivo de captura de vídeo. |
| Panel web en red local (v1) | Ver y controlar escenas, cámaras y stream desde el móvil y el ordenador | Servido por la app. Sin servicios externos ni cuentas. |

Los nombres exactos de peticiones y eventos se comprueban contra la documentación de la versión instalada antes de implementar.

## 10. Roadmap

**Fase 1 · MVP**
- Conexión a OBS con reconexión automática.
- Escenas con miniaturas e indicador de programa.
- Escenas rápidas: panel visible en directo, un toque, búsqueda por nombre y silenciar micro al entrar.
- Fuentes de cámara (móvil, webcam, integrada): selección, cambio y estado de conexión.
- Silenciar micro, iniciar y parar stream, salud básica.
- Chat de Twitch y YouTube en solo lectura.
- Autoarranque (app y OBS), bandeja y ejecución portable en Windows.

**Fase 2 · v1**
- Panel web en la red local para el móvil y el ordenador, con escenas, cámaras, chat en solo lectura y control del stream (RF-41 a RF-49).
- Exportar e importar escenas rápidas (RF-40).
- Filtros del chat (menciones, moderadores, suscriptores).
- Atajos globales y acciones rápidas.
- Modo directo completo con avisos.
- Cambio automático a otra cámara si se pierde la activa (opcional).
- Perfiles.

**Fase 3 · v2**
- Versión para macOS.
- Enviar mensajes desde la app y destacarlos en OBS.
- Instalador firmado y actualizaciones automáticas.

## 11. Riesgos

| Riesgo | Mitigación |
|---|---|
| Cambios en obs-websocket o en las APIs de chat | Adaptadores aislados; fijar versiones probadas. |
| Cuota de la API de YouTube agotada | Intervalo de sondeo razonable; aviso en la app. |
| OpenCam probado sobre todo en Linux; Windows y macOS aún con pruebas limitadas | Prueba de concepto en Windows antes de cerrar el MVP. |
| OBS sobrescribe sus archivos de escenas al guardar | La app no edita esos archivos; trabaja por obs-websocket (RF-39). |
| Otro dispositivo de la red local accede al panel sin permiso | Contraseña y código por dispositivo, revocación y registro de acciones (RF-43, RF-47). |
| Iniciar o parar el directo por error desde el móvil | Confirmación en dos pasos (RF-46), registro (RF-47) y opción para desactivarlo (RF-49). |
| El panel depende del PC encendido y de la misma Wi‑Fi | Estado real en el panel (RF-42); los comandos no se guardan para más tarde. |
| Aviso de certificado en el navegador del móvil | Certificado local que se acepta una vez por dispositivo (RF-48). |
| Desconexión del cable USB durante el directo | Aviso inmediato (RF-7); reconexión automática de OpenCam; usar un cable de datos de buena calidad. |
| Corte de imagen al cambiar de móvil a webcam en directo | Transición preparada en OBS; probar el cambio antes del directo. |
| Calor del móvil: USB también carga el móvil y OpenCam necesita la app abierta (sin streaming en segundo plano) | Medir la temperatura con y sin carga; ajustar resolución y fps; documentar la recomendación. |
| Driver Unity Capture de terceros para la cámara virtual en Windows | Documentar la instalación; comprobar compatibilidad con la versión de Windows del usuario. |
| Miniaturas que consumen CPU y afectan al stream | Límite de frecuencia; pausar fuera de vista; medir antes de cerrar la v1. |
| SmartScreen de Windows con el ejecutable sin firmar | Aviso en la documentación; firma de código en v2. |
| Autoarranque: OBS puede tardar en estar listo al encender el ordenador | Reintentos en segundo plano con espera; aviso en la bandeja; ruta de OBS configurable. |
| OpenCam es un proyecto reciente (último cambio el 19/10/2026) y puede cambiar | Fijar la versión probada; revisar el repositorio antes de actualizar. |

## 12. Preguntas abiertas

Ninguna por ahora. Las decisiones tomadas están repartidas en las secciones 3, 5 y 8.

## 13. Estructura propuesta (cuando empiece el código)

```
stream-control-app/
├── README.md
├── docs/                   decisiones y notas de integración
├── src/                    interfaz (TypeScript)
└── src-tauri/              núcleo en Rust (Tauri 2)
    ├── obs/                adaptador obs-websocket
    ├── chat/               adaptadores de chat (Twitch, YouTube)
    ├── web/                panel web en la red local (v1)
    └── settings/           ajustes, escenas rápidas y almacén seguro
```

## 14. Próximos pasos

1. Prueba de concepto en Windows con Tauri 2 (MVP): conectar con obs-websocket, lanzar escenas rápidas y cambiar de cámara a mano, alternando OpenCam por USB y una webcam.
2. Probar el arranque automático de app y OBS al encender el ordenador.
3. Probar el chat de Twitch y el de YouTube en solo lectura.
4. Diseñar el panel web (v1): contraseña por dispositivo, comandos permitidos, confirmación de inicio y parada, y HTTPS local, antes de programarlo.
5. Cerrar el alcance exacto del MVP y crear el proyecto.
