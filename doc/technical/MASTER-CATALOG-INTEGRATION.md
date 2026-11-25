# Intégration du catalogue maître dans Amethys Core

## Architecture

Le crate `amethys-catalog` expose maintenant `remote::CatalogProvider`, un contrat de fournisseur basé sur une URL HTTPS. `StaticCatalogProvider` est l'adaptateur de fichiers statiques ; son alias initial `GitHubPagesCatalogProvider` conserve le point d'entrée GitHub Pages sans l'imposer aux consommateurs.

Le client `RemoteCatalogClient` :
- télécharge et valide le manifeste v2 ;
- télécharge uniquement l'index compact Zstandard ;
- vérifie taille et SHA-256 avant décompression ;
- borne les tailles compressées et décompressées ;
- valide les IDs uniques et triés ;
- met en cache l'index JSON pour la recherche hors ligne ;
- télécharge les fragments de détails à la demande et les met en cache par bucket et checksum.

Le client ne télécharge jamais la base SQLite temporaire du pipeline. Le cache catalogue est distinct des tables de bibliothèque locale et ne crée aucune preuve de possession ou de licence. Les détails n'exigent pas d'identifiant IGDB : celui-ci est une provenance optionnelle, jamais une clé canonique. Les fiches exposent aussi `source_ids`, une liste générique de couples espace de noms / identifiant externe (IGDB, boutiques ou futurs fournisseurs autorisés). Le manifeste compte ces identités séparément des identités de boutique. Les jeux sans offre de boutique restent valides dans le catalogue maître.

## IPC et CLI

Le Core expose :
- `SyncMasterCatalog` : synchronise le manifeste et l'index ;
- `SearchMasterCatalog` : recherche dans l'index local déjà synchronisé.
- `GetMasterCatalogGame` : récupère la fiche complète d'un jeu par son ID Amethys et ne télécharge que le shard concerné.

Commandes :
```sh
amethysctl catalog sync
amethysctl catalog search "hollow knight"
amethysctl catalog show 12345
```

La recherche est hors ligne. La première synchronisation nécessite une connexion HTTPS. Le cache est conservé dans `<AMETHYS_DATA_DIR>/catalog-cache` ou dans le dossier de données Amethys standard.

## Configuration

L'URL par défaut est :

`https://likounsee.github.io/Amethys-Catalog/`

Pour tester un autre hébergement statique compatible, définir `AMETHYS_CATALOG_URL` dans l'environnement du processus Core. L'URL doit être HTTPS et le serveur doit publier le même contrat de manifeste et de fichiers. Aucun identifiant GitHub ou secret IGDB n'est nécessaire côté client.

## Limites actuelles

- La recherche et les fiches détaillées sont exposées via Core IPC et CLI, et consommées par l'écran Store du prototype desktop. La validation du parcours graphique complet sur Windows et Linux reste à effectuer.
- Les métadonnées du catalogue distant ne sont pas fusionnées avec les installations locales.
- Une entrée du catalogue n'implique ni achat, ni propriété, ni installation, ni autorisation de téléchargement. L'absence d'offre connue ne supprime pas la fiche du catalogue.
