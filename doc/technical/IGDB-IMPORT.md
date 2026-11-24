# Import IGDB dans le catalogue maître Amethys

Le catalogue maître et son pipeline IGDB sont désormais maintenus dans le dépôt dédié [Amethys-Catalog](https://github.com/Likounsee/Amethys-Catalog), et non dans Amethys-Launcher.

## Où gérer l'import

- Dépôt : [Likounsee/Amethys-Catalog](https://github.com/Likounsee/Amethys-Catalog)
- Génération et publication : [workflows GitHub Actions](https://github.com/Likounsee/Amethys-Catalog/actions)
- Format publié : [documentation du catalogue](https://github.com/Likounsee/Amethys-Catalog#published-format)

Les secrets `IGDB_CLIENT_ID` et `IGDB_CLIENT_SECRET` doivent être configurés uniquement dans les paramètres Actions du dépôt Amethys-Catalog. Le workflow de ce dépôt génère les données, conserve le registre central des identifiants Amethys et publie les index et fragments statiques sur GitHub Pages.

## Règles d'identité

- L'IGDB ID identifie une fiche dans la source ; il ne remplace pas l'identifiant Amethys.
- Chaque Amethys ID est attribué de manière centrale, reste stable et n'est jamais réutilisé.
- Les identifiants externes des boutiques sont conservés comme chaînes.
- Une installation locale ne crée et ne modifie jamais une fiche du catalogue mondial.
- Une offre de catalogue ne prouve ni la possession d'une licence ni l'autorisation de télécharger ou d'installer un jeu.

## Côté Amethys-Launcher

Le launcher ne contient plus de script d'import IGDB ni de copie du registre global. Il consomme le catalogue publié à travers son abstraction `CatalogProvider`, vérifie les tailles et SHA-256, puis met en cache l'index et les fragments nécessaires. Cette séparation évite de dupliquer la génération et les identifiants entre les deux dépôts.

Ne placez jamais de secret IGDB dans Amethys-Launcher, dans un fichier versionné ou dans le chat.

## Attribution et usage commercial

Avant toute distribution commerciale intégrant des données IGDB, vérifier les conditions et obligations d'attribution directement auprès d'IGDB.