# Import IGDB dans le catalogue maître Amethys

IGDB est la source principale de découverte et de métadonnées du catalogue mondial. Le dépôt Amethys reste l'autorité pour les IDs Amethys.

## Prérequis

1. Créer une application Twitch Developer de type Confidential.
2. Activer l'authentification à deux facteurs sur le compte Twitch.
3. Récupérer le Client ID et le Client Secret de l'application.
4. Installer Python 3.10+ (le script n'utilise que la bibliothèque standard).

IGDB utilise le flux OAuth2 `client_credentials` et les requêtes POST Apicalypse sur `https://api.igdb.com/v4`. Le navigateur desktop ne doit jamais appeler IGDB directement : les identifiants restent côté outil d'import/Core.

## Lancer l'import

PowerShell :

```powershell
$env:IGDB_CLIENT_ID = "..."
$env:IGDB_CLIENT_SECRET = "..."
python scripts/import_igdb_catalog.py
```

Linux/macOS :

```sh
export IGDB_CLIENT_ID="..."
export IGDB_CLIENT_SECRET="..."
python3 scripts/import_igdb_catalog.py
```

Le script récupère les jeux présents sur Windows, Linux et macOS (plateformes IGDB 6, 3 et 14), par pages de 500. Il conserve les IDs Amethys déjà attribués pour un même IGDB ID, attribue les nouveaux IDs dans le dépôt, et collecte les IDs externes Steam/Epic/GOG lorsque IGDB fournit une correspondance exploitable. Les IDs externes restent des chaînes.

Les doublons d'IDs Amethys ou de couples plateforme/ID externe font échouer la génération. Les offres sont des liens vers les pages des boutiques ; le script ne déclare aucun droit de téléchargement ou de lancement.

## Politique d'identité

- IGDB ID est une référence de source, pas un Amethys ID.
- L'Amethys ID est stable et ne doit jamais être réattribué à un autre jeu.
- Les imports locaux ne modifient jamais le catalogue maître.
- Les éditions/remakes/remasters restent des fiches IGDB séparées quand IGDB les distingue.
- Ne pas committer les secrets Twitch. Les stocker dans le gestionnaire de secrets local ou GitHub Actions.

## Limites connues à compléter

IGDB ne fournit pas nécessairement une correspondance de boutique pour chaque jeu. Les jeux sans ID externe restent dans le catalogue de métadonnées. Les sources de boutiques non prises en charge ne sont pas transformées en offres. Les développeurs/éditeurs et les types de jeu pourront être enrichis dans une évolution du schéma.


## Import automatisé depuis GitHub Actions

Le workflow manuel `.github/workflows/igdb-catalog-import.yml` lance le même import puis valide les IDs et pousse le catalogue généré sur `main`.

Configurer les deux secrets de dépôt suivants dans **Settings → Secrets and variables → Actions** :

- `IGDB_CLIENT_ID`
- `IGDB_CLIENT_SECRET`

Ensuite lancer **Actions → Import IGDB master catalogue → Run workflow**. Ne jamais placer le Client Secret dans le dépôt, dans un fichier de configuration versionné ou dans le chat.

Le premier import essaie de conserver les IDs des 30 fiches actuelles en rapprochant les identifiants externes déjà connus. Le registre `catalogs/amethys-id-registry.json` conserve ensuite les correspondances IGDB → Amethys, y compris les IDs connus qui ne sont plus présents dans le catalogue courant.


## Attribution et usage commercial

La FAQ IGDB indique que l'API est gratuite, y compris pour les usages commerciaux, mais demande aux produits commerciaux intégrant IGDB de passer par leur processus de partenariat et d'afficher une attribution visible à IGDB.com. Avant toute distribution commerciale d'Amethys intégrant ces données, contacter `partner@igdb.com` et confirmer les conditions applicables.
