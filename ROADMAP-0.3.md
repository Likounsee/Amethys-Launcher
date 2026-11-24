# Amethys Launcher 0.3 — périmètre et plan de réalisation

> Cette version remplace les anciens objectifs 0.3 centrés sur une intégration partenaire Steam ou sur une chaîne de distribution complète. La 0.3 doit d'abord rendre Amethys utile comme launcher local, bibliothèque unifiée et boutique de découverte alimentée par le catalogue maître.

## 1. Objectif de la version

Livrer une version Windows et Linux cohérente permettant de :

1. détecter et importer les jeux déjà installés sur le PC ;
2. gérer une bibliothèque locale fiable et lancer les jeux lorsque leur configuration le permet ;
3. explorer un véritable catalogue de jeux Amethys, avec recherche et fiches détaillées ;
4. proposer une expérience « Pour vous » fondée sur les jeux présents dans la bibliothèque ;
5. présenter une section Promotions correctement structurée, sans prix ni réductions inventés ;
6. synchroniser le catalogue public et continuer à consulter les données déjà mises en cache hors ligne.

La 0.3 doit être utile sans compte Amethys, sans serveur privé et sans téléchargement de jeux géré par Amethys.

## 2. Périmètre fonctionnel

### A. Bibliothèque locale

- Détecter les installations depuis les launchers et sources locales pris en charge.
- Afficher clairement les jeux enregistrés et les nouvelles installations détectées.
- Importer explicitement une installation dans la bibliothèque Amethys.
- Éviter les doublons par identité de plateforme et chemin normalisé.
- Associer une installation à une fiche du catalogue maître lorsque l'identité est suffisamment fiable.
- Permettre la recherche, le tri, les filtres, les fiches locales et la personnalisation des jaquettes.
- Vérifier la disponibilité du chemin et de l'exécutable, permettre de corriger la configuration et lancer le jeu.
- Préserver les fichiers et les données des launchers officiels : importer ne signifie jamais déplacer ou gérer leur installation.
- Windows et Linux sont des cibles de la 0.3. La couverture réelle de chaque source doit être documentée et testée ; aucune intégration de plateforme particulière n'est une condition de sortie.

### B. Catalogue maître Amethys

- Conserver un catalogue partagé, indépendant des bibliothèques privées des utilisateurs.
- Utiliser des identifiants Amethys stables et des identifiants externes de boutiques clairement séparés.
- Publier les métadonnées disponibles : titre, description, genres, éditeur, date de sortie, médias et liens officiels lorsque vérifiés.
- Alimenter le catalogue à partir du dépôt Amethys-Catalog et de son pipeline de génération/validation.
- Distribuer les fichiers statiques via GitHub Pages et automatiser leur publication avec GitHub Actions.
- Synchroniser le manifeste et l'index, charger les fiches détaillées à la demande et vérifier tailles/empreintes.
- Conserver un cache local utilisable hors ligne, afficher la version et la date de synchronisation et préserver le dernier catalogue valide en cas d'échec.
- Ne jamais confondre présence au catalogue, possession, installation, licence et disponibilité au téléchargement.

### C. Boutique

- Raccorder l'interface desktop au catalogue maître réel ; éliminer les données de démonstration du parcours normal.
- Fournir recherche, navigation, résultats, fiches détaillées, genres, médias et liens vers les boutiques officielles.
- Distinguer les états chargement, vide, erreur, hors ligne, cache disponible et absence de fiche.
- Indiquer clairement lorsqu'une information n'est pas disponible.
- Ne pas afficher de prix, avis, disponibilité ou promotion fictifs.
- Garder une séparation nette entre boutique/catalogue global et bibliothèque locale.

### D. « Pour vous » — recommandations

- Construire le profil à partir de tous les jeux enregistrés dans la bibliothèque locale Amethys.
- Utiliser les identifiants catalogue fiables en priorité ; utiliser les titres normalisés avec prudence lorsque l'identifiant manque.
- Récupérer les genres/métadonnées catalogue nécessaires même lorsqu'ils ne sont pas déjà présents dans le cache local.
- Agréger les préférences sans surpondérer les doublons.
- Exclure les jeux déjà présents dans la bibliothèque, y compris les variantes de titre évidentes.
- Expliquer les recommandations par les genres ou caractéristiques communes.
- Ne pas présenter une liste générique comme personnalisée lorsqu'aucun signal pertinent n'est disponible.
- Aucun temps de jeu, compte externe ou historique d'activité n'est requis.

### E. Promotions

- Conserver une section Promotions dans la boutique, avec son état vide et ses explications correctement conçus.
- Préparer le modèle interne pour recevoir ultérieurement des offres sourcées.
- N'afficher une promotion que si le prix, la devise, la source, l'URL et la date de vérification sont fiables.
- Les prix en temps réel, historiques exhaustifs, alertes et comparaisons multi-boutiques ne sont pas des critères de sortie 0.3 sans sources autorisées et suffisamment fraîches.
- Aucune dépendance à une API commerciale tierce ne doit devenir le cœur du catalogue Amethys.

### F. Préparation technique des futurs téléchargements

Le Download Manager, Install Manager, Update Manager et Repair Manager complets ne font pas partie des fonctionnalités livrées en 0.3.

Seule une préparation proportionnée est autorisée :
- conserver une distinction explicite entre lien externe, distribution gérée par la plateforme officielle et éventuelle distribution gérée par Amethys ;
- documenter les capacités d'une source (lancement, installation directe, mise à jour, réparation) sans prétendre qu'elles sont disponibles ;
- garder les identités jeu, offre, source et installation séparées ;
- définir les frontières d'adaptateurs nécessaires pour que de futurs connecteurs puissent être ajoutés sans réécrire la bibliothèque et le catalogue ;
- ne pas créer de faux jobs, fausses progressions, fausses installations ou écrans de téléchargement sans source autorisée réelle.

Les téléchargements, installations, mises à jour et réparations gérés par Amethys seront développés lorsqu'une source autorisée et un mécanisme d'accès aux fichiers seront disponibles. Les clients officiels restent responsables de leurs téléchargements protégés, licences, DRM et mises à jour.

## 3. Hors périmètre explicite de la 0.3

- Comptes Amethys, authentification, A2F et gestion de sessions.
- Backend de comptes, synchronisation cloud du profil et de la bibliothèque.
- Paiements, portefeuille et achats.
- Download Manager fonctionnel pour les jeux.
- Install Manager fonctionnel pour les jeux.
- Update Manager et Repair Manager pour les fichiers de jeux.
- Cloud saves, réseau social, amis, chat et cloud gaming.
- Intégration partenaire ou accès privé à une plateforme comme condition de sortie.
- Prix temps réel et historique exhaustif multi-boutiques sans source autorisée.
- macOS ; l'architecture ne doit pas le rendre impossible, mais Windows et Linux sont les cibles 0.3.
- Campagne globale de Quality Check et consolidation finale multi-OS : prévues pour la 0.4.

Les tests ciblés nécessaires à chaque modification restent obligatoires pendant la 0.3. Reporter la campagne globale de Quality Check ne signifie pas reporter les tests de développement.

## 4. Architecture et contraintes

- Le Core Rust reste propriétaire des données persistantes et des opérations.
- Le desktop GhostEngine et le CLI utilisent le protocole IPC versionné ; ils n'accèdent pas directement à SQLite.
- La bibliothèque locale reste privée à l'appareil ; le catalogue maître partagé ne contient aucune installation, chemin local ou donnée personnelle.
- GitHub Pages et GitHub Actions constituent l'hébergement statique initial ; aucun serveur payant permanent n'est obligatoire pour le périmètre 0.3.
- Le fournisseur de catalogue reste abstrait afin de permettre un hébergement différent ultérieurement.
- Les sources officielles et leurs politiques doivent être respectées ; aucun contournement de licence, DRM, authentification ou anti-cheat.
- Aucune installation existante n'est déplacée, écrasée ou modifiée automatiquement.
- Toute donnée incertaine reste explicitement incertaine ; ne pas fabriquer de correspondance, prix, possession ou état d'installation.
- Les migrations de base de données doivent préserver les données existantes.

## 5. Ordre de réalisation

| Ordre | Lot | Résultat attendu |
|---:|---|---|
| 0 | Audit et cadrage | Inventaire du code, des fonctionnalités déjà présentes et des blocages |
| 1 | Fondations catalogue | Identités, schémas, cache, fournisseur statique et validation |
| 2 | Pipeline du catalogue maître | Génération, contrôle qualité des données et publication statique |
| 3 | Bibliothèque locale | Détection, import, dédoublonnage, association et lancement |
| 4 | Boutique | Recherche, navigation, fiches réelles, états et liens officiels |
| 5 | Profil et recommandations | Profil issu de toute la bibliothèque, exclusions et explications |
| 6 | Promotions | Section utilisable sans données commerciales inventées |
| 7 | Préparation des futurs connecteurs | Contrats minimaux de capacités et documentation, sans gestionnaires complets |
| 8 | Intégration desktop | Parcours cohérent et retours d'erreur compréhensibles |
| 9 | Validation ciblée 0.3 | Tests pertinents, builds Windows/Linux, documentation des limites et préparation de release |

L'ordre peut être ajusté selon les dépendances réelles découvertes dans le code, mais les exclusions de périmètre restent inchangées.

## 6. Critères de sortie 0.3

La version est considérée prête lorsque :

- la bibliothèque locale peut détecter, importer, retrouver et lancer les jeux compatibles sans altérer les installations officielles ;
- le catalogue maître réel peut être synchronisé, vérifié, mis en cache et consulté hors ligne ;
- la boutique utilise ce catalogue réel pour la recherche et les fiches détaillées ;
- « Pour vous » exploite l'ensemble de la bibliothèque, exclut les jeux déjà présents et n'invente pas de personnalisation ;
- Promotions ne présente que des données commerciales vérifiées et explique proprement l'absence d'offres ;
- les parcours principaux sont vérifiés sur Windows et Linux et les limites des sources sont documentées ;
- les tests ciblés passent et aucune régression connue n'est masquée.

## 7. Règles de développement

- Développer directement sur `main`, sans Pull Request.
- Lire les fichiers concernés et leurs appels avant de modifier le code.
- Faire des changements ciblés, testables et cohérents avec l'architecture existante.
- Lancer les tests et vérifications pertinents après chaque lot ; corriger les échecs avant de poursuivre.
- Ne jamais annoncer un test réussi sans résultat vérifiable.
- Mettre à jour cette roadmap et la documentation lorsque le périmètre ou l'état réel change.
- Les pourcentages de progression doivent être calculés à partir des critères fonctionnels ci-dessus, pas du nombre de fichiers ou de commits.
