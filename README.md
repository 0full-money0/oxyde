# Horizon Mod Converter

Convertis tes mods **Minecraft Java** (Fabric, Quilt, Forge, NeoForge) en mods pour
**[Horizon](https://playhorizon.dev/)**, la version de Minecraft Java Edition 26.3 réécrite en Rust.
Regroupe-les ensuite en **modpack** à partager avec tes amis.

Gratuit, open source (MIT), un seul `.exe`, sans installation.

## Télécharger

👉 Va dans **[Releases](../../releases/latest)** et télécharge `HorizonModConverter.exe`.

L'app se met à jour toute seule : au démarrage elle regarde s'il y a une nouvelle version et te propose de l'installer en un clic.

## Utilisation

1. Lance `HorizonModConverter.exe`.
2. **Glisse tes `.jar`** dans la fenêtre (ou un dossier `mods` entier).
3. Clique sur **Convertir**.
4. Clique sur **Installer dans Horizon** (choisis une fois le dossier de jeu Horizon, celui qui contient `options.txt`).
   Les mods sont copiés dans `horizon-mods/` et activés.
5. Plusieurs mods ? Clique sur **Créer le modpack** : tu obtiens un fichier `.hzmods`.
   Envoie-le à tes amis : ils le glissent dans l'app et cliquent sur **Installer**.

## Ce qui est converti (et ce qui ne l'est pas)

Horizon **n'exécute pas le code Java** des mods ([doc officielle](https://playhorizon.dev/docs/mods)).
L'app récupère donc tout ce qui n'est pas du code et en fait un mod de contenu Horizon (`.hmod`) :

| Converti ✔ | Pas converti ✖ |
|---|---|
| Textures, modèles, sons, langues | Code Java (machines, interfaces, IA des mobs) |
| Recettes, tags, tables de loot, avancements | Mixins, événements |
| Structures, features et biome modifiers | Fichiers qui remplacent le vanilla |
| Blocs et objets (déclarés avec un modèle vanilla deviné d'après leur nom) | |

Détails techniques :

- manifeste `horizon-mod.toml` (API 0.2, capability `registries`, `side = "both"`) ;
- anciens dossiers (`recipes/`, `loot_tables/`, `tags/blocks/`…) renommés au format 1.21+/26.3 ;
- les fichiers hors des namespaces autorisés, les noms invalides et les fichiers > 16 Mio sont ignorés, avec la raison affichée ;
- `assets/<id>/items/*.json` ajoutés quand ils manquent ;
- blocs/objets : `data/<id>/horizon/block|item/*.json` avec un modèle vanilla (`_stairs` → escaliers, `_ore` → minerai, `_sword` → épée…). Décoche l'option dans **Réglages** pour ne pas les générer.

Les mods de **décoration, minerais, nourriture, structures** marchent le mieux.

## Format du modpack `.hzmods`

Un zip contenant `pack.json` (nom, version, liste des mods avec leur SHA-256) et `mods/<id>.hmod`.
Le format officiel `.hpack` de Horizon télécharge les mods depuis son registre, il ne peut donc pas contenir des mods convertis localement.
Tu peux aussi l'ouvrir comme un zip et copier `mods/*.hmod` dans `horizon-mods/` à la main.

## Sécurité

- L'`.exe` est **compilé par GitHub Actions** à partir de ce code ([workflow](.github/workflows/release.yml)), pas sur un PC.
- Chaque release publie le **SHA-256** de l'exe et une **attestation de provenance** GitHub.
  Pour vérifier : `gh attestation verify HorizonModConverter.exe --repo <propriétaire>/<dépôt>`.
- Pas de compresseur d'exe (UPX), pas d'installeur, pas de droits administrateur, pas de télémétrie.
- La mise à jour auto ne télécharge que depuis les releases de ce dépôt et vérifie le SHA-256 avant de remplacer l'exe.

### Signature de code

Les releases sont signées automatiquement via [SignPath Foundation](https://signpath.org/) (certificat gratuit pour
l'open source) dès que le projet y est accepté. Tant qu'un exe n'est pas signé, Windows SmartScreen peut afficher
« Windows a protégé votre ordinateur » : clique sur **Informations complémentaires → Exécuter quand même**.

Activer la signature (mainteneurs), une fois le projet accepté par SignPath :

1. Dans SignPath, crée le projet, une *signing policy* `release-signing` et un *trusted build system* « GitHub.com » lié au dépôt.
2. Sur GitHub, **Settings → Secrets and variables → Actions** :
   - secret `SIGNPATH_API_TOKEN` (token d'un utilisateur CI SignPath) ;
   - variables `SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG`, `SIGNPATH_POLICY_SLUG`.
3. La prochaine release est signée. Le workflow refuse de publier si la signature n'est pas valide.

### Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).

- Committers and reviewers: the members of this repository with write access.
- Approvers: the repository owner.
- Only binaries built by this repository's GitHub Actions workflow from its public source are signed.
- Privacy: this program does not transfer any information to other networked systems unless specifically requested
  by the user, except the update check, which reads this repository's latest GitHub release (no personal data is sent).

## Ligne de commande

```bash
HorizonModConverter.exe --convert <dossier-sortie> mod1.jar mod2.jar
```

## Compiler soi-même

```bash
cargo build --release
```

## Publier une nouvelle version (mainteneurs)

1. Change `version` dans `Cargo.toml` (ex. `1.0.1`) et commit.
2. `git tag v1.0.1 && git push origin main --tags`
3. GitHub compile et publie la release. Les apps déjà installées proposent la mise à jour.

## Licence

[MIT](LICENSE). Projet non officiel, sans lien avec Mojang, Microsoft ou l'équipe Horizon.
Les mods convertis restent soumis à la licence de leurs auteurs : vérifie-la avant de partager un modpack.
