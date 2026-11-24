# AImanity 0.0.6 Fluorine

[English](../../README.md) · [Alle Sprachen](../../README.md) · Apache-2.0

## Downloads und Stand

[0.0.6 Fluorine herunterladen](https://github.com/MrMilkDragon/AImanity/releases/tag/v0.0.6) · [Installationsprüfung und getestete Hardware](release-status.md)

Installiere 0.0.6 neu. Der Arbeitsbereich startet ohne Geräte. Erstelle bei Bedarf Android- oder Linux-Gäste, kopple ein entferntes Gerät oder erteile einem einzelnen Auftrag Zugriff auf den Host-Desktop.

## Funktionen und Szenarien

Der Agent wählt für jeden Auftrag ein freigegebenes Gerät. Gespräch, Aufgabenliste und Arbeitsdateien stehen auf allen Geräten zur Verfügung. Unabhängige Aufgaben können parallel an Unteragenten gehen.

Mit Copilot kannst du Anweisungen per Sprache korrigieren. Liberty erkundet selbstständig. Task & Goal arbeitet auf ein vorgegebenes Ziel hin.

## Technik und Daten

Die native Oberfläche nutzt Rust, Iced und wgpu. Die Nachrichten verwenden protobuf. Die eingebettete Datenbank redb speichert Geräte, Gespräche und Aufgaben lokal.

## Ausblick, Verantwortung und Lizenz

Geplant sind weitere Sprachanbieter, Skills für Apps und Büroabläufe, die durch Benachrichtigungen starten.

Oberfläche und Ersteinrichtung unterstützen zehn Sprachen. Der Sprachschalter links von Skills ändert die Oberfläche; Gastbetriebssysteme und rohe Werkzeugausgaben werden nicht automatisch übersetzt. Arabisch verwendet RTL.

Nutzer verantworten Berechtigungen, Konten, externe Aktionen und App-Bedingungen. Kein Einsatz für Täuschung, Betrug oder Handlungen ohne Einwilligung. Originalcode: Apache-2.0; Drittkomponenten behalten ihre Lizenzen.

[Freigabestand](release-status.md) · [Architektur](architecture.en.md) · [Kompatibilität](compatibility.md) · [Drittanbieter](../../THIRD_PARTY_NOTICES.md) · [Haftungshinweise](../../DISCLAIMER.md)
