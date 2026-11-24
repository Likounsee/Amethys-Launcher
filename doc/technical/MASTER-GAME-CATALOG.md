# Catalogue maître mondial des jeux Amethys

## Source de vérité

Le catalogue mondial est maintenu dans le dépôt dédié [Likounsee/Amethys-Catalog](https://github.com/Likounsee/Amethys-Catalog). Amethys-Launcher ne conserve aucune copie complète du catalogue ni registre local d'attribution des identifiants.

Le dépôt catalogue publie des fichiers statiques versionnés via GitHub Pages :
- `manifest.json` : version, compteurs, schémas, tailles et empreintes SHA-256 ;
- `index/games-index.json.zst` : index compact pour la recherche ;
- `shards/*.json.zst` : fiches détaillées et identités de boutiques chargées à la demande ;
- `metadata/genres.json` : métadonnées partagées.

Voir le [format publié](https://github.com/Likounsee/Amethys-Catalog#published-format) et la [documentation du dépôt catalogue](https://github.com/Likounsee/Amethys-Catalog/tree/main/doc).

## Identifiants et gouvernance

- Chaque jeu reçoit un Amethys ID global, positif, stable et attribué dans le dépôt central.
- Les IDs publiés ne sont jamais réattribués, même lorsqu'une fiche disparaît du catalogue courant.
- Les identifiants de boutiques sont des chaînes opaques et sont associés à un fournisseur explicite.
- Une identité de boutique ne doit jamais être devinée à partir du titre d'un jeu.
- Les éditions, remakes, remasters et DLC restent des fiches distinctes lorsque la source les distingue.

## Séparation avec la bibliothèque locale

Le catalogue maître décrit les jeux disponibles dans l'univers Amethys. Il ne représente ni les jeux possédés par l'utilisateur ni les jeux installés sur son PC.

La bibliothèque locale reste privée dans la base SQLite du Core. Elle conserve les installations détectées ou importées, leurs chemins, leurs exécutables et leur état. Les deux ensembles ne sont reliés qu'avec des identifiants stables vérifiés, jamais par le seul nom du jeu.

## Consommation par le launcher

Le Core utilise l'abstraction `CatalogProvider`. L'adaptateur GitHub Pages télécharge l'index, vérifie son SHA-256 et sa taille, puis récupère les fragments de détails à la demande. Le cache permet la recherche hors ligne. Un hébergement compatible différent pourra être utilisé sans réécrire la Store.

Le catalogue fournit des métadonnées et des liens de boutiques ; il n'accorde aucun droit de distribution, de téléchargement, de lancement, de licence ou de contournement des protections des plateformes.