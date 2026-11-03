# Cómo actualizar Stream Control

**No hace falta desinstalar nada ni volver a configurarlo.** Cierras la app, pones la versión nueva y listo. Tus ajustes (escenas rápidas, cámaras, micro…), la contraseña de OBS y los móviles emparejados se conservan, porque no están dentro del programa:

| Qué | Dónde está |
|---|---|
| Ajustes, dispositivos del panel, registro de acciones, certificado | `%APPDATA%\app.streamcontrol.desktop\` (pega eso en el Explorador de archivos) |
| Contraseña de OBS, contraseña del panel y clave del certificado | Administrador de credenciales de Windows (almacén seguro) |

## 1. Mira qué versión tienes

Arriba, junto al nombre: **Stream Control v0.2.0**.

## 2. Descarga la nueva

En GitHub, en el repositorio de Stream Control:

- **Releases** (a la derecha de la página principal): cada versión publicada trae dos archivos.
  - `Stream Control_X.Y.Z_x64-setup.exe`: el instalador.
  - `Stream-Control_X.Y.Z_portable.exe`: la versión que se ejecuta sin instalar.
- Si todavía no hay una Release: pestaña **Actions** → la última ejecución verde de «Build de Windows» → al final, **Artifacts** → `stream-control-windows`. Descarga el `.zip` y descomprímelo.

## 3. Instálala encima

**Si usas el instalador:** cierra Stream Control y ejecuta el instalador nuevo. Instala sobre la versión anterior y mantiene todo. Si te pregunta por borrar los datos de la aplicación, **déjalo sin marcar**.

**Si usas la versión portable:** cierra Stream Control, copia el `.exe` nuevo donde tenías el antiguo y bórralo (o déjalos los dos). Al abrir el nuevo, ya tiene tus ajustes.

La primera vez, Windows SmartScreen puede avisar de que el programa «no es reconocido» (aún no está firmado): pulsa **Más información → Ejecutar de todas formas**.

## 4. Comprueba que va bien

- Arriba debe poner la versión nueva y «Conectado a OBS».
- Si algo falla, vuelve a poner el `.exe` anterior: los ajustes siguen siendo compatibles hacia atrás. Antes de actualizar puedes copiar la carpeta de ajustes (arriba) como seguro.

## No hay actualización automática (todavía)

Es una decisión de privacidad: la app no se conecta a internet por su cuenta (README, RNF-4). Las actualizaciones automáticas firmadas están previstas para la versión 2.

---

## Para quien publica una versión nueva

1. Sube el número de versión en tres sitios: `package.json`, `src-tauri/Cargo.toml` y `src-tauri/tauri.conf.json` (por ejemplo a `0.3.0`).
2. Haz commit y súbelo a `main`.
3. Crea y sube una etiqueta con ese número: `git tag v0.3.0 && git push origin v0.3.0`.
4. GitHub Actions ejecuta las pruebas en Windows, compila (unos 10–15 minutos) y publica la Release con el instalador y el portable.
