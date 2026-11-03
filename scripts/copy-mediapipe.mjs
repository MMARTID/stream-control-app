// Copia el motor WebAssembly de MediaPipe desde node_modules a public/mediapipe/wasm, para que la app
// lo lleve dentro y funcione sin internet. Se ejecuta solo antes de `npm run dev` y `npm run build`.
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const from = join(root, "node_modules", "@mediapipe", "tasks-vision", "wasm");
const to = join(root, "public", "mediapipe", "wasm");
const files = ["vision_wasm_internal.js", "vision_wasm_internal.wasm"];

if (!existsSync(from)) {
  console.error("Falta @mediapipe/tasks-vision. Ejecuta `npm install` primero.");
  process.exit(1);
}
mkdirSync(to, { recursive: true });
for (const f of files) copyFileSync(join(from, f), join(to, f));
console.log(`MediaPipe: ${files.length} archivos copiados a public/mediapipe/wasm`);
