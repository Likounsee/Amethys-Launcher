# Catalogue maître Amethys — dépôt et distribution

Le catalogue mondial de la Store est désormais maintenu dans le dépôt dédié
[Likounsee/Amethys-Catalog](https://github.com/Likounsee/Amethys-Catalog).

Son endpoint statique prévu est :

- Manifeste : `https://likounsee.github.io/Amethys-Catalog/manifest.json`
- Index de recherche : `https://likounsee.github.io/Amethys-Catalog/index/games-index.json.zst`

Le format publié est décrit dans le [README du dépôt catalogue](https://github.com/Likounsee/Amethys-Catalog#published-format) et dans sa documentation
[IGDB-CATALOG.md](https://github.com/Likounsee/Amethys-Catalog/blob/main/doc/IGDB-CATALOG.md).

## Séparation des données

Le catalogue global contient les fiches maîtres des jeux PC Windows et leurs identités de boutiques. Il ne signifie ni possession ni installation.

La bibliothèque locale est conservée dans la base privée du Core de chaque PC. Elle ne contient que les installations détectées, importées ou gérées sur cette machine, leurs chemins et leur état. Les deux ensembles restent séparés et ne sont rapprochés que par des identifiants stables vérifiés, jamais par le titre seul.

## Distribution et cache

Le catalogue est distribué sous forme de fichiers statiques versionnés :

- `manifest.json` : version, compteurs, schéma, checksums et liste des fragments ;
- `index/games-index.json.zst` : index compact ID/nom/slug pour la recherche hors ligne ;
- `shards/*.json.zst` : détails et identités de boutiques chargés à la demande ;
- `metadata/genres.json` : liste commune des genres.

Le launcher doit télécharger l'index compact, puis uniquement les fragments de détails nécessaires. Chaque fichier est vérifié par SHA-256 et placé dans un cache local borné. La base SQLite complète utilisée pendant la génération reste temporaire dans le runner ; elle n'est pas publiée ni téléchargée par les utilisateurs.

Le Core consommera un contrat de fournisseur abstrait. GitHub Pages est le premier adaptateur statique ; un hébergement Amethys pourra être ajouté ultérieurement sans réécrire le Store.

## Génération

Le pipeline et les secrets IGDB résident désormais dans le dépôt catalogue. Configurer `IGDB_CLIENT_ID` et `IGDB_CLIENT_SECRET` dans les Actions secrets de ce dépôt, puis activer GitHub Pages avec la source **GitHub Actions**. Le workflow s'exécute chaque lundi à 04:17 UTC et peut être déclenché manuellement.

Le registre d'identifiants Amethys est initialisé depuis l'ancien dépôt launcher lors du premier build, puis conservé et mis à jour dans le dépôt catalogue. Ne jamais supprimer ce registre : les IDs publiés sont permanents et ne sont jamais réutilisés.

La génération respecte les conditions IGDB applicables, notamment les obligations d'attribution et de partenariat avant toute distribution commerciale.
