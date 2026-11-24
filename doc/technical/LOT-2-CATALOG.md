# Lot 2 — Modèle catalogue et sources

**Statut : implémenté dans le workspace Rust.**

## Résultat

Le Lot 2 ajoute `crates/amethys-catalog` et initialise son schéma au démarrage de l'agent. Il construit les fondations métier et de persistance dont le Store du Lot 3 dépendra.

## Modèle

- `CatalogDocument` : document versionné, identifié par source et version de catalogue.
- `CatalogGame` : identité canonique Amethys et métadonnées propres au flux.
- `CatalogOffer` : offre liée à un fournisseur et à un identifiant externe.
- `DistributionMode` : `amethys_managed`, `official_platform_managed`, `external_link`, `metadata_only`.
- `OfferCapabilities` : capacités explicites, contrôlées par les capacités du fournisseur enregistré.
- `DistributionProviderAdapter` / `ProviderRegistry` : contrat et registre des fournisseurs.
- `CatalogSourceAdapter` : contrat de chargement d'une source de catalogue.

Les identités de jeux, d'offres et de fournisseurs sont distinctes. Aucune fusion par titre n'est réalisée. Les offres ne constituent pas une preuve de licence.

## Format et validation

Le format JSON est versionné (`schema_version = 1`) et refuse les champs inconnus. La validation vérifie les identifiants, doublons, limites de taille, longueurs, capacités réellement déclarées et cohérence du mode de distribution.

Les URLs d'action et références de manifestes doivent être HTTPS. Une offre Amethys-managed exige une capacité `direct_install` déclarée par l'adaptateur et une référence de manifeste. Une offre de plateforme officielle ne peut pas revendiquer les opérations de fichiers gérées par Amethys. Les sources externes ne peuvent pas déclarer d'installation directe.

Limites : 16 Mio par document, 100 000 jeux, 250 000 offres, 64 genres par jeu, 512 octets par titre. Ces limites sont centralisées dans la crate.

## Sources et capacités

`StaticJsonCatalogSource` lit un fichier régulier, refuse les liens symboliques, vérifie sa taille avant lecture et borne également la lecture effective. Le trait `CatalogSourceAdapter` permet d'ajouter ultérieurement des sources embarquées ou réseau sans changer le modèle.

Les fournisseurs de base `external-link` et `metadata-only` sont volontairement conservateurs : ils ne prétendent intégrer ni Steam, ni Epic, ni GOG, ni autre plateforme. Les intégrations officielles nécessiteront un adaptateur concret et des capacités documentées.

Le téléchargement/synchronisation HTTP n'est pas anticipé ici : le transport réseau et ses règles appartiennent aux lots ultérieurs. Le Lot 2 fournit la source statique locale de référence.

## Cache SQLite et hors ligne

Le dépôt catalogue est stocké dans la base SQLite détenue par le Core, sans ouvrir une deuxième base. Les tables sont :

- `catalog_schema_migrations` : versionnement isolé du sous-schéma catalogue ;
- `catalog_sources` : identité, version, empreinte SHA-256 et date de synchronisation ;
- `catalog_source_games` : métadonnées de jeu par source ;
- `catalog_offers` : offres et capacités déclarées.

Le cache par source est remplacé dans une transaction après lecture et validation complètes. Une erreur de lecture, de JSON, de validation ou d'écriture laisse l'ancien snapshot intact. Les identités canoniques préexistantes ne sont pas supprimées lorsqu'une source est rafraîchie ou retirée. Les données de chaque source sont séparées afin qu'un flux ne puisse pas écraser silencieusement les métadonnées d'un autre.

La recherche locale et la lecture des offres utilisent uniquement SQLite : elles fonctionnent sans réseau après une synchronisation réussie.

Le sous-schéma catalogue possède sa propre table de migrations car le crate de stockage 0.2 expose la connexion SQLite mais son numéro de schéma principal est encore plafonné à 5. Cela évite de faire croire que la migration principale 6 est prise en charge alors qu'elle ne l'est pas. Les migrations catalogue sont transactionnelles et indépendantes ; une consolidation dans le registre de migrations central pourra être faite lors d'une évolution du crate de stockage.

## Exemple

Le fichier [catalog-v1.example.json](../../catalogs/catalog-v1.example.json) est volontairement vide : il démontre le format sans présenter de jeux, prix, offres ou disponibilités fictifs comme des données réelles.

## Tests ciblés ajoutés

- validation d'un catalogue valide ;
- rejet des capacités non autorisées par un fournisseur ;
- rejet des doublons et versions de schéma inconnues ;
- rejet de l'installation directe sans autorisation explicite et manifeste ;
- rejet des documents vides, trop volumineux et contenant des champs inconnus ;
- validation stricte des URL HTTPS, hôtes et identifiants intégrés ;
- rejet des identifiants externes dupliqués pour un même fournisseur ;
- refus des fichiers sources trop volumineux et des liens symboliques sur Unix ;
- synchronisation, persistance et recherche hors ligne ;
- conservation de l'ancien cache après un rafraîchissement invalide ;
- suppression des offres obsolètes sans suppression de l'identité canonique ;
- monotonie des versions et immutabilité du contenu à version égale ;
- initialisation idempotente du schéma.

La campagne CQ globale reste reportée à la 0.4. Ces tests ciblés sont des tests de développement du Lot 2, pas une validation complète de release.
