# Catalogue PC SQLite distribué indépendamment du launcher

## Objectif

Le catalogue global distribué aux clients Amethys est un snapshot SQLite public, généré côté GitHub Actions à partir d'IGDB. Aucun appel IGDB ni secret Twitch n'est effectué depuis le launcher. Le catalogue est publié sur **GitHub Pages**, indépendamment des Releases et versions du launcher.

Les URLs stables du site sont :

- Manifeste : `https://likounsee.github.io/Amethys-Launcher/catalog-manifest.json`
- Base compressée : `https://likounsee.github.io/Amethys-Launcher/catalog.db.zst`

Le workflow remplace le contenu publié à chaque génération. Il ne crée aucune Release et ne pousse pas le binaire de la base dans l'historique Git de `main`.

## Séparation stricte Store / bibliothèque locale

`catalog.db.zst` est le catalogue Store global commun à tous les utilisateurs. Il ne contient que les fiches maîtres et leurs identités de boutiques. Il ne contient ni jeux installés, ni chemins, ni état de téléchargement, ni compte/possession. Un jeu présent dans `games` n’est donc pas réputé possédé ou installé.

La bibliothèque locale est stockée dans la base privée du Core de chaque PC (`games` / `local_game_locations` / `local_game_metadata`). Elle ne liste que les installations détectées, importées ou gérées sur ce PC. Une installation locale peut ne correspondre à aucun jeu du catalogue et conserver alors un `amethys_id` nul. La correspondance ne se fait que par une identité externe vérifiée ou une validation explicite, jamais par le titre seul. Désinstaller un jeu retire son état d’installation local, pas sa fiche du catalogue Store.

## Schéma SQLite

- `games` : une fiche par jeu IGDB Windows PC principal, avec l'ID Amethys stable, le nom, le slug, le résumé, la date de sortie, l'URL de jaquette et les genres.
- `game_stores` : zéro à plusieurs identités de boutiques par jeu. `external_id` est toujours TEXT. La contrainte `UNIQUE(store, external_id)` empêche qu'un même identifiant de boutique associe deux jeux différents.
- `catalog_meta` : version du schéma, filtres, date de génération et compteurs.

Les index couvrent la recherche de jeux par nom/slug, la résolution d'ID Amethys et la correspondance exacte boutique + identifiant externe.

## Filtrage et déduplication

Le générateur utilise IGDB `games` avec `category = 0` et `platforms = (6)` (PC Windows). La pagination est faite par curseur `id > last_id`, pas avec un offset croissant, afin de dépasser les limites d'offset. Les sources de boutiques reconnues sont Steam, GOG, Epic, EA, Ubisoft, Battle.net, Amazon Games et Humble. itch.io et les sources inconnues sont ignorés. Les identifiants externes ne sont jamais convertis en nombres.

Les IDs Amethys existants sont conservés depuis le registre du dépôt et les identités déjà présentes dans le catalogue maître. Les nouveaux IDs sont attribués par le job central, jamais à partir de l’ordre d’import local. Les IDs déjà connus restent réservés et ne sont jamais réutilisés. Aucun scan ou import local ne peut attribuer un `amethys_id`.

Une identité de boutique déjà attribuée à une autre fiche IGDB est ignorée et comptabilisée plutôt que de créer une association ambiguë.

## Génération manuelle

```powershell
$env:IGDB_CLIENT_ID = "..."
$env:IGDB_CLIENT_SECRET = "..."
python -m pip install zstandard==0.25.0
python scripts/build_igdb_catalog_db.py
```

Le script produit `catalog.db`, `catalog.db.zst`, `catalog-manifest.json` et met à jour `catalogs/amethys-id-registry.json`. Le manifeste contient notamment le nombre de jeux, la date/version du catalogue, les tailles et les SHA-256 de la base compressée et de la base SQLite. Le script valide la base avec `PRAGMA integrity_check`.

## Publication automatique avec GitHub Actions

Le workflow `.github/workflows/build-igdb-catalog.yml` s'exécute chaque lundi à 04:17 UTC et peut aussi être lancé manuellement. Configurer les secrets de dépôt `IGDB_CLIENT_ID` et `IGDB_CLIENT_SECRET` dans **Settings → Secrets and variables → Actions**.

La première activation nécessite de sélectionner **Settings → Pages → Build and deployment → Source: GitHub Actions**. Ensuite, le workflow construit la base, met à jour le registre d'identifiants sur `main`, puis déploie uniquement les deux fichiers de catalogue sur GitHub Pages. Il ne crée pas de Release, ne modifie pas la version du launcher et ne stocke pas la base dans l'historique Git.

## Côté client

Le client Rust devra télécharger d'abord `catalog-manifest.json` depuis l'URL stable, comparer `catalog_version` avec la version locale, puis récupérer `database.file` si le catalogue est plus récent. Il doit vérifier `compressed_sha256` avant décompression, puis `uncompressed_sha256` après décompression, vérifier SQLite en lecture seule et remplacer le fichier local de façon atomique (fichier temporaire puis renommage). Les recherches et associations locales se feront ensuite dans cette base via `rusqlite`; les jaquettes seront téléchargées séparément à la demande.

Ce pipeline assure la génération et la publication du catalogue. Le téléchargement, la mise à jour atomique et le cache de jaquettes côté Rust constituent l'étape d'intégration client suivante.

## Attribution IGDB

Avant toute distribution commerciale, respecter les conditions IGDB applicables, notamment l'attribution visible et les éventuelles obligations de partenariat.
