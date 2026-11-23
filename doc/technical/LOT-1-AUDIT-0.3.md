# Audit technique complet — base de la 0.3

- Date de l'audit : 23 novembre 2026
- Branche examinée : `main`
- Commit examiné : `33903f17abf4772a617564dfe0c028a34c67ae9e`
- Périmètre : inventaire du dépôt, des applications, des crates Rust, du stockage, de l'IPC, de la découverte, du client desktop et de la documentation.
- Nature : audit documentaire du code source GitHub. Il ne prétend pas remplacer une exécution locale ou une validation sur une machine physique.

## 1. Conclusion exécutive

La 0.2 fournit déjà une base logicielle plus avancée qu'une simple bibliothèque locale. Il existe un workspace Rust organisé, un agent central, un client CLI, un client desktop GhostEngine, un protocole IPC versionné, une base SQLite migrée, des identifiants canoniques de catalogue, des identifiants de plateformes, un stockage média local et une découverte de jeux.

**La 0.3 ne doit donc pas réécrire Amethys.** Elle doit prolonger le Core existant et ajouter une chaîne de distribution gérée par Amethys, tout en préservant les installations importées et les mécanismes de lancement de la 0.2.

En revanche, le dépôt ne contient pas encore de véritable Store connecté ni de moteur de téléchargement, d'installation, de mise à jour ou de réparation. Les composants génériques de runtime, d'événements et d'orchestration ne constituent pas encore ces systèmes métier.

## 2. Inventaire du dépôt

Le dépôt examiné contient 143 fichiers suivis dans l'arbre Git à la révision auditée.

### Applications

| Élément | État constaté | Décision 0.3 |
|---|---|---|
| `apps/agent` | Agent Rust central ; possède la base et traite les commandes IPC | Conserver et étendre progressivement |
| `apps/cli` | Client CLI pour les opérations Core et la découverte | Conserver ; ajouter des commandes de diagnostic/gestion si utiles |
| `prototypes/ghostengine-amethys-preview` | Application desktop expérimentale réellement reliée au Core | Conserver comme client desktop de référence |
| `amethys-launcher-ui` | Dossier de prototypes/aperçus et éléments UI historiques distincts du manifeste desktop exécuté | Ne pas confondre avec le client desktop compilé |

Le desktop actuel est un prototype Rust autonome situé dans `prototypes/ghostengine-amethys-preview`. Son manifeste dépend du GhostEngine local et des crates Amethys. Le workspace racine ne l'inclut pas comme membre : il possède son propre manifeste/workspace.

### Crates existantes

| Crate | Responsabilité constatée | Réutilisation |
|---|---|---|
| `amethys-config` | Chemins de données par OS, configuration JSON versionnée, écritures temporaires | Réutiliser pour config et emplacements gérés |
| `amethys-discovery` | Détection en lecture seule des installations/launchers et candidats exécutables | Conserver ; ne pas lui confier les installations |
| `amethys-errors` | Codes d'erreur stables et messages | Étendre avec des domaines Store/download/install |
| `amethys-events` | Bus d'événements Tokio broadcast | Réutiliser pour notifications de progression et d'état, sans en faire une file de travaux persistante |
| `amethys-ipc` | Enveloppes JSON, UUID de corrélation, protocole versionné, framing borné, commandes et réponses | Étendre avec compatibilité explicite et commandes 0.3 |
| `amethys-logging` | Initialisation tracing | Conserver et structurer les événements des opérations |
| `amethys-media` | Chemins sûrs pour médias de bibliothèque et cache de médias catalogue | Réutiliser ; ajouter le transport réseau ailleurs |
| `amethys-orchestration` | Gestionnaires génériques de modules, tâches et workers avec arrêt borné | Réutiliser comme primitive ; ajouter un gestionnaire métier durable |
| `amethys-runtime` | Cycle de vie du Core et bus d'événements | Conserver |
| `amethys-storage` | SQLite, migrations, bibliothèque, catalogue canonique, métadonnées, identifiants externes, paramètres | Étendre par migrations non destructives |
| `vendor/ghostengine` | Snapshot local du moteur UI | Conserver pour le desktop actuel ; ne pas coupler le Core à l'UI |

## 3. Ce qui existe déjà dans le modèle de données

La base SQLite possède un historique de migrations jusqu'à la version 5. Les migrations créent notamment :

- les paramètres applicatifs ;
- les installations/entrées de bibliothèque ;
- les métadonnées associées aux jeux ;
- les identifiants externes des fournisseurs ;
- les jeux canoniques du catalogue et leurs métadonnées ;
- les relations entre identités externes et jeux canoniques.

Le code distingue déjà l'identifiant d'une installation locale de l'identifiant canonique d'un jeu Amethys. Cette distinction est essentielle : plusieurs installations du même jeu peuvent partager une fiche catalogue sans être fusionnées en une seule installation.

Les opérations existantes couvrent l'ajout, la recherche, la liste, la configuration d'exécutable, la suppression, les paramètres, les métadonnées et les identifiants externes.

**Manques pour la 0.3 :** aucun modèle persistant dédié aux produits du Store, aux sources de distribution, aux manifestes de fichiers, aux tâches de téléchargement, aux installations gérées, aux versions installées, aux opérations de mise à jour ou aux réparations n'a été identifié dans les migrations actuelles.

La base doit évoluer par nouvelles migrations. Les données de bibliothèque existantes doivent rester intactes ; supprimer/recréer la base n'est pas une stratégie de migration acceptable.

## 4. Core, IPC et sécurité

### Points réutilisables

- L'agent est propriétaire de SQLite ; le desktop ne l'ouvre pas directement.
- Les échanges sont encadrés par une version de protocole, un identifiant de requête et une taille maximale de frame.
- Le transport possède des implémentations Windows named pipe et Unix socket.
- Les tests du transport couvrent notamment les sockets privés, les connexions multiples et l'identité du pair named pipe.
- Les changements de bibliothèque sont refusés lorsque le Core n'est pas dans l'état autorisé.
- Le Core limite les connexions concurrentes et protège l'accès à la connexion SQLite.
- Les chemins de données utilisateur sont résolus par OS et le répertoire Unix privé est contrôlé.

### Limites à traiter avant d'exposer les opérations 0.3

- Le protocole actuel est en version 1 et ne définit pas encore les contrats Store/download/install/update.
- Les commandes IPC actuelles sont centrées sur Ping, état Core et bibliothèque locale.
- Le client desktop crée des runtimes Tokio ponctuels pour ses appels IPC ; la progression longue durée nécessitera une stratégie de requêtes et d'événements cohérente.
- Le bus broadcast actuel est volatil : une UI déconnectée peut manquer des événements. L'état d'une opération doit donc être récupérable depuis le Core et la base.
- Les opérations longues ne doivent pas être exécutées dans le traitement synchrone d'une requête IPC.
- Les autorisations locales, la validation des chemins et la séparation entre commandes de lecture et mutations doivent être maintenues pour toutes les nouvelles commandes.

Décision : conserver le Core comme autorité ; ajouter des opérations identifiées et persistées, puis exposer par IPC des commandes de démarrage, pause, reprise, annulation et consultation d'état. Les événements servent à accélérer l'UI, mais une interrogation d'état doit toujours permettre de retrouver la vérité.

## 5. Découverte, installation et lancement

La crate `amethys-discovery` et l'agent savent détecter des installations, enrichir certains identifiants de fournisseurs et proposer des exécutables. La découverte reste en lecture seule et l'import est explicite/idempotent.

Le lancement desktop valide que l'exécutable résolu reste dans le dossier d'installation enregistré. Cette protection ne doit pas être affaiblie.

À ne pas confondre :

- **Découverte** : constater qu'un jeu existe déjà sur le disque.
- **Import** : enregistrer une installation existante dans la bibliothèque.
- **Téléchargement** : récupérer les fichiers depuis une source autorisée.
- **Installation gérée** : créer ou mettre à jour un répertoire appartenant à Amethys.
- **Lancement** : démarrer l'entrée déclarée et validée, directement ou par un launcher officiel.

La 0.3 doit ajouter l'installation gérée sans casser la découverte/import de la 0.2.

## 6. Interface desktop

Le client desktop utilise les primitives GhostEngine et lit les jeux réels par IPC. Il possède déjà les pages Bibliothèque, Store et Paramètres ainsi que les interactions de base, les états de chargement/erreur, les notifications, la navigation clavier et la gestion des fenêtres compactes.

Le code UI comprend aussi des données de maquette contenant des titres, prix, promotions, recommandations et états de bibliothèque fictifs. Ces données sont explicitement décrites comme des placeholders dans le module de données ; elles ne constituent pas un catalogue réel et ne doivent jamais alimenter une boutique publiée.

Décision :

- réutiliser la fenêtre, le thème, la navigation et les composants UI existants ;
- remplacer progressivement tout contenu de maquette par des réponses réelles du Core ;
- ne jamais présenter de prix, disponibilité, note ou promotion inventés ;
- afficher un état vide/indisponible clair tant qu'aucune source réelle n'est configurée ;
- éviter de réécrire toute l'interface avant que les contrats Core/IPC soient stabilisés.

## 7. Distribution et contraintes des fournisseurs

Le README et les principes du projet indiquent qu'Amethys ne remplace pas les comptes, licences, DRM ou politiques des boutiques officielles.

Il n'existe pas de preuve, dans le code examiné, d'un accès universel autorisé aux fichiers de jeux commerciaux Steam, Epic, GOG ou Xbox. La présence d'un identifiant externe ou d'un nom de plateforme n'accorde aucun droit de télécharger, d'installer ou de redistribuer les fichiers du jeu.

La 0.3 doit donc distinguer explicitement les modes de distribution :

1. **Amethys-managed** : Amethys télécharge et installe uniquement un paquet dont la source, les droits, le manifeste et la méthode de distribution sont autorisés.
2. **Official-platform-managed** : Amethys dirige l'utilisateur vers le client/API officiel ou appelle une intégration officiellement permise ; la plateforme reste responsable de l'authentification, de la licence, du DRM et des fichiers protégés.
3. **Local-imported** : l'installation existe déjà ; Amethys la référence et la lance selon les capacités disponibles, sans la revendiquer comme installée par Amethys.

Si aucun accès officiel n'est disponible, la fiche peut proposer d'ouvrir la boutique ou le launcher officiel, mais ne doit pas afficher un bouton de téléchargement Amethys fictif.

## 8. Manques fonctionnels confirmés pour la 0.3

Aucun composant métier dédié n'a été identifié pour :

- catalogue distant réellement chargé et mis en cache ;
- registre de sources et adaptateurs fournisseurs ;
- validation/versionnement/signature de manifestes de distribution ;
- téléchargement HTTP/streaming, reprise et contrôle d'intégrité ;
- planificateur de téléchargements et limites de concurrence ;
- extraction sécurisée d'archives ;
- installation transactionnelle et récupération après interruption ;
- registre d'installations gérées ;
- comparaison de versions et mise à jour ;
- vérification/réparation de fichiers ;
- profils de lancement versionnés et liés à un jeu/source ;
- gestion unifiée des opérations longues et de leur progression persistée.

Les bibliothèques présentes sont des fondations, pas des implémentations de ces fonctionnalités.

## 9. Risques architecturaux à traiter dans le Lot 0

1. **Ne pas transformer le catalogue local en catalogue distant par simple ajout de champs.** Le produit catalogue, l'identité fournisseur et l'installation locale sont des entités différentes.
2. **Ne pas confondre événement et état persistant.** Une opération doit survivre au redémarrage de l'agent.
3. **Ne pas exécuter le réseau, les extractions ou les téléchargements dans une requête IPC bloquante.**
4. **Ne pas laisser un manifeste choisir un chemin arbitraire.** Les chemins doivent être relatifs, normalisés et contenus dans le répertoire géré.
5. **Ne pas remplacer les fichiers d'une installation existante sans stratégie de sauvegarde/rollback.**
6. **Ne pas supposer que tous les fournisseurs autorisent une installation directe.**
7. **Ne pas stocker de secrets de plateforme dans le catalogue public ou les logs.**
8. **Ne pas ajouter un serveur payant obligatoire.** Le catalogue public statique et le cache local restent privilégiés pour la base du produit ; les services nécessitant un backend sont différés.
9. **Ne pas coupler le Core à GhostEngine.** Le desktop est un client IPC comme le CLI.
10. **Ne pas faire de refonte globale prématurée.** Les nouvelles frontières doivent être créées au rythme des lots et validées contre les besoins réels.

## 10. Décisions de conservation

- Conserver le workspace Rust et les applications agent/CLI.
- Conserver SQLite et faire évoluer le schéma par migrations.
- Conserver le Core comme propriétaire des données persistantes.
- Conserver l'IPC local comme frontière entre UI/CLI et Core.
- Conserver la découverte en lecture seule et l'import des jeux existants.
- Conserver les IDs d'installation, Game IDs canoniques et IDs externes séparés.
- Conserver le stockage média local et son cache.
- Conserver GhostEngine et le client desktop actuel.
- Conserver les principes de sécurité : aucun contournement DRM, licence, authentification ou anti-cheat.

## 11. Décisions de non-réécriture

Ne pas réécrire la découverte, la bibliothèque, le système de lancement, la base entière, l'IPC ou l'UI simplement pour introduire le Store. Les nouveaux domaines doivent s'intégrer aux contrats existants et n'extraire un composant que lorsqu'une responsabilité distincte le justifie.

## 12. Conclusion du Lot 1

Le Lot 1 est terminé au niveau de l'audit du dépôt : les composants à réutiliser, les absences fonctionnelles, les risques et les contraintes d'intégration sont identifiés.

La prochaine étape est le Lot 0 : figer les contrats et l'architecture de la 0.3 avant de commencer l'implémentation métier. Aucun moteur Store/download/install n'est déclaré déjà présent.
