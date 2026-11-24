# Catalogue maître mondial des jeux Amethys

## Règle fondamentale

Le catalogue maître est une donnée versionnée du projet Amethys, identique pour tous les utilisateurs. Il ne doit jamais être construit à partir des jeux installés localement, de l'ordre de découverte sur un PC ou des comptes Steam/Epic/GOG d'un utilisateur.

Un jeu reçoit un identifiant Amethys permanent, attribué dans le dépôt central. Le même jeu conserve cet identifiant sur tous les appareils et pour tous les utilisateurs.

## Identifiants

- `amethys_id` : entier positif, global, attribué une seule fois par Amethys.
- `platform_id` : identifiant textuel stable de la plateforme (`steam`, `epic-games`, `gog`, etc.).
- `external_game_id` : valeur opaque fournie par la plateforme. Elle reste une chaîne, même si elle ne contient que des chiffres.

Le couple plateforme + identifiant externe relie un jeu de plateforme à un jeu Amethys. Un jeu Amethys peut avoir plusieurs liens externes.

## Source de vérité et synchronisation

Le fichier `catalogs/master-games-v1.json` est la source versionnée du catalogue maître. Il est distribué avec l'application et pourra être mis à jour via des versions de catalogue signées. Le cache SQLite local est une copie consultable, jamais l'autorité d'attribution des IDs.

Les utilisateurs peuvent ajouter des jeux locaux à leur bibliothèque, mais cela ne crée pas automatiquement une nouvelle entrée dans le catalogue mondial. Les propositions d'ajout doivent être examinées et fusionnées dans le catalogue central avant attribution d'un nouvel ID.

## Attribution et gouvernance

- Les IDs sont attribués explicitement dans le dépôt, de manière déterministe et sans réutilisation.
- Une suppression ou un retrait du Store ne libère jamais un ID.
- Les titres alternatifs, éditions, remasters, bundles et DLC sont modélisés explicitement ; ils ne sont pas fusionnés par simple ressemblance de nom.
- Chaque lien externe doit conserver sa provenance et être vérifié depuis une source officielle ou fiable.
- Aucun ID externe n'est deviné ou fabriqué.
- Les doublons de plateforme + ID externe sont interdits.

## Format

```json
{
  "schema_version": 1,
  "catalog_id": "amethys.global-games",
  "catalog_version": 1,
  "games": [
    {
      "amethys_id": 1,
      "canonical_name": "Nom du jeu",
      "platform_ids": [
        { "platform_id": "steam", "external_game_id": "12345" },
        { "platform_id": "gog", "external_game_id": "987654" }
      ]
    }
  ]
}
```

L'exemple est illustratif, pas une fiche de jeu réelle. Le fichier initial reste vide tant que les données officielles n'ont pas été collectées et vérifiées.

## Ordre de travail avant le Store

1. Définir le schéma et les règles de validation du catalogue maître.
2. Constituer la liste de jeux et attribuer leurs IDs Amethys dans le dépôt.
3. Rattacher les IDs des plateformes, avec provenance vérifiée.
4. Détecter les doublons et les éditions distinctes.
5. Importer/synchroniser le catalogue maître dans le cache Core.
6. Faire consommer ce catalogue par le Store ; ne jamais y générer d'IDs.
