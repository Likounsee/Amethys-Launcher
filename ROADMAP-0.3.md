# Amethys Launcher 0.3 — Roadmap de développement détaillée

**Statut :** Lot 0 (audit), Lot 1 (architecture/fondations) et Lot 2 (catalogue/sources) terminés ; CI ciblée du Lot 2 verte.  
**Base :** branche `main` ; implémentation du Lot 2 suivie directement sur `main`.  
**Version précédente :** 0.2.0 gelée.  
**Quality Check / CQ globale :** reportée à la version 0.4.

## Objectif produit de la 0.3

Transformer Amethys en plateforme capable de présenter un catalogue réel et de gérer le téléchargement, l'installation, la bibliothèque, les mises à jour, la réparation et le lancement des jeux lorsque leur mode de distribution est officiellement autorisé.

La 0.3 ne remplace pas les comptes, licences, DRM ou protections des plateformes. Une fiche catalogue n'implique pas qu'Amethys possède le droit ou la capacité de télécharger le jeu. Lorsqu'une intégration directe n'est pas autorisée/disponible, Amethys doit utiliser le client officiel ou proposer un lien vers la boutique officielle.

## Décision de périmètre : CQ

La campagne globale de Quality Check, la validation de release et la consolidation qualité finale ne font **pas** partie de la 0.3 ; elles constituent la 0.4. Les lots 0.3 gardent uniquement les vérifications ciblées nécessaires pour ne pas développer à l'aveugle : tests de logique, fixtures, migrations et essais fonctionnels du composant en cours. Les vérifications déjà présentes ne doivent pas être supprimées ou contournées.

## Ordre d'exécution

L'ordre de préparation est respecté : **Lot 0 (audit de l'existant) a été réalisé en premier, puis Lot 1 (architecture et fondations)**. Les lots fonctionnels commencent ensuite.

| Ordre | Lot | Résultat attendu | Dépendances |
|---:|---|---|---|
| 1 | Lot 0 — Audit complet de l'existant | État réel du dépôt, inventaire, réutilisation, manques et risques | Aucune |
| 2 | Lot 1 — Architecture et fondations | Contrats, modèle métier, persistance, IPC, sécurité, frontières | Lot 1 |
| 3 | Lot 2 — Modèle catalogue et sources | Implémentation réalisée ; validation ciblée CI en cours — modèles, catalogue versionné, cache SQLite, capacités et adaptateur statique | Lots 0 et 1 |
| 4 | Lot 3 — Store réel | Recherche, catégories, fiches, offres et états issus de sources réelles | Lot 2 |
| 5 | Lot 4 — Gestionnaire d'opérations | Jobs persistés, file, états, concurrence, progression, pause/reprise/annulation | Lot 0 |
| 6 | Lot 5 — Download Manager | Téléchargement autorisé, reprise sûre, contrôle d'intégrité | Lots 2 et 4 |
| 7 | Lot 6 — Install Manager | Validation manifeste, staging, extraction sûre, activation et désinstallation | Lots 4 et 5 |
| 8 | Lot 7 — Intégration bibliothèque/lancement | Jeux gérés intégrés sans casser les jeux importés, profils et méthodes de lancement | Lots 2 et 6 |
| 9 | Lot 8 — Update & Repair Manager | Comparaison de versions, plan, mise à jour et réparation | Lots 4, 6 et 7 |
| 10 | Lot 9 — Intégration desktop complète | Store, téléchargements, installation, bibliothèque et mises à jour reliés au Core | Lots 3 à 8 |
| 11 | Lot 10 — Finalisation fonctionnelle 0.3 | Parcours produit cohérent, documentation d'utilisation et limites connues | Lots 2 à 9 |

Les numéros suivent la convention corrigée : Lot 0 = audit ; Lot 1 = architecture et fondations.

---

## Lot 0 — Audit complet de l'existant

**Statut : terminé.**

Livrables :
- [Audit technique de la base 0.2](technical/LOT-0-AUDIT-0.3.md)
- Inventaire des applications et crates ;
- inventaire du stockage et des migrations ;
- état de l'IPC et du transport ;
- état de la découverte, de la bibliothèque et du lancement ;
- état du desktop et des maquettes ;
- limites de distribution et risques ;
- décisions de conservation/non-réécriture.

Critère de fin : les éléments existants et absents sont distingués sans supposer qu'une fondation générique est déjà une fonctionnalité Store.

## Lot 1 — Architecture et fondations

**Statut : terminé au niveau de la conception. Aucune implémentation métier anticipée.**

Livrable : [Architecture cible et fondations 0.3](technical/ARCHITECTURE-0.3.md).

Décisions figées :
- Core propriétaire des données et opérations ;
- UI/CLI clients IPC ;
- séparation CatalogGame / CatalogOffer / DistributionSource / ManagedInstall / LocalInstallation ;
- trois modes de distribution : Amethys-managed, Official-platform-managed, Local-imported ;
- opérations persistées et récupérables ;
- manifestes déclaratifs, validés et sans code arbitraire ;
- migrations SQLite non destructives après le schéma 5 ;
- staging, vérification, activation et rollback ;
- catalogue statique/cache local sans backend payant obligatoire ;
- respect strict des mécanismes officiels des plateformes.

Critère de fin : les lots fonctionnels disposent de contrats, dépendances et règles de sécurité explicites.

## Lot 2 — Modèle catalogue et sources

**Statut : terminé ; validation CI ciblée réussie sur Windows et Linux.** Implémentation : [modèle catalogue, sources et cache](technical/LOT-2-CATALOG.md).

**But :** créer les structures réelles sur lesquelles le Store reposera.

Travaux :
1. Définir les types Rust pour jeux catalogue, offres, fournisseurs, capacités et manifestes.
2. Ajouter les schémas de sérialisation et versions.
3. Ajouter les migrations SQLite nécessaires au cache catalogue et aux offres.
4. Définir un format de catalogue statique versionné.
5. Ajouter un chargeur avec validation stricte, limites de taille et provenance.
6. Ajouter un cache local avec date de dernière synchronisation et comportement hors ligne.
7. Définir le trait d'adaptateur fournisseur et ses capacités (navigation, métadonnées, lancement officiel, installation autorisée, mise à jour autorisée).
8. Ajouter un fournisseur de catalogue statique de référence et des fixtures ; aucune offre fictive ne doit être présentée comme réelle.

Livré : crate `amethys-catalog`, validation stricte du JSON versionné, adaptateur fichier statique borné, registre de fournisseurs/capacités, cache SQLite transactionnel isolé par source, recherche locale et initialisation du schéma au démarrage du Core. Le format d’exemple est volontairement vide : aucune offre commerciale n’est inventée.

Critères d'acceptation :
- un catalogue valide est chargé et mis en cache ;
- un catalogue invalide ou trop volumineux est rejeté proprement ;
- une mise à jour ne détruit pas le dernier cache valide ;
- les identités externes ne fusionnent pas des jeux par simple nom ;
- l'absence de réseau conserve les données mises en cache.

## Lot 3 — Store réel

**But :** rendre la page Store réellement alimentée par le catalogue du Lot 2.

Travaux :
1. API Core de recherche, liste, catégories, filtres et détails.
2. Résolution des médias et chargement paresseux/cache.
3. Affichage des offres, fournisseurs, prix/devise seulement lorsqu'ils sont vérifiés et datés.
4. Distinction entre téléchargeable par Amethys, géré par plateforme officielle, lien externe et indisponible.
5. Liens vers les pages officielles.
6. États de chargement, vide, erreur et hors ligne.
7. Remplacement des données de maquette par les données IPC réelles.

Critères :
- aucune donnée commerciale inventée ;
- chaque offre indique sa source ;
- un jeu sans méthode d'installation autorisée n'affiche pas une action d'installation Amethys ;
- le Store reste consultable depuis le cache hors ligne.

## Lot 4 — Gestionnaire d'opérations persistantes

**But :** bâtir le moteur commun utilisé par téléchargement, installation, mise à jour et réparation.

Travaux :
1. Modèle d'opération et machine à états.
2. Persistance des opérations et de leur progression.
3. Identifiants d'opération stables.
4. Workers asynchrones, concurrence configurable et arrêt contrôlé.
5. Pause, reprise et annulation coopératives.
6. Récupération au redémarrage : reprendre, replanifier ou marquer needs_attention selon le type d'opération.
7. API IPC pour créer/lister/consulter/contrôler les opérations.
8. Notifications d'événements ; l'état relu depuis le Core reste la vérité.

Critères :
- une opération survit au redémarrage de l'agent ;
- une commande répétée ne crée pas de doublon involontaire ;
- transitions illégales refusées ;
- erreur et annulation ne laissent pas un état bloqué ;
- UI/CLI peuvent retrouver une opération par son ID.

## Lot 5 — Download Manager

**But :** télécharger des fichiers uniquement depuis une source autorisée.

Travaux :
1. Transport HTTP asynchrone et gestion des redirections contrôlées.
2. Téléchargement en streaming sans charger le paquet entier en mémoire.
3. Limites de concurrence et de débit.
4. Progression octets/fichiers, vitesse et taille inconnue gérées proprement.
5. Fichiers temporaires et métadonnées de reprise.
6. Reprise conditionnelle via Range et validateurs serveur ; redémarrage complet si la cohérence ne peut être prouvée.
7. Contrôle SHA-256 (ou algorithme explicitement déclaré et supporté) après téléchargement.
8. Gestion des erreurs, timeouts, espace disque et annulation.
9. Masquage des URL signées et tokens dans les logs.

Critères :
- fichier téléchargé correspond à la taille et à l'empreinte ;
- interruption et reprise n'acceptent pas de contenu incohérent ;
- téléchargement corrompu refusé ;
- aucun fichier final n'est publié avant validation.

## Lot 6 — Install Manager

**But :** convertir un téléchargement vérifié en installation gérée et récupérable.

Travaux :
1. Validation du manifeste et des capacités de source.
2. Vérification du chemin d'installation choisi et de l'espace disque.
3. Extraction sûre avec limites d'entrées, tailles et compression.
4. Protection traversal, liens et collisions de chemins.
5. Installation en répertoire staging possédé par Amethys.
6. Vérification des fichiers après extraction.
7. Activation contrôlée et sauvegarde de l'ancienne version si remplacement.
8. Désinstallation limitée aux fichiers enregistrés comme appartenant à Amethys.
9. Nettoyage après interruption sans toucher aux installations importées.

Critères :
- installation complète et vérifiée avant d'être visible comme prête ;
- interruption ne détruit pas l'ancienne installation ;
- archive malveillante rejetée ;
- désinstallation ne sort jamais du répertoire géré.

## Lot 7 — Intégration bibliothèque et lancement

**But :** faire cohabiter proprement installations importées et gérées.

Travaux :
1. Relier ManagedInstall au Game ID canonique.
2. Conserver un identifiant distinct pour chaque installation.
3. Afficher la provenance : importée ou gérée par Amethys.
4. Ajouter version, source, emplacement et état d'installation.
5. Ajouter Game Profiles déclaratifs et versionnés.
6. Décrire point d'entrée relatif, working directory, arguments autorisés, méthode de lancement et prérequis.
7. Valider le profil et les chemins avant lancement.
8. Supporter lancement direct ou hand-off vers launcher officiel lorsque documenté.
9. Préserver les validations de sécurité du lancement 0.2.

Critères :
- les jeux importés continuent à apparaître et se lancer comme avant ;
- les jeux gérés apparaissent après installation ;
- aucun profil ne peut lancer un chemin hors installation ;
- les méthodes officielles ne sont pas remplacées par une simulation.

## Lot 8 — Update & Repair Manager

**But :** maintenir les installations gérées cohérentes.

Travaux :
1. Comparer manifeste/build installés et build disponible.
2. Produire un plan explicite avant modification.
3. Télécharger uniquement les fichiers nécessaires si le manifeste le permet.
4. Appliquer en staging et vérifier avant activation.
5. Préserver l'ancienne version pour rollback lorsque possible.
6. Vérifier les empreintes de l'installation.
7. Réparer les fichiers manquants/corrompus depuis la source autorisée.
8. Générer un rapport de réparation.
9. Gérer les cas où la source ne permet pas le delta ou le rollback.

Critères :
- une mise à jour incomplète ne remplace pas une version fonctionnelle ;
- une réparation ne touche que les fichiers appartenant à l'installation gérée ;
- les installations importées ne sont pas modifiées sans mécanisme officiel explicite.

## Lot 9 — Intégration desktop complète

**But :** connecter toutes les pages et opérations au vrai Core.

Travaux :
1. Store : recherche, catégories, détails, offres et action disponible.
2. Téléchargements : file, progression, pause/reprise/annulation, historique.
3. Installation : emplacement, espace, confirmation et résultat.
4. Bibliothèque : provenance, version, état et lancement.
5. Mises à jour : badge, détails, appliquer.
6. Réparation : diagnostic et résultat.
7. Notifications cohérentes, erreurs compréhensibles et récupération de l'état après redémarrage.
8. Paramètres de stockage géré, concurrence et cache.
9. Supprimer les chemins UI de maquette qui pourraient être confondus avec des données réelles.

Critères :
- aucun écran ne simule une opération ;
- toute action visible est connectée à une commande Core réelle ou désactivée avec explication ;
- la fermeture/réouverture de l'UI ne perd pas l'état d'une opération Core.

## Lot 10 — Finalisation fonctionnelle 0.3

**But :** terminer le périmètre fonctionnel sans lancer la campagne CQ 0.4.

Travaux :
1. Parcours utilisateur complet pour chaque mode de distribution effectivement supporté.
2. Messages et états cohérents dans les cas sans fournisseur compatible.
3. Documentation utilisateur : catalogue, sources, installation, stockage, erreurs et limites.
4. Documentation technique des adaptateurs et manifestes.
5. Nettoyage des placeholders et fonctionnalités annoncées mais non implémentées.
6. Inventaire précis des fonctions réellement prises en charge et des dépendances à des accords éditeurs.
7. Préparer une liste de validation 0.4 sans exécuter la campagne CQ complète.

Critère :
- les fonctionnalités annoncées comme disponibles fonctionnent réellement sur les sources prises en charge ;
- les limites sont explicites ;
- les parcours non supportés renvoient vers la source officielle plutôt que de promettre une installation impossible.

---

## Dépendances critiques

- Le Store dépend du modèle catalogue et des adaptateurs (Lot 2 — terminé et validé).
- Le téléchargement dépend des opérations persistantes (Lot 4) et des manifestes/sources (Lot 2).
- L'installation dépend du téléchargement vérifié et du moteur d'opérations (Lots 4-5).
- L'intégration bibliothèque dépend du modèle d'installation (Lot 6).
- Les mises à jour/réparations dépendent du modèle d'installation et du manifeste (Lots 6-7).
- L'interface finale dépend des contrats IPC et de l'état réel des lots métier (Lots 3-8).

## Hors périmètre 0.3

- Comptes Amethys complets et synchronisation multi-appareils.
- Paiement, portefeuille, achats et gestion de licences propriétaire.
- Cloud Gaming.
- Synchronisation cloud des sauvegardes.
- Réseau social, amis, chat.
- Workshop/mods intégrés.
- Gestion universelle de tous les éditeurs sans API/accord.
- Contournement DRM, anti-cheat, authentification ou restrictions régionales.
- Backend payant obligatoire.
- Campagne globale CQ, validation finale multi-OS et release hardening : 0.4.

## Critère global de fin fonctionnelle 0.3

Pour au moins une source de distribution réellement autorisée et documentée, l'utilisateur doit pouvoir consulter une offre réelle, démarrer une opération, télécharger et vérifier les fichiers, installer dans un emplacement choisi, retrouver le jeu dans la bibliothèque, le lancer selon le profil valide, puis consulter et appliquer une mise à jour ou réparer l'installation si la source le permet.

Les autres sources peuvent être au stade catalogue/lien officiel si leurs mécanismes d'installation ne sont pas accessibles. Le support doit être présenté avec honnêteté, fournisseur par fournisseur.

## Règles de travail pour tous les lots

- Développement directement sur `main`, sans Pull Request.
- Un lot commence par la lecture des fichiers concernés et de leurs appels existants.
- Chaque changement doit être cohérent avec les contrats des Lots 0 et 1.
- Pas de fonctionnalité fictive, pas de données commerciales inventées.
- Ne pas déclarer un test exécuté s'il ne l'a pas été.
- Ne pas élargir le périmètre sans mettre à jour cette roadmap.
