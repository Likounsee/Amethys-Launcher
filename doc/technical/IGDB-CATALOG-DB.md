# Catalogue PC SQLite distribué par Release

## Objectif

Le catalogue global distribué aux clients Amethys est un snapshot SQLite public, généré côté GitHub Actions à partir d'IGDB. Aucun appel IGDB ni secret Twitch n'est effectué depuis le launcher. Le fichier publié est `catalog.db.zst`, asset de la dernière GitHub Release du dépôt.

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

Le script produit `catalog.db`, `catalog.db.zst` et met à jour `catalogs/amethys-id-registry.json`. Il valide la base avec `PRAGMA integrity_check`.

## GitHub Actions

Le workflow `.github/workflows/build-igdb-catalog.yml` s'exécute chaque lundi à 04:17 UTC et peut aussi être lancé manuellement. Configurer les secrets de dépôt `IGDB_CLIENT_ID` et `IGDB_CLIENT_SECRET` dans **Settings → Secrets and variables → Actions**.

Le job met à jour l'asset `catalog.db.zst` sur la dernière Release existante avec `gh release upload --clobber`. Il ne crée pas une nouvelle version du launcher chaque semaine. Le workflow nécessite une Release existante et les permissions `contents: write`. Le registre des IDs est ensuite committé sur `main` afin que les prochains imports conservent les identités.

## Côté client

Le client Rust devra interroger l'API GitHub Releases (métadonnées de la dernière Release), comparer la version/date du catalogue avec celle stockée localement, télécharger l'asset HTTPS si plus récent, vérifier son SHA-256 publié/calculé, puis décompresser dans le répertoire de données Amethys sous %AppData%. Le remplacement doit être atomique : décompresser dans un fichier temporaire, vérifier SQLite en lecture seule, puis renommer. Les recherches et associations locales se feront ensuite dans cette base via `rusqlite`; les jaquettes seront téléchargées séparément à la demande.

Ce document décrit le format et le pipeline de génération. Le téléchargement, la mise à jour atomique et le cache de jaquettes côté Rust constituent l'étape d'intégration client suivante.

## Attribution IGDB

Avant toute distribution commerciale, respecter les conditions IGDB applicables, notamment l'attribution visible et les éventuelles obligations de partenariat.
