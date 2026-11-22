# Amethys Launcher — feuille de route produit

La priorité de la 0.2 est une interface de bureau complète, cohérente et soignée, reposant sur la vraie bibliothèque locale. La boutique commerciale n'est pas un objectif de la 0.2.

## Version 0.2 — interface et fondations locales

### Interface
- [x] Fenêtre de bureau GhostEngine expérimentale intégrée au dépôt.
- [x] Bibliothèque réelle connectée à Amethys Core par IPC.
- [x] Recherche, filtres, tri, fiches de jeux, import et lancement sécurisé.
- [x] Détection en arrière-plan avec progression et annulation.
- [ ] Finaliser la direction visuelle, la typographie, les espacements et la cohérence des composants.
- [ ] Refaire les cartes de jeux autour de proportions d'illustrations fixes et de placeholders originaux.
- [ ] Compléter les états vides, chargements, erreurs, confirmations et notifications.
- [ ] Rendre la mise en page adaptative aux dimensions réelles de la fenêtre.
- [ ] Terminer la navigation clavier et le focus accessible des champs et dialogues.
- [ ] Concevoir des paramètres cohérents, en distinguant les réglages réellement enregistrés des fonctions à venir.

### Bibliothèque et métadonnées
- [ ] Introduire un Game ID Amethys stable, indépendant des identifiants Steam/Epic/GOG/Xbox.
- [ ] Définir les métadonnées : nom, genres, développeur, éditeur, date de sortie, description et identifiants externes.
- [ ] Prévoir les références de médias icon, logo, cover et banner sans lier ces ressources à un fournisseur.
- [ ] Stocker localement les médias des jeux de la bibliothèque et les préférences propres à l'utilisateur.
- [ ] Utiliser des illustrations originales de remplacement pendant le développement ; aucun visuel de jeu non autorisé ne doit être distribué.
- [ ] Améliorer le rapport d'import et les erreurs par installation.

### Qualité
- [ ] Éliminer les hypothèses de résolution fixe et vérifier les tailles de bibliothèque importantes.
- [ ] Stabiliser les interactions et les états d'erreur.
- [ ] Tests unitaires/intégration, packaging portable reproductible et documentation.
- [ ] Conserver Core comme propriétaire des données persistantes ; l'interface ne doit pas accéder directement à SQLite.

**Hors périmètre 0.2 :** catalogue commercial connecté, prix, promotions, achats, licences, téléchargement et installation depuis une boutique. Les écrans futurs peuvent être présentés comme indisponibles, jamais avec des données commerciales fictives.

## Version 0.3 — boutique et services en ligne

- Intégrer des fournisseurs réels au moyen d'adaptateurs documentés et autorisés.
- Charger le catalogue, les fiches, les médias, les prix et les offres depuis des sources fiables, avec date et provenance.
- Ouvrir les pages d'achat officielles ou utiliser les API officielles lorsque les opérations sont autorisées.
- Vérifier les jeux possédés uniquement via des mécanismes autorisés.
- Ajouter les flux d'installation/téléchargement lorsque les plateformes le permettent : progression, reprise, vérification, erreurs et annulation.
- Mettre en cache les médias du catalogue avec limites de taille, expiration, déduplication et nettoyage.
- Charger d'abord les éléments visibles de l'accueil, puis précharger progressivement les fiches en arrière-plan.
- Ne pas exiger un serveur privé payant pour commencer : évaluer les services gratuits, l'hébergement statique et le fonctionnement local. Ne jamais placer de secrets fournisseurs dans le client.
- Définir les données personnelles par utilisateur et les synchroniser uniquement si un backend et une politique de confidentialité adaptés existent.

## Version 1.0 — stabilisation et préparation publique

- Stabiliser l'architecture, la sécurité, les mises à jour et la récupération après erreur.
- Valider les flux fournisseurs et les limites d'utilisation de leurs API.
- Finaliser les performances, l'accessibilité, la documentation et les packages.
- Remplacer les placeholders par des médias originaux, fournis par les utilisateurs ou autorisés par leurs ayants droit.
- Obtenir les autorisations nécessaires pour les marques, contenus et intégrations avant toute diffusion publique.
- Préparer les éléments de présentation et les échanges partenaires lorsque le produit est suffisamment complet.

## Architecture média cible

- **Bibliothèque locale :** fichiers d'illustration stockés dans le répertoire de données Amethys, indépendamment du dossier d'installation du jeu.
- **Boutique distante :** médias récupérés depuis les fournisseurs intégrés, mis en cache localement et réutilisés entre les sessions.
- **Références :** les métadonnées ne contiennent que des références d'assets ; le Game ID ne change pas lorsqu'une jaquette est remplacée.
- **Résilience :** placeholder original si un fichier est absent, corrompu ou hors ligne.
- **Préchargement :** priorité aux visuels visibles, puis chargement progressif ; quotas, expiration et nettoyage du cache.
- **Serveur :** aucune infrastructure distante obligatoire pour la bibliothèque locale. GitHub peut héberger des ressources statiques publiques, mais ne remplace pas un backend privé pour les comptes, achats ou données personnelles.

## État actuel et limites à ne pas masquer

- Le catalogue commercial n'est pas connecté.
- Les temps de jeu et l'historique des sessions ne sont pas fournis par Core.
- Le lancement est encore effectué côté client de bureau.
- Le backend de fenêtre/rendu GhostEngine est expérimental et temporaire.
- Le prototype d'interface utilise encore une mise en page conçue autour de 1600×900.
