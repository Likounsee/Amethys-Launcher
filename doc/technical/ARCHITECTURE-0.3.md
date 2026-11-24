# Architecture cible et fondations — Amethys Launcher 0.3

Ce document transforme l'audit du Lot 0 en décisions d'architecture applicables. Il constitue le Lot 1 de la 0.3. Il ne prétend pas que les modules décrits sont déjà implémentés.

## 1. Objectif

Construire une chaîne de distribution gérée par Amethys, de la fiche catalogue à l'installation et au lancement, sans réécrire la bibliothèque 0.2 ni remplacer les clients officiels des plateformes.

## 2. Règles non négociables

1. Le Core reste propriétaire des données et des opérations longues.
2. Le desktop et le CLI restent des clients IPC ; ils n'ouvrent jamais SQLite.
3. La bibliothèque locale, le catalogue Amethys et les identités des plateformes restent séparés.
4. Une opération longue est persistée et récupérable après redémarrage.
5. Les événements IPC/notifications ne sont qu'un mécanisme de rafraîchissement ; l'état stocké est la source de vérité.
6. Toute source et tout téléchargement doivent être autorisés par le fournisseur/détenteur des droits.
7. Aucun contournement de DRM, licence, authentification ou anti-cheat.
8. Aucun code arbitraire dans les manifestes ou profils.
9. Les chemins des manifestes sont relatifs, normalisés, limités au dossier géré et protégés contre traversal/symlinks.
10. Les installations existantes importées ne sont jamais déplacées ou écrasées automatiquement.
11. Aucun serveur payant obligatoire pour la bibliothèque locale ou la lecture d'un catalogue public statique.
12. Pas de compte, paiement, cloud saves ou fonctions sociales dans la 0.3.

## 3. Modèle conceptuel

Les concepts suivants doivent rester distincts :

- **CatalogGame** : fiche Amethys canonique, issue du catalogue maître commun à tous les utilisateurs (titre, description, genres, éditeur, médias, etc.). Son `amethys_id` est attribué exclusivement par le pipeline central du catalogue.
- **Bibliothèque Store** : vue de découverte du catalogue maître complet ; elle contient les jeux connus, qu’ils soient possédés, installés ou non.
- **Bibliothèque locale** : vue des seules installations détectées, importées ou gérées sur ce PC ; elle ne crée jamais de fiche ni d’ID maître.
- **CatalogOffer** : offre disponible dans une source (provider, identifiant externe, prix/devise si vérifiés, disponibilité, URL officielle).
- **DistributionSource** : origine et méthode d'accès aux métadonnées/paquets ; type et capacités documentés.
- **DistributionManifest** : document versionné qui décrit les fichiers autorisés d'une build et leurs empreintes.
- **DownloadJob** : opération de récupération persistée (source, manifeste, progression, état, erreurs).
- **ManagedInstall** : installation créée/possédée par Amethys, associée à un jeu, une source, une version et un chemin géré.
- **LocalInstallation** : installation locale déjà existante ou importée, indépendante de la gestion de distribution.
- **GameProfile** : configuration déclarative de lancement, versionnée et validée.
- **InstallOperation** : extraction/assemblage/activation d'une installation.
- **UpdatePlan** : comparaison entre version installée et version disponible.
- **RepairReport** : résultats de vérification des fichiers.

Une offre catalogue ne signifie pas que le jeu est téléchargeable directement par Amethys. Les capacités doivent être déclarées explicitement.

## 4. Modes de distribution

### Amethys-managed

Amethys peut télécharger et installer le paquet uniquement si les droits et le mécanisme sont explicitement autorisés. Le manifeste fournit la liste des fichiers, tailles, empreintes et chemins relatifs attendus. Les URL peuvent expirer ; les secrets ne sont jamais inclus dans le manifeste public.

### Official-platform-managed

Amethys utilise une API/CLI officiellement permise ou ouvre le client officiel. La plateforme garde la responsabilité de l'identité, de la licence, du DRM, du téléchargement protégé et des mises à jour. Une intégration absente ou non autorisée ne peut pas être simulée.

### Local-imported

Le jeu a été détecté/importé par la 0.2. Amethys conserve son chemin et ses capacités existantes. Il n'est pas converti automatiquement en ManagedInstall.

## 5. Architecture logique

```text
GhostEngine Desktop / CLI
          |
          | versioned local IPC
          v
      Amethys Agent (Core)
          |
          +-- Catalog Service
          |      +-- Catalogue Repository (SQLite/cache)
          |      +-- Static Catalogue Source
          |      +-- Provider Adapters
          |
          +-- Operation Coordinator
          |      +-- Persistent Job Repository
          |      +-- Worker Scheduler / Cancellation
          |
          +-- Download Service
          |      +-- HTTP transport
          |      +-- Resume / range support
          |      +-- Hash verification
          |
          +-- Install Service
          |      +-- Manifest validation
          |      +-- Safe extraction
          |      +-- Staging / activation / rollback
          |
          +-- Update & Repair Services
          |
          +-- Existing Library / Discovery / Launch
          |
          +-- SQLite + local media/cache
```

Le diagramme représente les responsabilités logiques, pas des processus distincts obligatoires. Les premiers lots peuvent rester dans l'agent et être extraits en crates indépendantes lorsque les interfaces sont stabilisées.

## 6. Découpage cible en crates

N'ajouter une crate que lorsqu'elle possède une frontière claire et des tests autonomes. N'imposer pas de micro-crates pour chaque fonction.

| Crate cible | Responsabilité |
|---|---|
| `amethys-catalog` | Modèles de catalogue, offres, manifestes et validation de schéma |
| `amethys-sources` | Traits/capacités et adaptateurs de sources officielles |
| `amethys-download` | Transport, reprise, progression, checksum, limitation de débit |
| `amethys-install` | Staging, extraction sûre, installation, désinstallation, rollback |
| `amethys-operations` | Cycle de vie persistant des jobs, concurrence, pause/reprise/annulation |
| `amethys-update` | Comparaison de versions, plan de mise à jour et réparation |
| `amethys-profiles` | Schéma et validation des profils de lancement déclaratifs |

Les crates existantes `storage`, `ipc`, `config`, `media`, `discovery`, `runtime`, `orchestration` restent les fondations. Le lot d'architecture décidera si certaines responsabilités doivent d'abord être développées dans une crate existante avant extraction.

## 7. Contrats de données à figer avant l'implémentation

### Identité

- `catalog_game_id` identifie le jeu Amethys.
- `provider + external_id` identifie une fiche/offre externe.
- `installation_id` identifie une installation locale unique.
- `managed_install_id` identifie une installation gérée par Amethys.
- Ces identifiants ne sont jamais substituables.

### Manifeste de distribution

Champs conceptuels minimaux :

- version de schéma ;
- Game ID Amethys ;
- source et identifiant d'offre ;
- identifiant/version/build ;
- plateforme et architecture ;
- méthode de distribution ;
- taille totale attendue ;
- fichiers : chemin relatif, taille, algorithme et empreinte ;
- métadonnées de dépendances/prérequis déclaratifs ;
- provenance/signature du manifeste ;
- date/expiration lorsque nécessaire.

Le manifeste ne peut contenir ni script shell, ni exécutable de post-installation arbitraire, ni commande privilégiée. Les dépendances doivent référencer des packages/prérequis connus et validés.

### État d'opération

États génériques :

`queued -> preparing -> running -> (pausing -> paused) -> completed`

États terminaux alternatifs : `cancelled`, `failed`, `needs_attention`.

Chaque opération possède un UUID stable, un type, un état, des timestamps, une progression récupérable, un message d'erreur stable et les références au jeu/source/installation. Les transitions sont contrôlées par le Core. La progression est indicative et ne doit pas être faussement précise.

### Progression

Exposer au minimum les octets reçus/total connus, le nombre de fichiers traités/total connu, l'étape courante et la vitesse si mesurable. Les valeurs inconnues restent nulles ; ne pas afficher 100 % avant vérification/activation finale.

## 8. Persistance SQLite

Ajouter des migrations numérotées après la version 5. La migration et chaque opération d'écriture doivent être transactionnelles lorsque plusieurs entités changent ensemble.

La base publique `catalog.db.zst` ne contient que le catalogue maître et ses identités de boutiques (`games`, `game_stores`, `catalog_meta`). Elle ne contient aucune donnée utilisateur, chemin local, installation ou entitlement.\n\nDans la base privée du Core, les tables locales restent séparées : `games` (installations locales historiques), `local_game_locations` (emplacements), `local_game_metadata` et les références facultatives vers un `amethys_id` maître. Un import local non rapproché reste valide sans `amethys_id`. Les métadonnées locales ne doivent pas être écrites dans le catalogue partagé.\n\nTables conceptuelles à introduire progressivement :

- `catalog_sources` / `catalog_offers` ou équivalent normalisé ;
- `distribution_manifests` (référence/version/provenance, pas nécessairement le binaire entier) ;
- `operations` ;
- `download_jobs` et leurs métadonnées de reprise ;
- `managed_installs` ;
- `managed_install_files` ou snapshots de manifestes ;
- `game_profiles` et versions de profils ;
- `update_plans` / `repair_reports` si l'état ne peut pas être dérivé.

Ne pas dupliquer les métadonnées déjà stockées dans `catalogue_game_metadata`. Prévoir les index d'identité unique, les clés étrangères, les limites de taille et la migration de données existantes. La suppression d'un ManagedInstall ne supprime pas le CatalogGame canonique.

## 9. Contrat IPC

Conserver un protocole versionné et le framing borné. Les opérations longues sont lancées par commande IPC, puis suivies par identifiant :

- catalogue : synchroniser/consulter/rechercher les jeux et offres ;
- opérations : créer, lister, consulter, mettre en pause, reprendre, annuler ;
- installation : demander l'installation d'une offre autorisée, consulter les installations gérées, désinstaller ;
- mise à jour/réparation : demander un plan, appliquer, vérifier, réparer ;
- profils : consulter le profil effectif et ses diagnostics.

Les noms et payloads définitifs seront figés dans les schémas du Lot 0 avant codage. Chaque mutation vérifie l'état du Core et ses préconditions. Les réponses d'acceptation retournent un identifiant d'opération ; elles n'attendent pas le téléchargement complet.

Pour compatibilité, documenter la stratégie de version de protocole : rejet clair des versions incompatibles, pas de désérialisation silencieuse d'une commande inconnue. Prévoir les champs optionnels/versionnés nécessaires aux clients de même release.

## 10. Sécurité des fichiers et des archives

- Le répertoire racine des installations gérées est configuré par l'utilisateur et préparé explicitement.
- Les manifestes utilisent des chemins relatifs uniquement.
- Rejeter les chemins absolus, préfixes de volume, `..`, composants invalides, NUL, doublons ambigus et collisions de casse pertinentes.
- Rejeter les liens symboliques/hardlinks d'archive qui sortent de la racine ou permettent de l'écraser.
- Vérifier les tailles réelles, tailles cumulées, nombre d'entrées et limites de compression avant extraction.
- Écrire dans un dossier temporaire/staging créé par Amethys.
- Vérifier les empreintes après réception et après assemblage.
- Activer l'installation par renommage contrôlé lorsque le système de fichiers le permet ; sinon utiliser une procédure documentée.
- Ne jamais supprimer/écraser une installation importée ou un dossier préexistant.
- En cas d'échec, conserver l'ancienne version et nettoyer uniquement les fichiers temporaires possédés par l'opération.

## 11. Téléchargement et reprise

Le téléchargement doit utiliser un worker asynchrone, des limites de concurrence et une persistance périodique. La reprise n'est autorisée que si le serveur/source confirme que la plage et le validateur (ETag/Last-Modified ou équivalent) sont cohérents ; sinon recommencer proprement.

Les URLs signées et tokens éphémères sont des données sensibles : ne pas les écrire dans les logs, les erreurs publiques ou les manifestes partagés. Les redirections doivent être contrôlées et les schémas non sûrs refusés.

## 12. UI

Le desktop reste une vue du Core. Pages et états :

- Store : chargement, vide, erreur, hors ligne/cache, résultats, détails, offre indisponible, redirection vers source officielle ;
- téléchargements : file, progression, pause, reprise, annulation, échec, reprise après redémarrage ;
- installation : emplacement, espace requis, confirmation, étape, résultat ;
- Store : catalogue global complet, indépendant de l’installation et de la possession ;\n- Ma bibliothèque : uniquement les installations locales de ce PC, badge de provenance (importé / géré), version, état, action de lancement ;
- mise à jour/réparation : disponibilité, plan, progression, résultat et rapport ;
- paramètres : racine d'installation, concurrence, réseau/cache, espace.

Aucun prix, avis, promotion, disponibilité ou contenu de catalogue fictif. Les données de démonstration existantes ne sont pas réutilisées comme source de production.

## 13. Stratégie sans backend obligatoire

Le catalogue public est maintenu dans le dépôt dédié `Likounsee/Amethys-Catalog` et distribué sous forme de fichiers statiques versionnés (index compact + fragments de détails à la demande). Le Core doit utiliser un contrat `CatalogProvider` indépendant de GitHub ; GitHub Pages est le premier adaptateur, un serveur Amethys pourra le remplacer ultérieurement sans réécrire le Store. Le cache local permet la recherche hors ligne et les fragments sont téléchargés à la demande.

Cette approche ne fournit pas à elle seule des comptes, des achats, des licences, des API privées ou des fichiers de jeux commerciaux. Les intégrations dépendant d'un backend, d'accords éditeur ou de secrets utilisateurs restent optionnelles et explicites.

## 14. Architecture de test pendant la 0.3

La campagne globale de Quality Check / CQ est reportée à la 0.4, conformément à la décision produit. Pendant la 0.3, chaque lot doit néanmoins avoir les vérifications ciblées indispensables à son développement : tests unitaires des parseurs et transitions, fixtures locales, validation de migrations, tests de chemins et tests manuels de la fonctionnalité construite. Cela n'est pas la campagne CQ finale.

Ne jamais désactiver une vérification existante pour masquer une régression. La campagne de consolidation, de compatibilité multi-OS et de release est planifiée dans la 0.4.

## 15. Sortie du Lot 0

Le Lot 0 est terminé lorsque :

- les responsabilités et frontières sont écrites ;
- les identités catalogue/offre/installation sont distinguées ;
- les modes de distribution et leurs limites sont définis ;
- les états d'opérations et contrats IPC sont spécifiés ;
- le modèle de persistance et la stratégie de migration sont définis ;
- les règles de sécurité téléchargement/archives/installation sont définies ;
- les dépendances entre lots et critères d'acceptation sont publiés.

Le Lot 0 ne doit pas créer prématurément toutes les crates ou implémenter le moteur de téléchargement : ces travaux appartiennent aux lots fonctionnels.
