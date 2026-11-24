# Amethys Launcher 0.2 – nouvelle interface (guide pour l'autre IA)

Chaque fichier de ce zip garde le MÊME chemin relatif que dans le dépôt `Likounsee/amethys-launcher`.
Copier par-dessus les fichiers existants (racine du dépôt) :

| Fichier du zip | Rôle |
|---|---|
| prototypes/ghostengine-amethys-preview/src/gfx.rs | Design tokens, icônes, peintre (rectangles arrondis/ombres rapides), jaquettes dessinées par code |
| prototypes/ghostengine-amethys-preview/src/pages.rs | Pages : sidebar, bibliothèque, fiche jeu, boutique, paramètres, dialogue de retrait |
| prototypes/ghostengine-amethys-preview/src/app.rs | État, animations (survol, fondu, scroll, toasts), clavier, actions Core |
| prototypes/ghostengine-amethys-preview/src/status.rs | NOUVEAU : statuts des jeux calculés une fois par chargement |
| prototypes/ghostengine-amethys-preview/src/lib.rs | Ajoute `pub mod status;` |
| prototypes/ghostengine-amethys-preview/src/window_main.rs | Fenêtre, `--snapshot DIR`, `--size WxH`, boucle `run_animated` |
| vendor/ghostengine/src/lib.rs + native_window.rs | Patch moteur ADDITIF : `FrameOutcome`, `run_animated` (animations) |

Les previews sont dans `previews/` (PNG générés uniquement pour visualiser).

## Rendu : aucun PNG dans le logiciel
Toute l'interface (cartes, icônes, ombres, dégradés, texte) est dessinée en direct par GhostEngine.
Les jaquettes par défaut sont générées par code (couleur issue du nom + initiale). Seules les vraies
jaquettes choisies par l'utilisateur (dossier média du Core) sont chargées comme images.

## Contraintes respectées
Rust conservé, connexion réelle au Core (agent/IPC), aucune donnée inventée, `src/data.rs` n'est plus utilisé par l'UI.

## Vérifier
cargo build --release --manifest-path prototypes/ghostengine-amethys-preview/Cargo.toml --bins
cargo test --release --manifest-path prototypes/ghostengine-amethys-preview/Cargo.toml
Aperçu sans fenêtre : amethys-launcher --snapshot ./out [--size 1100x700]
(AMETHYS_DATA_DIR doit être un dossier en mode 0700.)
