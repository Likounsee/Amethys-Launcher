# Amethys Launcher 0.2 — feuille de route

Cette feuille de route décrit les critères de sortie de la version 0.2. Elle distingue les fonctions réellement disponibles des travaux à réaliser. Aucune donnée de catalogue, aucun prix, aucune promotion, aucun temps de jeu et aucun compte connecté ne doivent être simulés.

## Déjà opérationnel

- Application de bureau Rust avec le prototype de fenêtre GhostEngine intégré dans ce dépôt.
- Communication avec Amethys Core par IPC ; le launcher ne lit pas directement la base SQLite.
- Démarrage de l'agent local lorsque celui-ci est livré à côté du launcher.
- Lecture, actualisation, recherche et filtrage de la bibliothèque réelle.
- Détection locale en tâche de fond et import individuel des installations trouvées.
- Fiche d'un jeu avec métadonnées réellement enregistrées, vérification du dossier et configuration de l'exécutable.
- Lancement après canonicalisation du chemin et vérification qu'il reste dans le dossier d'installation.
- Retrait d'une entrée de bibliothèque avec confirmation explicite ; les fichiers du jeu restent intacts.
- Snapshot GhostEngine épinglé dans `vendor/ghostengine`, donc un seul dépôt Git est nécessaire.

## Travaux nécessaires avant 0.2

### P0 — Interface de bureau aboutie
- [x] Cartes de bibliothèque avec couverture, plateforme, statut réel, chemin et actions.
- [ ] Finaliser la hiérarchie visuelle et les états de chargement.
- [x] Ajouter le parcours clavier Tab / Maj+Tab, l'activation Entrée et un indicateur de focus visible.
- [ ] Adapter la grille et les panneaux à la taille réelle de la fenêtre ; supprimer les hypothèses rigides 1600×900.
- Rendre les interactions évidentes : focus clavier, retour visuel, raccourcis, navigation et états de chargement.
- Conserver les données réelles et afficher clairement les fonctionnalités non connectées.

### P0 — Bibliothèque fiable
- [x] Tri réel par nom, plateforme et état de lancement, avec filtres de plateforme et recherche multicritère.
- [x] Navigation clavier de base (Tab / Maj+Tab / Entrée) et focus visuel des actions visibles.
- [ ] Compléter les raccourcis de page et le focus accessible des champs.
- [x] Afficher la cause réelle d'indisponibilité du lancement (exécutable absent, chemin invalide, dossier inaccessible ou tentative de sortie du dossier du jeu) sans bloquer l'accès aux détails.
- [x] Confirmation avant retrait et explication que les fichiers ne sont pas supprimés.
- [x] Dédupliquer les résultats de détection contre la bibliothèque existante et entre eux (identité plateforme + chemin canonique).
- [ ] Ajouter une progression détaillée et une annulation réelle du scan (nécessite une API de découverte coopérative).
- [ ] Rapport détaillé des imports réussis/échoués et gestion des erreurs par entrée.
- Ne pas afficher de temps de jeu ni de dernière session tant que Core ne les fournit pas.

### P1 — Cycle de vie des jeux
- Déplacer le lancement dans Core ou un service dédié, plutôt que de lancer directement depuis l'interface.
- Suivre les processus lancés, détecter leur fermeture et enregistrer les sessions.
- Ajouter arguments et options de lancement avec validation et stockage persistant.
- Ajouter déplacement/réparation des chemins d'installation avec confirmation et vérification.

### P1 — Paramètres persistants
- Préférences de l'interface et du comportement du launcher.
- Gestion des dossiers de bibliothèque et des emplacements de détection.
- Diagnostics, journaux accessibles et informations de version.

### P2 — Catalogue et installation
- Définir des adaptateurs fournisseurs réels et documentés (authentification, recherche, fiches, liens).
- Afficher les prix et offres uniquement lorsqu'ils viennent d'une source fiable et datée.
- Téléchargement, reprise, vérification d'intégrité et désinstallation uniquement lorsqu'un fournisseur autorise réellement ces opérations.
- Tant qu'aucun fournisseur n'est intégré, conserver une boutique honnête indiquant son indisponibilité.

### P0 — Stabilisation et livraison
- Tests unitaires et d'intégration sur Windows x64.
- Vérifier le démarrage autonome, les chemins d'assets, l'agent et les permissions IPC.
- Packaging portable reproductible, notes de version et procédure de mise à jour.
- Mesurer le rendu sur bibliothèque volumineuse et éviter les scans filesystem dans le chemin de rendu.

## Ordre d'exécution

1. Interface et bibliothèque (sans inventer de données).
2. Sécurisation des actions et qualité de la détection.
3. Cycle de vie des processus et sessions via Core.
4. Paramètres persistants et diagnostics.
5. Fournisseurs réels, catalogue et installation si les API le permettent.
6. Validation Windows, package portable et documentation de sortie.

## Limites connues à ne pas masquer

- Le catalogue de boutique n'est pas connecté.
- Le temps de jeu et l'historique des sessions ne sont pas disponibles.
- Le lancement est encore effectué côté client de bureau.
- La confirmation de retrait et le tri sont présents ; la navigation clavier complète reste à faire.
- Le backend de fenêtre/rendu GhostEngine est expérimental et temporaire.
- La mise en page du launcher reste actuellement conçue autour d'une fenêtre 1600×900.
