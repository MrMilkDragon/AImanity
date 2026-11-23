# AImanity 0.0.5 Europium

[English](../../README.md) · [Alle Sprachen](../../README.md) · Apache-2.0

## Downloads und Stand

[macOS 0.0.5 Europium (DMG)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-macos-arm64.dmg) · [Windows 0.0.5 Europium (EXE)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-windows-x64-setup.exe) · [Linux (DEB)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-linux-amd64.deb) · [Linux (RPM)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-linux-x86_64.rpm)

Voraussetzungen sind macOS 15.0 oder neuer mit Apple Silicon beziehungsweise Windows 11 x64. Jedes Installationspaket enthält Android, Linux, den Emulator und die lokalen Dienste.

Linux auf x86_64 benötigt KVM und glibc ab 2.36. Wähle DEB für Debian/Ubuntu oder RPM für Fedora/Enterprise Linux 10. Installiere das nächste Paket im selben Format, um Geräte und Chats zu behalten. [Installationsbefehle](release-status.md#linux)

## Funktionen und Szenarien

Ein echter Android-Gast und ein schlanker Linux-Desktop nutzen denselben aktiven Agenten und dieselbe Unterhaltung. Weitere Modellprofile lassen sich in der Konfiguration speichern und auswählen.

- Apps wie WeChat anhand realer Bildschirme und Benachrichtigungen bedienen und im Auftrag des Nutzers antworten. Konto- und App-Beschränkungen bleiben wirksam.
- Eine Datei am Telefon empfangen, an Linux übertragen, dort eine Tabelle bearbeiten und prüfen und das Ergebnis zurückgeben. Geräteübertragung und Zustellung an einen Kontakt werden getrennt geprüft.
- Laufende Aufgaben durch weitere Nachrichten lenken und unterbrechen, ohne frühere Nachrichten oder Aktionen zu verlieren. Benachrichtigungen können den Agenten wecken; Liberty läuft geräteübergreifend.
- Skills, verknüpfte Wissensdokumente und Arbeitsdateien gemeinsam nutzen. Modelle/APIs, Gedächtnis und externe Verbindungen gehören zum Agenten. Die Geräte verbinden sich standardmäßig direkt. Einen Proxy kannst du im Arbeitsbereich einrichten.

Aktionen sind an Beobachtung, Gerät, Aufgabe, Kontroll-Epoche und eindeutige Aktions-ID gebunden. Unbekannte Ergebnisse werden vor Wiederholung beobachtet.

## Technik und Daten

`apps/native`: Rust (Iced/wgpu). `crates`: Rust/Axum/Tokio. `packages/protocol`: protobuf. Die Daten liegen in PostgreSQL, der Zugriff erfolgt über SQLx. Das Projekt wird mit Cargo gebaut.

Code und Nutzerdaten sind getrennt. Geräteplatten, Konten, Chats, Anhänge und Schlüssel werden nicht veröffentlicht. Updates benötigen versionierte Pakete, Sicherung, Migration und Rücknahme statt eines direkten `git pull` in Nutzerdaten.

## Ausblick, Verantwortung und Lizenz

Geplant sind weitere Sprachanbieter, Skills für Apps und Büroabläufe, die durch Benachrichtigungen starten.

Oberfläche und Ersteinrichtung unterstützen zehn Sprachen. Der Sprachschalter links von Skills ändert die Oberfläche; Gastbetriebssysteme und rohe Werkzeugausgaben werden nicht automatisch übersetzt. Arabisch verwendet RTL.

Nutzer verantworten Berechtigungen, Konten, externe Aktionen und App-Bedingungen. Kein Einsatz für Täuschung, Betrug oder Handlungen ohne Einwilligung. Originalcode: Apache-2.0; Drittkomponenten behalten ihre Lizenzen.

[Freigabestand](release-status.md) · [Architektur](architecture.en.md) · [Kompatibilität](compatibility.md) · [Drittanbieter](../../THIRD_PARTY_NOTICES.md) · [Haftungshinweise](../../DISCLAIMER.md)
