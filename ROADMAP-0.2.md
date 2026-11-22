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
- [x] Conserver un UUID local stable pour chaque installation/import de jeu, indépendant de l'identifiant du fournisseur.
- [x] Poser une première couche SQLite pour les métadonnées (genres, développeur, éditeur, date de sortie, description), les identifiants externes multiples et les références icon/logo/cover/banner.
- [ ] Créer le Game ID catalogue canonique, distinct de l'UUID d'installation : plusieurs installations Steam/Epic/GOG/Xbox du même titre doivent pouvoir partager un seul Game ID Amethys.
- [ ] Rattacher les métadonnées et identifiants externes à ce Game ID catalogue, puis relier chaque installation locale au titre correspondant sans fusion automatique hasardeuse.
- [ ] Migrer proprement la première couche de métadonnées vers le modèle catalogue canonique, sans perdre les données des bibliothèques existantes.
- [ ] Stocker localement les médias des jeux de la bibliothèque et les préférences propres à l'utilisateur.
- [ ] Utiliser des illustrations originales de remplacement pendant le développement ; aucun visuel de jeu non autorisé ne doit être distribué.
- [ ] Améliorer le rapport d'import et les erreurs par installation.

### Qualité
- [ ] Éliminer les hypothèses de résolution fixe et vérifier les tailles de bibliothèque importantes.
- [ ] Stabiliser les interactions et les états d'erreur.
- [ ] Tests unitaires/intégration, packaging portable reproductible et documentation.
- [ ] Conserver Core comme propriétaire des données persistantes ; l'interface ne doit pas accéder directement à SQLite.

**Hors périmètre 0.2 :** catalogue commercial connecté, prix, promotions, achats, licences, téléchargement et installation depuis une boutique. Les écrans futurs peuvent être présentés comme indisponibles, jamais avec des données commerciales fictives.

## Architecture future — boutique personnalisée sans serveur obligatoire

La boutique personnalisée est prévue pour la version 0.3 et ne doit pas imposer un serveur payant à l'utilisateur ni au mainteneur pour les fonctions de base.

- Catalogue commun : publier des fichiers de catalogue versionnés et des métadonnées statiques sur GitHub (Pages ou Releases), sous réserve des quotas et conditions du service.
- Personnalisation : calculer localement les recommandations et le classement à partir du catalogue public, des jeux installés, de la bibliothèque, de la liste de souhaits et des préférences de l'utilisateur.
- Vie privée : conserver localement les préférences et signaux personnels ; ne pas les envoyer à un serveur pour construire la page d'accueil.
- Médias : mettre en cache localement les ressources publiques du catalogue, avec chargement progressif des fiches.
- Fonctionnement hors ligne : garder accessibles la bibliothèque, les métadonnées déjà mises en cache et les préférences sans connexion.
- Services distants éventuels : comptes Amethys, synchronisation multi-appareils et fonctions communautaires restent optionnels et ne seront étudiés que si un hébergement gratuit et durable ou un financement est trouvé.
- Limites : GitHub peut distribuer des fichiers publics statiques, mais ne remplace pas une base de données privée, un système de comptes sécurisé ni les API officielles des boutiques.
- Achats, licences et installation via les plateformes officielles : hors périmètre de la 0.2, à étudier pour la 0.3 avec les API et mécanismes autorisés par chaque fournisseur.

Aucun abonnement serveur ne doit être une dépendance pour utiliser la bibliothèque locale d'Amethys.

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
