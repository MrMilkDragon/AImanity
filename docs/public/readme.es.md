# AImanity 0.0.5 Europium

[English](../../README.md) · [Todos los idiomas](../../README.md) · Apache-2.0

## Descargas y estado

[macOS 0.0.5 Europium (DMG)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-macos-arm64.dmg) · [Windows 0.0.5 Europium (EXE)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-windows-x64-setup.exe) · [Linux (DEB)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-linux-amd64.deb) · [Linux (RPM)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-linux-x86_64.rpm)

Requiere macOS 15.0 o posterior con Apple Silicon, o Windows 11 x64. Cada instalador incluye Android, Linux, el emulador y los servicios locales.

Linux x86_64 requiere KVM y glibc 2.36 o posterior. Elige DEB para Debian/Ubuntu o RPM para Fedora/Enterprise Linux 10. Para actualizar y conservar los dispositivos y chats, instala el nuevo paquete en el mismo formato. [Comandos de instalación](release-status.md#linux)

## Funciones y escenarios

AImanity reúne un teléfono Android y un escritorio Linux con una sola conversación y un único agente activo. Guarda otros perfiles de modelo y elige cuál usar en Configuración.

- Usar aplicaciones como WeChat mediante su pantalla y notificaciones reales, y responder según las instrucciones del usuario. Las restricciones de cuenta y aplicación siguen vigentes.
- Recibir un archivo en el teléfono, transferirlo a Linux, procesar y comprobar una hoja de cálculo y devolver el resultado. Transferencia entre dispositivos y entrega a un contacto se verifican por separado.
- Añadir instrucciones e interrumpir la generación sin perder mensajes ni acciones. Las notificaciones pueden despertar al Agente; Liberty funciona en ambos dispositivos.
- Compartir Skills, documentos de conocimiento vinculados y archivos de trabajo. Modelos/API, memoria y conexiones externas pertenecen a los Agentes. Los dispositivos se conectan directamente de forma predeterminada. Puedes configurar un proxy en el espacio de trabajo.

Las acciones se vinculan a observaciones, dispositivo, tarea, época de control e identificador único. Un resultado desconocido se observa antes de repetir.

## Arquitectura y datos

`apps/native`: Rust (Iced/wgpu). `crates`: Rust/Axum/Tokio. `packages/protocol`: protobuf. Los datos se guardan en PostgreSQL mediante SQLx. Cargo compila el proyecto.

Código y datos de usuario están separados. No se publican discos, cuentas, chats, adjuntos ni claves. Las actualizaciones requieren artefactos versionados, copias de seguridad, migraciones y reversión; no un `git pull` directo sobre los datos del usuario.

## Futuro, responsabilidad y licencia

La hoja de ruta incluye más proveedores de voz, habilidades para aplicaciones y tareas de oficina que se inician al recibir una notificación.

Interfaz y configuración inicial admiten diez idiomas. El selector a la izquierda de Skills cambia la interfaz; no traduce automáticamente el sistema invitado ni las salidas originales. El árabe usa escritura de derecha a izquierda.

El usuario responde de permisos, cuentas, acciones externas y condiciones de las aplicaciones. No usar para engaño, fraude ni acciones sin consentimiento. Código original Apache-2.0; los terceros conservan sus licencias.

[Estado](release-status.md) · [Arquitectura](architecture.en.md) · [Compatibilidad](compatibility.md) · [Terceros](../../THIRD_PARTY_NOTICES.md) · [Aviso](../../DISCLAIMER.md)
