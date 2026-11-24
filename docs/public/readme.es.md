# AImanity 0.0.6 Fluorine

[English](../../README.md) · [Todos los idiomas](../../README.md) · Apache-2.0

## Descargas y estado

[Descargar 0.0.6 Fluorine](https://github.com/MrMilkDragon/AImanity/releases/tag/v0.0.6) · [Pruebas de instalación y hardware verificado](release-status.md)

Instala 0.0.6 como una instalación nueva. El espacio de trabajo se abre sin dispositivos. Puedes crear invitados Android o Linux, vincular un dispositivo remoto o autorizar el uso del escritorio local para una tarea concreta.

## Funciones y escenarios

El agente elige un dispositivo autorizado según la tarea. Todos comparten la conversación, la lista de tareas y los archivos de trabajo. Los subagentes pueden ejecutar tareas independientes en paralelo.

Copilot permite corregir instrucciones por voz. Liberty explora libremente. Task & Goal trabaja hacia el objetivo indicado.

## Arquitectura y datos

La interfaz nativa usa Rust, Iced y wgpu. Los mensajes usan protobuf. La base de datos integrada redb guarda los dispositivos, las conversaciones y las tareas en el equipo.

## Futuro, responsabilidad y licencia

La hoja de ruta incluye más proveedores de voz, habilidades para aplicaciones y tareas de oficina que se inician al recibir una notificación.

Interfaz y configuración inicial admiten diez idiomas. El selector a la izquierda de Skills cambia la interfaz; no traduce automáticamente el sistema invitado ni las salidas originales. El árabe usa escritura de derecha a izquierda.

El usuario responde de permisos, cuentas, acciones externas y condiciones de las aplicaciones. No usar para engaño, fraude ni acciones sin consentimiento. Código original Apache-2.0; los terceros conservan sus licencias.

[Estado](release-status.md) · [Arquitectura](architecture.en.md) · [Compatibilidad](compatibility.md) · [Terceros](../../THIRD_PARTY_NOTICES.md) · [Aviso](../../DISCLAIMER.md)
