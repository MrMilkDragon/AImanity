# AImanity

[English](../../README.md) · [Toutes les langues](../../README.md) · Apache-2.0

## Téléchargements et état

[macOS 0.0.5 Europium (DMG)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-macos-arm64.dmg) · [Windows 0.0.5 Europium (EXE)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-windows-x64-setup.exe) · [Linux (DEB)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-linux-amd64.deb) · [Linux (RPM)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-linux-x86_64.rpm)

Configuration requise : macOS 15.0 ou ultérieur sur Apple Silicon, ou Windows 11 x64. Chaque installateur contient Android, Linux, l’émulateur et les services locaux.

Sur Linux x86_64, KVM et glibc 2.36 ou ultérieure sont requis. Choisissez le DEB pour Debian/Ubuntu ou le RPM pour Fedora/Enterprise Linux 10. Pour conserver vos appareils et conversations lors d’une mise à jour, installez la nouvelle version au même format. [Commandes d’installation](release-status.md#linux)

## Fonctions et scénarios

AImanity réunit un téléphone Android et un bureau Linux autour d’une conversation et d’un seul agent actif. Enregistrez vos profils de modèles et choisissez le profil actif dans la configuration.

- Utiliser des applications telles que WeChat à partir de l’écran et des notifications réels ; répondre selon les instructions de l’utilisateur. Les restrictions du compte et de l’application restent applicables.
- Recevoir un fichier sur le téléphone, le transférer à Linux, traiter et vérifier un tableau, puis retourner le résultat. Transfert entre appareils et remise à un contact sont deux résultats distincts.
- Ajouter des instructions et interrompre la génération sans perdre messages ni actions. Les notifications peuvent réveiller l’Agent ; Liberty fonctionne sur les deux appareils.
- Partager Skills, documents de connaissance liés et fichiers de travail. Modèles/API, mémoire et connexions externes appartiennent aux Agents. Les appareils se connectent directement par défaut. Vous pouvez configurer un proxy dans l’espace de travail.

Les actions sont liées à une observation, un appareil, une tâche, une époque de contrôle et un identifiant unique. Un résultat inconnu est observé avant toute répétition.

## Architecture et données

`apps/native` : Rust (Iced/wgpu). `crates` : Rust/Axum/Tokio. `packages/protocol` : protobuf. Les données sont stockées dans PostgreSQL via SQLx. Le projet se compile avec Cargo.

Code et données utilisateur sont séparés. Disques, comptes, conversations, pièces jointes et clés ne sont pas publiés. Les mises à jour doivent employer des artefacts versionnés, sauvegardes, migrations et retours arrière, sans `git pull` directement sur les données utilisateur.

## Suite, responsabilité et licence

La feuille de route prévoit davantage de services vocaux, de compétences pour les applications et de tâches bureautiques déclenchées par des notifications.

Interface et première configuration couvrent dix langues. Le sélecteur à gauche de Skills change l’interface ; il ne traduit pas automatiquement les systèmes invités ni les sorties brutes. L’arabe utilise le sens droite à gauche.

L’utilisateur reste responsable des autorisations, comptes, actions externes et conditions des applications. Ne pas employer le système pour tromper, frauder ou agir sans consentement. Code original sous Apache-2.0 ; dépendances sous leurs propres licences.

[État de publication](release-status.md) · [Architecture](architecture.en.md) · [Compatibilité](compatibility.md) · [Tiers](../../THIRD_PARTY_NOTICES.md) · [Avertissement](../../DISCLAIMER.md)
