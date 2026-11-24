# Amethys Launcher 0.3 — Périmètre et roadmap révisés

**Version précédente :** 0.2.0 gelée.  
**Branche de développement :** `main`.  
**Cibles de développement :** Windows et Linux dès la 0.3 ; macOS reste différé.  
**Quality Check globale et validation finale multi-OS :** 0.4.

## 1. Objectif produit de la 0.3

Faire d'Amethys un launcher PC réellement utile autour de trois piliers :

1. **Bibliothèque unifiée :** détecter, importer, identifier et lancer les jeux déjà installés, sans remplacer les clients officiels.
2. **Découverte personnalisée :** comprendre les goûts du joueur à partir des jeux présents dans sa bibliothèque Amethys et lui proposer des jeux similaires.
3. **Catalogue fiable :** disposer d'un catalogue maître commun, structuré, versionné et actualisable gratuitement via des fichiers statiques publiés sur GitHub Pages.

La priorité plateforme est **Steam**, sur Windows et Linux. La 0.3 doit préparer une intégration Steam propre et démontrable, tout en respectant les interfaces, autorisations et limites publiques de Valve. Les intégrations nécessitant des accès partenaires ou des accords ne doivent pas être simulées : elles seront activées uniquement lorsqu'elles seront autorisées et disponibles.

La 0.3 doit être un produit présentable à des partenaires. Elle ne doit pas essayer de construire prématurément une boutique commerciale complète ou de remplacer l'infrastructure de distribution de Steam.

## 2. Décisions produit figées

### Recommandations 0.3

- Le profil initial est calculé exclusivement à partir des jeux réellement présents dans la bibliothèque Amethys.
- Chaque jeu doit être associé à une identité catalogue fiable, en privilégiant les identifiants externes connus (notamment Steam AppID), puis une correspondance de titre contrôlée.
- Les métadonnées de catalogue fournissent les genres, catégories et autres caractéristiques disponibles.
- Le moteur agrège les préférences à partir de l'ensemble des jeux possédés ; il exclut les jeux déjà présents des suggestions.
- Une recommandation doit pouvoir expliquer les catégories ou caractéristiques qui la motivent.
- Une donnée manquante ne doit pas être inventée : le jeu reste dans la bibliothèque et son profil peut être enrichi ultérieurement.
- Aucun temps de jeu, temps en ligne ou comportement Steam n'est requis pour la 0.3.

### Intégration Steam

- Windows et Linux sont des cibles de première classe.
- Détecter les bibliothèques Steam locales et associer les installations aux identités catalogue.
- Préserver les installations et fichiers gérés par Steam.
- Lancer un jeu via Steam ou son mécanisme officiel lorsque nécessaire ; ne pas contourner licence, authentification, DRM ou anti-cheat.
- Préparer une frontière d'adaptateur Steam pour les fonctions supplémentaires.
- Les statistiques de compte, heures jouées et activité ne seront ajoutées qu'avec une intégration et des autorisations adaptées.

### Catalogue et données commerciales

- Le catalogue maître Amethys reste indépendant des données privées de chaque PC.
- GitHub Pages héberge les artefacts statiques ; GitHub Actions peut les construire et les publier périodiquement, sans serveur permanent obligatoire.
- Le launcher conserve un cache local et affiche la date/version des données.
- Les prix et promotions ne sont affichés que si leur source, leur devise et leur date de vérification sont connues.
- Les prix temps réel, l'historique exhaustif et la comparaison multi-boutiques ne sont pas promis sans source autorisée, fiable et suffisamment fraîche.
- Steam est la première cible de données commerciales, mais SteamDB reste une source indépendante et ne constitue pas une API Valve garantie.

## 3. Ordre d'exécution révisé

| Ordre | Lot | Résultat attendu | Statut |
|---:|---|---|---|
| 0 | Audit de l'existant | Inventaire, réutilisation, manques et risques | Terminé |
| 1 | Architecture et fondations | Core, IPC, stockage, sécurité et frontières | Conception terminée |
| 2 | Catalogue maître et identités | IDs stables, catalogue versionné, cache et adaptateur statique | Implémenté ; CI ciblée annoncée verte dans le suivi précédent |
| 3 | Profil joueur et recommandations | Suggestions basées sur tous les jeux présents dans la bibliothèque | À réaliser / compléter |
| 4 | Enrichissement et correspondance catalogue | Associer les jeux locaux aux AppIDs et métadonnées, gérer les correspondances incertaines | À réaliser |
| 5 | Intégration bibliothèque Steam Windows/Linux | Détection, import, association, état et lancement via Steam officiel | À réaliser / auditer l'existant |
| 6 | Store catalogue et découverte | Recherche, catégories, fiches, recommandations « Pour vous » et états réels | À réaliser / compléter |
| 7 | Préparation intégration partenaire Steam | Adaptateur isolé, capacités explicites, documentation et démonstrateur sans API privée simulée | À réaliser |
| 8 | Pipeline catalogue statique | Génération, validation, publication GitHub Pages, cache et rafraîchissement | À réaliser / compléter |
| 9 | Intégration desktop et finalisation 0.3 | Parcours cohérent, documentation, limites et dossier de présentation | À réaliser |

L'état réel de chaque lot doit être confirmé par lecture du code et tests ciblés avant de le déclarer terminé. Les numéros des lots historiques 0 à 2 sont conservés pour la traçabilité ; les lots fonctionnels suivants sont réordonnés selon la nouvelle priorité produit.

## 4. Critères d'acceptation des lots prioritaires

### Lot 3 — Profil joueur et recommandations

- Le profil est calculé à partir de tous les jeux présents dans la bibliothèque Amethys, pas uniquement ceux qui ont déjà des métadonnées locales en cache.
- Les identités sont résolues via le catalogue maître et ses identifiants externes avant toute correspondance approximative.
- Les catégories/genres sont agrégés sans compter plusieurs fois les mêmes caractéristiques d'un jeu.
- Les jeux possédés sont exclus des recommandations, y compris lorsque les titres diffèrent uniquement par ponctuation, édition ou normalisation.
- Le classement est stable et explicable ; les suggestions sans signal suffisant ne sont pas présentées comme personnalisées.
- Des tests couvrent bibliothèque vide, jeu sans métadonnées, correspondance ambiguë, doublons, catégories communes et exclusion des jeux possédés.

### Lot 4 — Enrichissement catalogue

- Correspondance robuste entre installation locale, identifiant externe et `amethys_id`.
- Priorité aux IDs vérifiés ; les correspondances par titre doivent être normalisées et prudentes.
- Une correspondance incertaine est signalée et ne fusionne pas silencieusement deux jeux.
- Les métadonnées sont versionnées, sourcées et mises en cache.
- Le catalogue partagé ne contient aucune donnée privée de bibliothèque.

### Lot 5 — Steam Windows/Linux

- Détecter plusieurs bibliothèques Steam et leurs jeux installés.
- Ne pas confondre jeu possédé, jeu installé et jeu simplement présent au catalogue.
- Préserver les installations Steam et ne jamais modifier leurs fichiers pour les importer.
- Lancer par Steam lorsque le jeu ou la plateforme l'exige.
- Gérer les chemins et spécificités Windows/Linux sans supposer que les chemins Windows existent sous Linux.
- Tester les cas de bibliothèque absente, plusieurs bibliothèques, manifeste invalide, jeu désinstallé et AppID inconnu.

### Lot 6 — Store et « Pour vous »

- Alimenter l'écran depuis le catalogue maître réel, pas des données de démonstration.
- Proposer « Pour vous » à partir du profil de bibliothèque.
- Présenter les catégories, fiches et raisons de recommandation.
- Distinguer clairement jeu installé, jeu possédé si cette information est disponible, et jeu simplement catalogué.
- Aucun prix ou promotion fictif.

### Lot 7 — Préparation partenaire Steam

- Documenter les fonctions déjà opérationnelles et les fonctions qui nécessitent un accès partenaire.
- Démontrer la détection, l'identification, le lancement officiel et la découverte personnalisée.
- Séparer l'adaptateur Steam du cœur du catalogue et du profil joueur.
- Ne pas collecter ni afficher des statistiques de compte sans autorisation et mécanisme appropriés.
- Préparer une demande de collaboration précise, sans présenter des fonctions futures comme déjà acquises.

### Lot 8 — Publication gratuite du catalogue

- Générer des fichiers statiques versionnés et vérifiables.
- Publier les artefacts via GitHub Pages et automatiser leur mise à jour avec GitHub Actions dans les limites des quotas.
- Permettre le téléchargement différentiel ou à la demande des détails lorsque c'est possible.
- Conserver le dernier catalogue valide en cas d'échec de synchronisation.
- Afficher la version et la date de dernière actualisation.
- Ne jamais exposer de secrets, données personnelles ou informations de bibliothèque dans les fichiers publics.

## 5. Explicitement reporté après la 0.3

Ces fonctions ne doivent pas retarder le produit présentable centré sur la bibliothèque, les recommandations et Steam :

- Téléchargement et installation directs gérés par Amethys (Download Manager et Install Manager).
- Mise à jour et réparation des fichiers de jeux gérés par Amethys.
- Remplacement des clients officiels ou gestion universelle de tous les launchers.
- Prix et promotions en temps réel à grande échelle, historique complet et alertes multi-boutiques tant que les sources autorisées et fiables ne sont pas établies.
- Statistiques de temps de jeu, temps en ligne et activité Steam : après intégration autorisée.
- Comptes Amethys complets, synchronisation multi-appareils, paiement, portefeuille et achats.
- Cloud Gaming, cloud saves, réseau social, amis, chat et Workshop intégré.
- Support macOS ; l'architecture doit éviter de le bloquer, mais Windows et Linux sont les cibles 0.3.
- Campagne globale Quality Check, validation finale multi-OS et release hardening : version 0.4.

Les anciens lots Download Manager, Install Manager, Update & Repair ne sont pas supprimés de la vision à long terme ; ils sont explicitement déplacés hors du chemin critique 0.3.

## 6. Hors périmètre et règles non négociables

- Aucun contournement de DRM, licence, authentification, anti-cheat ou restriction régionale.
- Aucun téléchargement ou accès à des données commerciales sans source et autorisation appropriées.
- Aucun serveur payant obligatoire pour la bibliothèque locale, les recommandations ou la lecture du catalogue public statique.
- Aucun prix, temps de jeu, possession ou statut d'installation inventé.
- Le Core reste propriétaire des données persistantes ; desktop et CLI passent par IPC.
- Les installations officielles existantes ne sont jamais déplacées, écrasées ou modifiées automatiquement.
- Les migrations SQLite sont non destructives.
- Développement directement sur `main`, sans Pull Request.
- Lire les fichiers concernés et leurs appels avant chaque modification.
- Tests ciblés nécessaires à chaque lot ; ne jamais prétendre qu'un test a été exécuté sans preuve.
- Toute modification de périmètre doit être répercutée dans cette roadmap.

## 7. Critère global de fin fonctionnelle 0.3

Un utilisateur Windows ou Linux doit pouvoir :

1. retrouver/importer ses jeux installés dans une bibliothèque Amethys fiable ;
2. lancer les jeux compatibles, en respectant le launcher officiel lorsque nécessaire ;
3. voir ses jeux associés au catalogue maître Amethys avec des catégories et métadonnées vérifiables ;
4. obtenir une page « Pour vous » proposant des jeux similaires à partir de l'ensemble de sa bibliothèque, avec explication des recommandations et exclusion de ses jeux déjà présents ;
5. consulter un catalogue réel publié et actualisable via l'infrastructure statique gratuite ;
6. comprendre précisément quelles fonctions Steam sont disponibles immédiatement et lesquelles nécessitent un accord ou un accès partenaire.

La 0.3 est présentable lorsque ce parcours fonctionne réellement sur Windows et Linux, que les limites sont explicites et que le dossier de collaboration Steam s'appuie sur un démonstrateur vérifiable. La boutique commerciale avancée et les statistiques de compte ne sont pas des conditions de fin de la 0.3.
