# Modelo de reconocimiento facial

`face_landmarker.task` es el modelo **Face Landmarker** de [MediaPipe](https://ai.google.dev/edge/mediapipe/solutions/vision/face_landmarker) (Google, licencia Apache-2.0).

- Origen: `https://storage.googleapis.com/mediapipe-models/face_landmarker/face_landmarker/float16/1/face_landmarker.task`
- Tamaño: 3.758.596 bytes
- SHA-256: `64184e229b263107bc2b804c6625db1341ff2bb731874b0bcc2fe6544e0bc9ff`

Se ejecuta en local dentro de la app: no se envía ninguna imagen a ningún sitio. El motor WebAssembly
(`wasm/`) se copia desde `node_modules/@mediapipe/tasks-vision` con `scripts/copy-mediapipe.mjs`.
