# Polyhedron Studio

**Moteur de jeu voxel et studio de création intégré, écrit en Rust.**
On construit le jeu *dans* le jeu : blocs, mobs, objets, biomes, structures, scripts, interface, sons et règles se créent dans l'éditeur, puis se testent immédiatement avec **▶ Play (F7)**.

![Rust 2024](https://img.shields.io/badge/Rust-2024-orange) ![Bevy 0.20](https://img.shields.io/badge/Bevy-0.20.1-blue) ![Windows](https://img.shields.io/badge/plateforme-Windows-lightgrey)

> Règle de conception : **zéro fichier externe**. Rien ne s'importe, tout se fabrique dans l'éditeur et se sauvegarde en fichiers `.ron` lisibles dans `project/`.

---

## Sommaire

- [Démarrage rapide](#démarrage-rapide)
- [Le studio](#le-studio)
- [Le moteur](#le-moteur)
- [Le jeu](#le-jeu)
- [Contrôles](#contrôles)
- [Organisation du dépôt](#organisation-du-dépôt)
- [Limites connues](#limites-connues)

---

## Démarrage rapide

### Télécharger l'exécutable

Prends `PolyhedronStudio.exe` dans les **Releases** GitHub et place-le à côté du dossier `project/` du dépôt : le studio lit ses données dans le dossier où il est lancé.

### Compiler depuis les sources

Prérequis : [Rust](https://rustup.rs) 1.97.1 ou plus récent. Le **Windows SDK** est facultatif : il sert à intégrer l'icône dans l'exécutable (`build.rs`) ; sans lui la compilation continue avec un simple avertissement.

```bash
git clone https://github.com/Batpolygon/polyhedron-studio.git
cd polyhedron-studio
cargo run --release
```

`check.bat` et `test.bat` lancent `cargo check` et `cargo test` et écrivent le résultat dans `_check.log` / `_test.log`.

---

## Le studio

L'interface est découpée en **espaces de travail** (rail d'icônes à gauche, chacun avec sa disposition de panneaux ancrables) :

| Espace | Outil | Ce qu'on y fait |
|---|---|---|
| 🌍 **World** | Éditeur de monde | Poser / casser des blocs, palette filtrable, génération procédurale, console |
| 🧱 **Blocks** | **Forge** | Blocs multi-boîtes (16×16×16, magnétisme 1/16), couleurs par face, mouchetures lumineuses, parties animées (rotation, flottement, pulsation, mode *Always / Powered / Motor*), facettes Voronoi, vent, ports de câbles, règle de placement |
| 🐾 **Mobs** | **Sculptor** | Rigs en boîtes, squelette, gizmos (G déplacer, R pivoter, S taille, P pivot), timeline d'animation à clés, Glow par boîte, stats, comportements, biomes, butin, cerveaux (`Brain`) |
| 🗡 **Items** | **Armoury** | Objets en boîtes colorées : outils, armes, armures, nourriture, combos et effets |
| 🧩 **Scripts** | **Weave** | Scripts visuels (voir ci-dessous) |
| 🌲 **Biomes** | **Terraformer** | Aperçu 3D du biome seul, reliefs par bruit (`Landform`), palettes, filons, grottes, couches de roche, montagnes, carte 2D par biome / hauteur / bloc |
| 🏛 **Structures** | Atelier de structures | Poser, effacer, ancres, pipette ; variantes pondérées par biome |
| 🖼 **Interface** | **Showcase** | HUD et menus : arbre de widgets, ancres, bindings, écrans Overlay / Modal / Death / Pause |
| 🎨 **Shader** | Shader | Paramètres du rendu toon, presets de shader |
| 🔊 **Ambiance** | Ambiance | Sons par oscillateurs, musique en grille de pas par biome et moment de la journée, particules, ciel, nuages, météo |
| 📜 **Story** | Histoire | Quêtes, dialogues, succès |
| ⚙ **Rules** | Règles | Toutes les valeurs du jeu (`game_rules.ron`) et l'éditeur de crafting complet |

### Fonctions communes

- **Save all** (Ctrl+S) : écrit monde, blocs, items, mobs, biomes, scripts, écrans, structures, règles, crafting, sons, ambiance, histoire, câbles et réglages. Le bouton passe en orange quand quelque chose a changé.
- **Annuler / rétablir** (Ctrl+Z / Ctrl+Y) sur 200 étapes, pour le monde, les mobs, les objets, les blocs, les biomes, les structures, les scripts, l'interface, les règles et l'ambiance.
- **Multi-sélection** (Maj / Ctrl + clic) dans la Forge, l'Armoury et le Sculptor.
- **Projets** : créer, ouvrir, dupliquer, projet exemple, arbre avec recherche, renommage avec mise à jour des références, suppression sûre vers `.trash`, validation globale.
- **Export d'un jeu autonome** (sans le studio) dans `exports/`.
- **Play-In-Editor (F7)** : snapshot du monde en mémoire, partie lancée sur place, rollback complet à l'arrêt.

### Weave, les scripts visuels

- Programmes **globaux** ou **liés** à un bloc ou un mob (variables locales, `g.` pour partager).
- Événements : On Start, Tick, Interact, Hit, Signal, Energy, On Game Event.
- Blocs : maths, capteurs, questions au monde, listes, poser / casser un bloc, particules, sons, heure, faire apparaître un mob, déplacement, impulsion, tir, actions, santé, attaque, apprivoiser, signaux, écrans, sauvegarde, quêtes, dialogues.
- Débogage : Pause, Step, Breakpoint. Modèles de programmes fournis (*Mob Brain*, *Day Night*, *Signal Lamp*).

---

## Le moteur

### Voxels

- **9 formes** : Cube, demi-cube horizontal et vertical, Stairs, coins d'escalier intérieur / extérieur, Ramp, coins de rampe intérieur / extérieur, chacune en **24 orientations**.
- **Fusion sub-voxel** : deux demi-cubes complémentaires dans une même cellule, chacun avec son matériau ; masquage exact des faces cachées.
- **Blocs synthétiques personnalisables** (touche C) : couleur libre, teinte Voronoi, relief low-poly, couleur par partie de la forme.
- **Liquides** : écoulement, sources infinies, assèchement, surfaces inclinées ; seules les cellules modifiées sont simulées.

### Génération du monde

- Monde **infini** en chunks, chargement et déchargement en continu sur des threads de travail ; types Infini / Plat / Île.
- **16 biomes** choisis par climat (température / humidité) avec mélange des hauteurs : Plains, Great Forest, Desert, Swamp, Oasis, Burnt Forest, Volcano, Frozen Lake, Crater, Mnemo Plain, Mud, Ice Floe, Pyramid and Temple…
- **Montagnes** en bruit ridgé jusqu'à Y 200, roche et neige selon l'altitude et la pente.
- **Grottes** : cavernes géantes, tunnels, passages, lacs de lave et lacs souterrains, stalactites et stalagmites, lumière d'exploration qui suit le joueur.
- **Filons** de minerai réglables par monde et par biome.
- **59 structures** de biome et de caverne (arbres, pyramides, temples, champignons géants, cristaux, mines, camps, ruches Mnemo…).
- **Sauvegarde compacte** : seules les colonnes modifiées sont écrites, le reste revient de la seed (un monde intact pèse 84 octets).

### Rendu

- Shaders **WESL** : rendu toon, normal map Voronoi en facettes low-poly, contours soudés aux coins.
- **Vent** sur le feuillage et **vagues** sur les liquides.
- **Ciel** procédural : dégradé, soleil carré, lune en pixels, environ 900 étoiles, nuages cubiques, cycle jour / nuit, météo.
- **Icônes 3D** de tous les blocs, objets, œufs et ressources, rendues par le moteur.

### Physique

- **Rapier 3D** : un collider trimesh par chunk, généré depuis le maillage.
- **Movables** : des constructions de blocs deviennent des corps physiques (lancées avec E sur un bloc Movable), poussables, avec flottaison dans les liquides.
- **Moteurs et roues** : un moteur fait tourner les blocs posés sur son moyeu (liaison pivot), pousse ou soulève un movable, et se réaligne sur la grille à l'arrêt.
- **Véhicules** : siège Wagon, pilotage à la Remote Control.

### Audio

- Synthétiseur **DSP en pur Rust** sur thread dédié, sans aucun fichier audio : pas, minage, lasers, moteurs, interface, vent, musique procédurale.

---

## Le jeu

### Modes

**Créatif** (vol, cassage instantané, inventaire complet, mobs pacifiques), **Survie** (vie, endurance, ressources, crafting) et **Spectateur** (vol sans collision).

### Ressources et crafting

20 ressources organisées en familles :

| Famille | Ressources |
|---|---|
| Naturelles | Floral, Mineral, Crystalline, Faunal, Ethereal, Astral, Mnemonical |
| Énergie | Voltic |
| Craftées | Synthetic, Technic, Conflux, Garbage, Atomic |
| Infusions | Celliom, Aetech, Voltiom, Refractal, Fluxiom, Refreatech |
| Fin de jeu | Omnicore |

Stations : **Synthesizer**, **Assembler**, **Infuser** (une infusion ratée donne du Garbage), **Atomiser**. Les blocs récoltés se convertissent en ressource pure. Toutes les recettes sont éditables dans l'espace Rules.

### Machines et câbles

- Plus de 40 machines : Solaronel, Kineticster, Combustor, Energiser, Omnireactor, Container, Excavator, Laser, Repulsor, Terminal, Comandor, Transmitter, Screen, Emitter / Receiver, Reconstructor, Etheraltar, Televerseur, Mnemo-Digestor, Mnemo-Cloner, Astral-Container…
- **Câbles à ports typés** : Énergie (carré), Ressource (triangle), Signal (hexagone) ; entrées vertes, sorties rouges. Les câbles Signal portent des valeurs Booléen / Float / String.
- **Réseau Omnicore** par monde, alimenté par l'Omnichorus.
- Panneau de réglages par machine (touche E), bouton ON/OFF.

### Objets, armes et armures

Sword, Pickaxe, Axe, Gun, Brush, Wrench, Wire, Remote Control, Binder, Atomixer, EtheraGun, BlastyGun, VoltiGun, AstralSaber ; armures MineralCoat, TechnicArmor (saut), RefreatechArmor (vol), AstralRevetment, Mnemo-Symbiote, Etherealink. Combat à **combo** avec hitmarker, parade et recul.

### Créatures

**21 entités** avec comportement, biomes, cycle jour / nuit et butin. Certaines se chevauchent (Galopin), d'autres construisent leur nid, se reproduisent ou explosent (Grom). Œufs d'apparition cubiques aux couleurs du mob.

### Inventaire et interface

- 30 cases et la barre rapide, piles de 999, glisser-déposer partout, onglets Blocks / Machines / Items & tools / Mobs / Resources / Inventory / Craft.
- Conteneurs de 27 cases.
- HUD : cœurs, endurance, réticule avec bloc ou créature visé, actions possibles, minicarte, aperçu 3D du joueur.
- **Chat et 40 commandes** (`T` ou `/`) avec complétion par Tab, sélecteurs `@p` `@s` `@n` et coordonnées relatives `~`.
- **Personnalisation** du personnage boîte par boîte, presets, import / export.
- Menu principal avec panorama procédural, création de mondes (nom, graine, type, mode, biome unique, niveau de la mer), paramètres (sensibilité, FOV, distance d'affichage, volumes, **touches réassignables**), support manette.

---

## Contrôles

Tous réassignables dans *Settings → Controls*.

| Touche | Action |
|---|---|
| ZQSD / WASD | Se déplacer |
| Espace | Sauter, nager ; double Espace = voler (créatif) |
| Maj | Sprint, descendre |
| Ctrl | Dash, vol rapide |
| Clic gauche | Miner, attaquer, pousser |
| Clic droit | Poser, parer, manger, équiper une armure |
| E | Interagir (machines, sièges, movables) |
| Tab / I | Inventaire |
| 1-9 / molette | Barre rapide |
| G | Jeter l'objet (Maj : la pile) |
| C | Personnaliser le bloc synthétique tenu |
| F5 | Vue 1re / 3e personne |
| T, / | Chat, commande |
| Échap | Pause |
| F7 | Lancer / arrêter la partie depuis le studio |

---

## Organisation du dépôt

```
src/
  core/        projets, sauvegarde, état de l'application, Play-In-Editor
  editor/      studio : espaces de travail, panneaux, undo, Save all
  voxel/       formes, maillage, chunks, génération, biomes, structures, liquides
  renderer/    shaders, ciel, icônes
  physics/     contrôleur du joueur, movables, moteurs
  gameplay/    combat, minage, crafting, machines, câbles, mobs, commandes
  scripting/   Weave : AST, machine virtuelle, bibliothèque de blocs
  ui/          HUD et menus (Showcase)
  audio/       synthétiseur DSP
assets/        icône de l'application et police du HUD (intégrées à l'exe)
project/       toutes les données du jeu en .ron (blocs, items, mobs, biomes,
               structures, scripts, shaders, interface, règles, mondes)
tools/         scripts Python qui ont généré les designs de base
               (blocs, mobs, items, joueur, structures, icône, police)
build.rs       intègre l'icône dans l'exécutable Windows
```

Les scripts `tools/design_*.py` réécrivent les fichiers qu'ils listent : les relancer écrase les retouches faites dans le studio sur ces éléments.

**Pile technique** : Rust 2024 · Bevy 0.20.1 · bevy_egui 0.43 · egui_dock 0.21 · bevy_rapier3d 0.37 · rodio 0.23 · ron / serde · noise.

---

## Limites connues

- Certaines parties restent dans le code du moteur, sans équivalent éditable : movables, véhicules, moteurs, physique des câbles (réglages exposés), interface de l'éditeur, menu principal, panneaux de machines.
- Une partie des machines (Reconstructor, Combustor, Omnireactor, Repulsor, Ringer, Mnemo-Digestor…) sont des fonctions du moteur ; leurs ports, recettes et réglages sont des données.
- Les mobs ne nagent pas encore (ils marchent au fond de l'eau).
- Le vent et les vagues ne déplacent pas les ombres.
- Ouvrir, renommer ou supprimer un projet redémarre l'application : enregistre d'abord.

---

## Licence

MIT OR Apache-2.0.

*By Batpolygon.*
