# Registre central des identités de jeux — Amethys 0.3

## Objectif

Le catalogue doit être global et centralisé : l'Amethys ID d'un jeu est défini une seule fois dans le catalogue maître versionné du dépôt, puis distribué à tous les utilisateurs. Il n'est jamais attribué localement lors d'une découverte ou d'un import.

Le Store, la bibliothèque, les imports locaux et les intégrations de plateformes doivent tous parler du même jeu sans dépendre de son titre, qui peut varier selon la langue ou la plateforme.

Chaque jeu canonique reçoit deux familles d'identifiants distinctes :

| Champ | Type | Attribution | Exemple |
|---|---|---|---|
| `amethys_id` | entier SQLite positif (64 bits) | Amethys, automatiquement | `1`, `2`, `3` |
| `platform_id` | identifiant textuel de plateforme | Nom stable du fournisseur | `steam`, `epic-games`, `gog` |
| `external_game_id` | chaîne opaque | Plateforme concernée | `730`, `123456`, `game-slug` |

L'ID externe est volontairement une chaîne, même lorsque sa valeur est composée de chiffres : Steam utilise généralement des App IDs numériques, mais d'autres plateformes utilisent des UUID ou des slugs. Le convertir systématiquement en entier ferait perdre des identifiants valides et pourrait supprimer des zéros initiaux.

## Règles d'identité

1. `amethys_id` identifie le jeu dans tout l'écosystème Amethys. Il ne dépend jamais d'une plateforme.
2. Un jeu peut avoir plusieurs identités externes : une par plateforme, voire plusieurs offres distinctes si le modèle le justifie.
3. Une identité externe est unique pour un fournisseur : le couple `(platform_id, external_game_id)` ne doit pas désigner deux jeux différents.
4. Le titre, l'éditeur, le développeur, l'image ou la date de sortie ne sont jamais des clés d'identité.
5. Les IDs de plateformes sont conservés exactement comme fournis par leur source officielle ; aucune conversion numérique destructive.
6. Les IDs Amethys ne sont jamais réattribués après suppression d'une source de catalogue. Le registre canonique survit aux actualisations de flux.
7. Une offre ou une fiche importée ne prouve ni licence, ni propriété, ni droit de téléchargement.

## Stockage

La migration indépendante du cache catalogue ajoute `catalogue_game_id_registry` :

- `amethys_id INTEGER PRIMARY KEY AUTOINCREMENT` : identifiant numérique stable ;
- `catalogue_game_id TEXT UNIQUE` : clé interne existante du catalogue, conservée pour compatibilité ;
- clé étrangère vers `catalogue_games`.

Les offres existantes portent déjà `provider_id` et `external_id`. Elles représentent respectivement la plateforme et son identifiant externe. L'unicité du couple est vérifiée lors de la validation du catalogue et par la contrainte SQL.

La recherche renvoie désormais aussi `amethys_id`. Le dépôt expose la résolution depuis la clé catalogue et la recherche inverse depuis une identité externe.

## Exemple conceptuel

```text
Amethys ID: 1
Titre: Example Game
Plateformes:
  - platform_id: steam
    external_game_id: "730"
  - platform_id: gog
    external_game_id: "1234567890"
```

Cet exemple illustre uniquement le format. Il ne constitue pas une entrée commerciale réelle.

## Ce qui reste à alimenter

Le registre technique est prêt à recevoir les jeux. La liste commerciale doit être alimentée uniquement à partir de sources fiables/officielles et vérifiées : nom canonique, plateforme, ID externe, URL officielle et provenance. Aucun ID ne doit être deviné à partir d'un nom ou d'une recherche approximative.

Les connecteurs et la collecte à grande échelle des catalogues de plateformes restent des travaux distincts. Le Store doit consommer ces identités, jamais en recréer à partir des titres.


## Catalogue maître commun

La source de vérité globale est `catalogs/master-games-v1.json`. Elle est identique pour tous les utilisateurs et contient les Amethys IDs attribués explicitement par le projet. Le cache SQLite de chaque installation ne fait que répliquer cette source. Les jeux locaux non référencés restent des imports locaux et ne reçoivent pas automatiquement un ID global. Voir `doc/technical/MASTER-GAME-CATALOG.md`.
