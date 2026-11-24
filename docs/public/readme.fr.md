# AImanity 0.0.6 Fluorine

[English](../../README.md) · [Toutes les langues](../../README.md) · Apache-2.0

## Téléchargements et état

[Télécharger 0.0.6 Fluorine](https://github.com/MrMilkDragon/AImanity/releases/tag/v0.0.6) · [Installations vérifiées et matériel testé](release-status.md)

Installez la version 0.0.6 dans un nouvel espace de travail. Il s’ouvre sans appareil. Créez des invités Android ou Linux à la demande, associez une machine distante ou autorisez une tâche à utiliser le bureau de l’hôte.

## Fonctions et scénarios

L’agent choisit un appareil autorisé selon la tâche. Les appareils partagent la conversation, la liste des tâches et les fichiers de travail. Des sous-agents peuvent traiter des tâches indépendantes en parallèle.

Copilot permet de corriger une consigne à la voix. Liberty explore librement. Task & Goal poursuit l’objectif que vous lui confiez.

## Architecture et données

L’interface native utilise Rust, Iced et wgpu. Les messages utilisent protobuf. La base embarquée redb conserve les appareils, conversations et tâches sur la machine.

## Suite, responsabilité et licence

La feuille de route prévoit davantage de services vocaux, de compétences pour les applications et de tâches bureautiques déclenchées par des notifications.

Interface et première configuration couvrent dix langues. Le sélecteur à gauche de Skills change l’interface ; il ne traduit pas automatiquement les systèmes invités ni les sorties brutes. L’arabe utilise le sens droite à gauche.

L’utilisateur reste responsable des autorisations, comptes, actions externes et conditions des applications. Ne pas employer le système pour tromper, frauder ou agir sans consentement. Code original sous Apache-2.0 ; dépendances sous leurs propres licences.

[État de publication](release-status.md) · [Architecture](architecture.en.md) · [Compatibilité](compatibility.md) · [Tiers](../../THIRD_PARTY_NOTICES.md) · [Avertissement](../../DISCLAIMER.md)
