# Polyhedron Studio — état du projet (07/11/2026)

Rust 2024 · Bevy 0.20.1 · bevy_egui 0.43 · egui_dock 0.21 · bevy_rapier3d 0.37 · rodio 0.23 · ~33 000 lignes.
`cargo check` passe sans erreur ni avertissement (07/11), 166 tests OK. `check.bat` / `test.bat` à la racine lancent la vérification et les tests.

## Feuille de route

| Étape | Statut | Où |
|---|---|---|
| 1. Crate, plugins, dock egui_dock | ✅ (existant) | `main.rs`, `editor/` |
| 2. Shaders WESL toon / Voronoi / outline | ✅ + densité Voronoi par bloc | `renderer/` |
| 3. Voxels, 9 formes, fusion sub-voxel, trimesh Rapier | ✅ | `voxel/shapes.rs`, `mesher.rs`, `world.rs` |
| 4. PIE (F7) snapshot / rollback, caméra, joueur | ✅ | `core/pie_manager.rs`, `physics/player_controller.rs` |
| 5. Éditeur de mobs & actions | ✅ (existant) + blend-tree en jeu | `gameplay/mobs/` |
| 6. Trame (scripts visuels) : AST, VM, événements, blocs monde | ✅ | `scripting/` |
| 7. Biomes, génération, streaming, structures | ✅ | `voxel/biomes.rs`, `world_gen.rs`, `chunk_manager.rs`, `structures.rs` |
| 8. Vitrine (HUD / menus) | ✅ | `ui/`, `editor/views/umg_designer.rs` |
| 9. Movables, véhicules, énergie, câbles (ports typés), machines | ✅ | `physics/movable_compound.rs`, `gameplay/` |
| 10. Combat, minage, audio DSP | ✅ | `gameplay/combat.rs`, `mining.rs`, `audio/` |

## Nouveautés du 07/11 (génération, cavernes, Mnemo)

- **Montagnes** (`MountainSettings`) : chaînes en bruit ridgé jusqu'à Y 200 environ, roche et neige selon l'altitude et la pente, crêtes irrégulières (`crag`). Réglage `peaks` par biome. Panneau Biomes, section Mountains.
- **Cavernes** : grandes salles de 60 à 100 blocs de large, colonnes, stalactites, stalagmites, lacs souterrains. Bedrock à -96.
- **Lumière de caverne** : `explorer_light`, `explorer_range`, `explorer_color` dans `CaveSettings`. Une lumière suit le joueur sous la surface (`cave_light`, `gameplay/world_rules.rs`). Elle n'éclaire que près du joueur et ne projette pas d'ombre.
- **Filons** : `OreVein.width` > 0 donne un filon long, `vein_scale` règle l'espacement.
- **Structures de caverne** (`CaveStructureRule` dans `_world.ron`) : champignons géants, cristaux, mines, camps, sanctuaires, os, ruches Mnemo. Elles doivent tenir dans leur colonne de chunk.
- **Structures de biomes** : `tools/design_structures.py` écrit 59 structures, 9 blocs neufs et les règles de biomes. Le relancer écrase les retouches faites dans le studio sur ce qu'il liste. Ancienne version : `_sauvegarde_2026-11-07/generation/`.
- **Mnemo rouge et vivant** : blocs, mobs, items et armure passés au rouge, sol de Mnemo Plain en `mnemo_rot`, balancement par `wind` sur `mnemo_rot` et `mnemonical_ore`, bloc `mnemo_heart` animé.

## Nouveautés du 07/11 (mondes, Polyhedron Studio)

- **Monde écrasé par le démo** : à l'arrêt d'une partie, le panorama ou le monde du studio pouvait reprendre le monde avant la sauvegarde. Corrigé : `panorama` tourne après `PieSet::Transition` et attend la fin de l'arrêt.
- **Sauvegarde d'un monde infini** : seules les colonnes modifiées sont écrites, y compris celles déchargées en mémoire ; le reste revient de la seed. Un monde intact pèse 84 octets au lieu de 211 Mo.
- **Application renommée Polyhedron Studio** : exécutable `target/debug/polyhedron-studio.exe`, titre de fenêtre, À propos.
- **Icône cube** : dessinée par `tools/design_icon.py` (`assets/icon/`), affichée sur la fenêtre (`src/main.rs`) et intégrée à l'exécutable par `build.rs` (rc.exe du SDK Windows, simple avertissement s'il manque).

## Nouveautés du 07/11 (Save all, joueur, items, menus)

- **Save all** (`src/editor/save_all.rs`) : bouton de la barre du haut et Ctrl+S. Écrit le monde, les blocs (brouillon de la Forge compris), items, mobs, biomes, scripts, écrans, structures, règles, crafting, sons, atmosphère, histoire, câbles, réglages, panneaux, preset et source du shader. Le bouton passe en orange quand quelque chose a changé, et la bulle liste quoi. Refusé pendant Play.
- **Icônes** : la cause des carrés était la police Hack, seule à avoir certains symboles alors que l'interface n'utilise pas Hack. Symboles remplacés par des emoji, Hack ajouté en dernier recours, et `tools/check_glyphs.py` contrôle maintenant la vraie pile de polices, données du projet comprises.
- **Joueur** (`tools/design_player.py`) : nouveau squelette (hanches, avant-bras, tibias, pouce, doigts en vue FPS), clips refaits avec Run, Jump, Fall, Land. Correction du bug principal : les bras partaient derrière le corps. Les passages entre clips sont lissés, la marche suit la distance, marche arrière et pas de côté gérés.
- **Items** (`tools/design_items.py`) : les 20 modèles refaits, de 12 à 34 boîtes selon l'item.
- **Menus** : rail d'icônes à gauche par familles, barre du haut réduite, menu ☰ en sous-menus, explications dans une icône ℹ au survol. Monde : la console passe en onglet, la vue 3D gagne de la place.
- Les nouveaux projets partent des blocs, entités, items, biomes et structures de `project/`.
- Limites : le bouton Save all ne s'éteint pas pour une sauvegarde faite depuis le bouton d'un panneau (règles, crafting, ambiance, histoire, blocs) ; il s'éteint au Save all suivant. Save all réécrit tous les fichiers de blocs avec leurs valeurs complètes.

## Nouveautés du 07/11 (éditeur complet, phases 1 à 7)

Règle : **zéro fichier externe**. Tout se crée dans l'éditeur, rien ne s'importe.

- **Règles** (espace Rules) : toutes les valeurs du jeu (`game_rules.ron`), onglet Crafting complet, listes de choix partout (`editor/pickers.rs`).
- **Weave** : opérandes variables, événements de jeu (ON GAME EVENT), questions au monde, listes, tir, déplacement, joueur, écrans, sauvegarde, Pause / Step / Breakpoint, modèles de programmes.
- **Assemblage** : machines par composants (`BlockFunction::Custom`, 6 machines migrées), cerveaux de mobs (`Brain`), effets et combos d'objets.
- **Vitrine** : nouveaux widgets, écrans Overlay / Modal / Death / Pause, parties du HUD désactivables.
- **Ambiance** (nouvel espace) : sons par oscillateurs, musique à grille de pas par biome et moment, particules avec aperçu, ciel, nuages, météo (`audio.ron`, `atmosphere.ron`). Sons assignables aux blocs et aux mobs.
- **Monde** : filons (monde et biome), grottes, couches de roche, blocs du monde, reliefs par bruit (`Landform`, les 8 reliefs spéciaux sont devenus des préréglages), variantes de structures pondérées, carte 2D par biome / hauteur / bloc. Plus aucun identifiant de bloc en dur dans `world_gen.rs`.
- **Histoire** (nouvel espace) : quêtes, dialogues, succès (`quests.ron`), blocs Weave Start quest et Show dialogue.
- **Projets** : créer, ouvrir, dupliquer, projet exemple, arbre avec recherche, renommage avec mise à jour des références, suppression sûre vers `.trash`, validation globale, export d'un jeu sans studio dans `exports/`, aide intégrée. Ctrl+Z couvre les règles, le crafting, le son, l'ambiance et l'histoire.

## Nouveautés du 06/11 (refonte des designs)

- **Blocs** : 39 machines redessinées (éléments colorés et parties animées) et 30 blocs de terrain (couleurs de faces et mouchetures). Tout est dans `project/blocks/*.ron` et reste modifiable dans la Forge.
- **Mobs** : 21 rigs refaits (22 à 52 boîtes, clips Idle, Walk et Attack), stats et comportements inchangés. Tout est dans `project/entities/mobs/*.ron` et reste modifiable dans le Sculptor.
- **Forge, nouveaux réglages** : FACE COLOURS (couleur par face), FLECKS (mouchetures, avec lueur), et pour les parties Float axis, Rock, Pulse, Rhythm et phase. Les parties sont visibles dans l'aperçu de la Forge et dans les icônes.
- **Sculptor** : réglage Glow par boîte.
- **Shader** : `@version 4` pour les mouchetures (les copies du projet sont remplacées, avec un `.bak`).
- **Icônes de l'éditeur** : les 20 caractères sans glyphe (affichés en carrés) sont remplacés. `python tools/check_glyphs.py` signale tout caractère que les polices d'egui ne savent pas dessiner.
- **Générateurs** : `tools/design_blocks.py` et `tools/design_mobs.py` ont écrit ces données. Les relancer écrase les retouches faites dans la Forge ou le Sculptor sur les blocs et mobs qu'ils listent.
- **Restent simples** : les formes synthétiques (personnalisées par le joueur), l'eau, et les rigs Player et Player_Hand. Le terrain n'a ni modèle ni partie : un modèle casserait le maillage glouton et le masquage des faces. Les parties n'apparaissent pas en main ni sur les objets au sol (le modèle seul y est dessiné).

## Nouveautés du 03/11 (3e passe)

- **Icônes** : chaque icône est rendue une 2e fois 1,5 s après la première (le premier rendu d'un nouveau type de maillage pouvait sortir vide pendant la compilation du shader : épée, blaster, télécommande).
- **Inventaire créatif** : panneau INVENTORY à droite de la bibliothèque (30 cases) ; on y glisse les entrées de la bibliothèque, on réorganise, on glisse vers la barre rapide et retour (`slot_cells`, `Inventory::set_grid`).
- **Œufs de mob** : œuf cubique aux couleurs du mob, identique en icône et en main (`items::egg_boxes`).
- **Ports** : entrées et sorties au centre des faces latérales (entrées -X, -Z, +Z, +X ; sorties +X, +Z, -Z, -X ; plusieurs ports sur une face s'empilent) ; les pions sortent de leur face (`data_cables::port_face`).
- **Moteur arrêté** : un moteur du monde remet ses blocs dans le monde, alignés sur la grille (angle arrondi au quart de tour, `settle_rotor`) ; sur un movable, la roue d'un moteur arrêté tourne jusqu'au quart de tour le plus proche.

## Nouveautés du 03/11 (2e passe, 12 retours)

- **Moteur sur movable** : les blocs posés sur le moyeu d'un moteur deviennent une roue (corps à part, max 256 blocs) tenue par une liaison pivot Rapier dont le moteur suit la vitesse, le sens et l'alimentation réglés. Sans rien sur le moyeu, le moteur pousse le movable comme avant. `E` sur un cadre range le châssis et ses roues ensemble (`physics/movable_compound.rs` : `RotorOf`, `rotor_cells`).
- **Inventaire** : onglet *Resources* en créatif seulement ; la survie a *Inventory* + *Craft*. Les 30 cases de survie ont leur propre ordre (`Inventory::grid`) : glisser une case sur une autre les échange, case ↔ barre rapide dans les deux sens. Glisser un slot de la barre sur l'inventaire ne fait plus disparaître l'objet. Les conteneurs ont aussi des cases réorganisables (`MachineState::layout`, `MachineCommand::MoveStored`).
- **Blocs 100 % données** : plus aucune pièce ni modèle ajouté par le code (`machine_parts`, `machine_model` et le complément « parts absent → pièces du moteur » supprimés). Un bloc est exactement son fichier ; les pièces animées se font dans la Forge.
- **Câbles** : attaque avec le Câble = retire les câbles du port visé, sinon le câble visé (courbe), sinon tous ceux du bloc visé. Casser ou remplacer un bloc retire les câbles qui y étaient branchés (`prune_cables`, `VoxelWorld::take_changed`).
- **Customize** : toute la colonne de droite (couleurs, boutons, presets, import) dans un seul défilement ; bouton *Import* à côté d'*Export* et section IMPORT toujours visible.
- **Réticule** : affiche aussi la créature visée (nom, PV, apprivoisée / hostile) quand elle est devant le bloc visé (`mining::AimedEntity`).
- **Créatif** : les mobs hostiles ignorent le joueur (pas de poursuite, coup, recul ; capteurs de script à distance infinie) et aucun coup de mob ne le blesse.
- **Blocs retirés** : beacon, forge_table, plinth, plinth_trim, tower, cable_node, coal_ore, iron_ore, gold_ore (fichiers supprimés au chargement). Structures Ruin et Crystal_Spire mises à jour (temple_brick, crystal_ore).
- **Icônes** : les groupes (`@floral`…) montrent l'icône de leur ressource ; une icône impossible à construire reçoit une gemme teintée au lieu de rester vide.

## Nouveautés du 03/11 (conformité au document DetailsPolyhedron)

`cargo check` sans erreur ni avertissement, **113 tests OK**. Sauvegarde des fichiers d'avant la passe : `_sauvegarde_2026-11-03/`.

- **Ressources** (`gameplay/crafting.rs`, `crafting.ron` version 3 — l'ancien fichier est remplacé, copie `.ron.bak`) : exactement les 20 de la spec : Floral, Mineral, Crystalline, Faunal, Ethereal, Astral, Mnemonical (naturelles), Voltic (énergie), Synthetic, Technic, Conflux, Garbage, Atomic (craftées), Celliom, Aetech, Voltiom, Refractal, Fluxiom, Refreatech (infusions), Omnicore. **Retirées** : fuel, coal, iron, gold, alloy, circuit, plasma, prism, conductor, magnetite, vegetal. Recettes de la spec (Synthesizer, Assembler, Infuser, nouvelle station **Atomiser**) ; une infusion ratée donne du **Garbage**. Groupes `@floral`, `@mineral`, `@crystalline`.
- **Énergie Voltic** : Solaronel (1 / 9 s de jour) et Kineticster (1 / 12 s sous pression) produisent du Voltic ; l'Energiser le consomme pour son champ (1 Voltic = 20 de charge, Voltiom = 200) ; Combustor : 10 Floral ou 10 Faunal → 1 Voltic de puissance, petit rayon (4) ; Omnireactor alimenté par le réseau Omnicore.
- **Machines** (`voxel/blocks.rs`, `gameplay/machines/`) : ajout de Repulsor, Terminal, Comandor, Transmitter (portes + mode Decoder), Visualiser, Emitter, Receiver, Screen, Atomiser, Ressourcer, Ringer, Laser, Etheraltar, Kineticster, Reconstructor, Excavator, Ethereaper, Omnireactor, Invocateral, Televerseur, Mnemo-Digestor, Mnemo-Cloner, Astral-Modifier, Astral-Container. Vacuum → **Astral-Vacum**, Harvester → **Mnemo-Harvestor**, Lamp → **Light**, Frame → **Movable**, Wagon Seat → **Wagon**, Solar → **Solaronel**. Réglages par machine (`machines/config.rs`, sauvés dans le `.meta` du monde), panneau `E` pour chacune, bouton ON/OFF. Valeurs typées Booléen / Float / String sur les câbles Signal (`gameplay/logic.rs`). Beacon retiré (caché).
- **Outils et armures** (`gameplay/items.rs`, `gameplay/tools.rs`) : Sword (2), Pickaxe (rapide sur la roche), Axe (rapide sur le Floral), Gun (3, 1 Mineral / tir), Brush (couleur choisie avec `C`), Wrench, Wire, Remote Control, Binder, Atomixer (Atomic aléatoire), EtheraGun, BlastyGun, VoltiGun, AstralSaber (4), MineralCoat (-1), TechnicArmor (-2, saut en plus), RefreatechArmor (-3, vol), AstralRevetment (-4), Mnemo-Symbiote (-2, +1 dégât, +2 PV), Etherealink. Armure portée avec Clic D. Pomme et Crystal blade retirées. Les revêtements (coatings) sont remplacés par les armures.
- **Entités** (`gameplay/mobs/bestiary.rs`, `gameplay/mob_ai.rs`) : les 21 entités de la spec avec leur comportement natif, leurs biomes, jour / nuit, butin (Faunal, Ethereal, Astral, Mnemonical). Apparition naturelle par biome, Galopin chevauchable une fois lié (`E`, Saut pour descendre), Grom qui bâtit son nid, se reproduit et explose, etc. Le Sentinel Drone d'exemple est retiré. Comportement / biomes / butin éditables dans l'atelier Mobs.
- **Biomes** : les 16 de la spec. Ajout Desert, Swamp, Oasis, Burnt Forest, Pyramid and Temple ; renommés Crater, Mnemo Plain, Mud, Ice Floe ; retirés Lake et Test. Minerai d'Astral au cœur des cratères, Mnemonical dans les Mnemo Plains, filons de Mineral dans les grottes. Structures Palm Tree, Burnt Tree, Pyramid, Temple.
- **Commandes** (`gameplay/commands.rs`) : les 40 commandes de la spec (+ `/gamemode` et `/give`), partagées par le chat, le Terminal et le Comandor ; sélecteurs `@p`, `@s`, `@n` ou nom d'entité ; `~` relatif. Les anciennes commandes façon Minecraft sont retirées.
- **Autres** : écran de mort (spawn du monde / là où je suis mort, 5 Ethereal / Etheraltar, 2 Ethereal) ; presets de personnalisation, import / export de personnalisation et de monde (`project/exports/`) ; curseur à la manette (stick gauche + A) quand la souris est libre.
- À supprimer à la main (plus compilés) : `src/gameplay/coatings.rs`, `src/gameplay/components.rs`.

## Nouveautés du 01/11 (6e passe, 2e liste de retours)

- **Inventaire** : **30 emplacements** + la hotbar, piles de **999** (blocs et ressources). Nouvel onglet **Resources**. En survie, un emplacement de hotbar à 0 se **vide**. Fiche d'un bloc récolté : **Convert 1 / Convert all** en ressource pure (herbe → *Vegetal*, pierre → *Mineral* ; règles `conversions` de `crafting.ron`, version 2 — l'ancien fichier est sauvé en `.ron.bak`).
- **Panneaux de machines** : Synthesizer et Infuser en **emplacements** : on glisse deux ressources depuis l'inventaire, le résultat de la recette correspondante s'affiche, bouton **Synthesize / Infuse** (x5). Assembler, Synthesizer, Infuser et Container montrent **l'inventaire du joueur dessous**.
- **Container** = coffre : 27 emplacements, glisser-déposer dans les deux sens, *Take everything*, rempli par les câbles Ressource.
- **Energizer** : se charge par son entrée Énergie (ou un champ voisin) et émet un **champ de force** (anneaux visibles) qui alimente **toutes les machines dans son rayon**, sans sortie.
- **Câbles et ports en vrais maillages** (`gameplay/cable_visuals.rs`) : tubes affaissés teintés par type avec une impulsion lumineuse, ports en **picots** : Énergie = carré, Ressource = triangle, Signal = hexagone ; **entrées vertes, sorties rouges**.
- **Blocs 100 % données** : les suppléments (moyeu du moteur, hublot du combustor, cœur de l'energizer…) sont des **parties animées** du bloc (`BlockDef::parts`, section *Animated parts* de la Forge : boîte, pivot, rotation, flottement, mouvement *Always / Powered / Motor*, couleur allumée). Elles s'affichent aussi sur les movables lancés.
- **Moteur sur movable** : son moyeu tourne et il pousse assez fort pour vaincre le frottement (frottement des movables 0,4) ; un moteur à l'axe vertical sert de rotor de sustentation.
- **Contrôleur / Télécommande** : `E` sur un Controller avec la Télécommande en main la **connecte** (`E` à nouveau : déconnecte ; un autre contrôleur la remplace). La connexion suit le lancement et le retour au sol. Sprint + ZQSD pilote, **même sans moteur** (petits propulseurs du contrôleur).
- **Siège** : le joueur est recollé au siège **après** la physique, sans contrôleur de personnage : plus de tremblement.
- **Mobs** : sautent une marche d'un bloc.
- **Cable Node** : marqué `hidden` (jamais dans l'inventaire ni l'artisanat ; retiré des inventaires existants).
- **Objets sans modèle** : un objet sans boîte prend le modèle de l'objet par défaut de son type (ou un lingot) — main, icône et objet au sol.
- **Animations du joueur en données** : plus aucun mouvement procédural. Les rigs *Player* et *Player Hand* reçoivent les clips par défaut qui leur manquent (`Idle`, `Walk`, `Swim`, `Fly`, `Sit`, `Hold`, `Block`, `Attack`, `Use`), enregistrés et éditables dans l'atelier Mobs. Seule la tête suit encore l'inclinaison de la caméra.
- **Shader** : au démarrage, le **dernier preset utilisé** est rechargé (`project/shaders/presets/last.txt`), sinon le premier créé ; un preset `Default` est créé s'il n'y en a aucun.

## Nouveautés du 01/11 (5e passe, retours de Feedback.txt)

- **Popups** : les fenêtres du jeu (inventaire, machines, menus, personnalisation) sont dans la couche `Middle` (`theme::WINDOW_ORDER`) ; les listes déroulantes, sélecteurs de couleur et menus s'ouvrent donc toujours **devant**. L'aperçu 3D du joueur (HUD et menu) est maintenant **rendu dans une texture** affichée par l'interface : il ne passe plus par-dessus les popups et **reste visible quand un menu est ouvert**.
- **Vent / vagues** : le shader de surface déclare enfin son étage *vertex* (`MaterialExtension::vertex_shader`) : le **bloc** ondule comme son contour.
- **Orientation des blocs** : le motif (Voronoi, facettes) suit l'orientation du voxel (UV_1 du maillage) ; le terrain généré reçoit une **direction aléatoire par cellule** (comme Minecraft). Un bloc posé **regarde le joueur** ; règle par bloc dans la Forge (*Placement* : face au joueur / aligné sur la face cliquée, les troncs sont alignés). Shaders en `@version 3`.
- **Movables** : poussés par les **liquides** (flottaison, freinage, courant suivant les niveaux d'écoulement). Les blocs **construits par le joueur** (jeu, éditeur, movable reposé) sont marqués (`VoxelWorld::mark_placed`) et suivent toujours le lancement (dessus, côtés, dessous), même si le reste touche le terrain.
- **Siège** : `E` sur un *Wagon Seat* assoit le joueur, qu'il soit posé dans le monde ou sur un movable (le siège visé, plus seulement le plus proche) ; `E` pour se lever.
- **Câbles refaits** : plus de *Cable Node* (les anciens fichiers se chargent comme blocs simples). Chaque bloc a des **ports typés** (Énergie / Ressource / Signal, entrée ou sortie) : ceux de sa fonction par défaut, ou les siens (section *Cable ports* de la Forge). Objet **Cable** (Armurerie, type *Cable tool*) : en main, il affiche les ports des machines proches ; Clic D sur un port puis sur un autre relie une sortie à une entrée du même type ; Clic G sur un port supprime ses câbles. Nouveaux blocs **Energizer** (stocke l'énergie de son entrée, la rend sur sa sortie) et **Container** (stocke les ressources). Le **Vacuum** aspire aussi les objets posés dessus. Câbles du studio : `project/worlds/cables.ron` ; câbles d'un monde du menu : dans son `.meta`.
- **Artisanat en données** : `project/crafting.ron` (créé au premier lancement) contient ressources, groupes, recettes et le coût par défaut. Le **Synthesizer** ne fait plus qu'une chose : la ressource **Synthetic**. À la main (onglet *Craft*), on fabrique **n'importe quel bloc ou objet** : sa recette s'il en a une, sinon le coût par défaut (1 Synthetic). Éditeur des recettes dans l'Armurerie (*CRAFTING*).
- **Inventaire** : les ressources se **glissent-déposent** (dans la barre, ou hors de la fenêtre pour les jeter) ; touche **G** (*Drop*) : jeter l'objet tenu (avec Maj : la pile). Boutons *Drop 1 / Drop all* dans la fiche. Listes défilables à la **molette** partout (`theme::wheel_scroll`).
- **Personnalisation** : une ligne par **boîte du modèle Player** (Torso, Head, Eye L/R, Arm L, Hand L…), en arbre ; couleur par nom de boîte (les anciens réglages par partie sont convertis).
- **Studio** : **Ctrl+Z / Ctrl+Y** (et boutons ↩ ↪) annulent / rétablissent n'importe quelle modification : monde, mobs, objets, bloc de la Forge, biomes, structures, scripts, interface (`editor/undo.rs`, 200 étapes, un glisser = une étape). **Multi-sélection** (Maj / Ctrl + clic, dans la vue et les listes) dans la Forge, l'Armurerie et le Sculpteur : le gizmo, Suppr et Dupliquer agissent sur toute la sélection.
- **HUD** : le compteur *Kills* et la barre du cycle jour/nuit sont retirés du HUD par défaut (la layout reste éditable dans la Vitrine).
- **Moins de code en dur** : Épée, Télécommande et Blaster sont des **objets de l'Armurerie** (`project/items/sword|remote|blaster.ron`, types *Weapon / Remote / Blaster*), modèles et icônes compris. Les **animations du joueur** jouent les **clips des rigs** *Player* / *Player Hand* s'ils existent (`Idle`, `Walk`, `Swim`, `Fly`, `Sit`, `Attack`, `Use`, `Block`, `Menu`) ; le mouvement procédural n'est plus qu'un repli pour les os qu'aucun clip n'anime.

## Nouveautés du 01/11 (4e passe)

- **Chat** : `Tab` complète le mot (commandes, sous-commandes, blocs, objets, ressources, mobs, biomes) et fait défiler les propositions ; la liste s'affiche au-dessus de la saisie. Nouvelles commandes : `/gamemode spectator`, `/omnicore`, `/give egg:<mob>`.
- **Modes de jeu** : F7 depuis l'éditeur lance toujours en **créatif** ; nouveau mode **spectateur** (vol sans collision, invisible, aucune interaction).
- **Combat** : le combo ne progresse que si le coup **touche** une entité (un coup dans le vide le remet à zéro) ; **hitmarker** sur le réticule (doré pour le 3e coup).
- **Artisanat** (`gameplay/crafting.rs`) : onglet *Craft* de l'inventaire (à la main : Assembler, Synthesizer, carburant, outils), **Synthesizer** (matière végétale + minérale -> blocs synthétiques), **Assembler** (nouveau bloc : ressources de base alloy / voltic / ethereal / circuit / fuel, toutes les machines, l'Omnicore), **Infuser** (surcharge : voltic + ethereal -> plasma, ethereal + crystal -> prism, voltic + gold -> conductor, alloy + voltic -> magnetite ; + revêtements). Les minerais lâchent coal / iron / gold / crystal.
- **Réseau Omnicore** : réserve par monde (sauvée dans le `.meta`). En survie chaque commande coûte 1 Omnicore. L'**Omnichorus** (nouveau bloc, `E`) y transfère l'Omnicore de l'inventaire. L'Omnicore se fabrique à l'Assembler (fin de jeu).
- **Inventaire** : icônes **rendues en 3D** (blocs, objets, outils, œufs, ressources ; `renderer/icons.rs`) partout (inventaire, barre, recettes) ; onglets **Blocks / Machines / Items & tools / Mobs** (œufs d'apparition, clic droit sur un bloc) **/ Inventory / Craft** ; info-bulle au survol, clic = fiche, le glisser ne démarre qu'au déplacement (plus de conflit avec le clic).
- **Couleur par élément** : dans la Forge (espace Blocks), avec un élément du modèle sélectionné, *Element colour* ne peint que la sélection ; *Use block colour* revient à la couleur du bloc (champ `color` de l'élément dans le `.ron`).
- **Blocs synthétiques** : les 9 formes (Synthetic Cube, Slab, Wall, Stairs..., Ramp...) se personnalisent avant de les poser avec **C** (couleur libre, teinte Voronoi, relief low-poly : force, facettes, biseau, motif, et **couleur par partie** : clic sur une partie du bloc dans la vue 3D orientable du designer, chaque plan de la forme se peint séparément) ; chaque variante est un bloc à part entière (`base~couleur~...`), reconstruit au chargement du monde ; elle lâche et consomme le bloc de base.
- **Shaders** : la normal map Voronoi et la teinte sont calculées dans l'espace du **bloc** (elles bougent avec les movables) ; **vent** (paramètre *Wind* par bloc, feuilles par défaut, contours compris) et **vagues** des liquides ; **presets** de shader (enregistrer / charger / supprimer, `project/shaders/presets/`). Les anciens fichiers `project/shaders/*.wesl` sont remplacés automatiquement (copie `.wesl.bak`).
- **Movables** : ne démarrent plus seuls ; `E` sur un cadre les lance avec **tous les blocs accrochés** (au niveau du cadre et au-dessus) ; `E` sur un de leurs blocs les **repose** dans la grille (rotation arrondie). Un **moteur** monté dessus (réglé avant ou après le lancement, `E`) **pousse** le movable le long de son axe (avant / arrière, puissance).
- **Personnage** : modèle 3D du joueur et main 1re personne éditables dans l'éditeur (espace Mobs, rigs *Player* et *Player Hand*, non supprimables) ; personnage **3D au menu** dont la tête suit la souris ; personnalisation **par partie** (tête, torse, bras, jambes) par listes déroulantes de couleurs nommées (ou couleur libre).

## Nouveautés du 01/11 (3e passe)

- **Tout le jeu et l'éditeur en anglais, sans accents** (textes d'interface, messages, noms d'outils : Weave, Sculptor, Forge, Terraformer, Workshop, Armoury, Showcase ; biomes Plains, Great Forest, Volcano, Frozen Lake ; objets du projet).
- **Contours** : la coque d'outline utilise maintenant des directions « soudées » (somme des normales des faces qui se touchent, T-jonctions comprises) : les contours se rejoignent aux coins (`mesher::outline_directions`, `poly_outline.wesl`).
- **Ciel** (`renderer/sky.rs`) : dôme en dégradé (lueur au lever / coucher), soleil carré avec halo, lune en croissant de pixels carrés, ~900 étoiles qui tournent avec le ciel ; les nuages s'assombrissent la nuit.
- **Créatif** : ni vie ni endurance, cassage instantané, aucun drop, **vol** (double Espace ; Espace monte, Maj descend, Ctrl rapide). Survie inchangée.
- **Movables poussables** : en les frappant (impulsion au point visé) ou en marchant contre eux (les gros sont plus lourds).
- **Grottes** façon Caves & Cliffs (`world_gen.rs`) : cavernes « cheese », tunnels « spaghetti », passages « noodle », lacs de lave au fond, deepslate sous y = -6, minerais (charbon, fer, or, cristal lumineux), sols de mousse lumineuse dans les biomes humides. Bedrock à -48 par défaut. Option *Caves* à la création d'un monde et dans les réglages de génération.
- **HUD** (style éditeur : sombre, moderne, arrondi) : barre d'objets arrondie, cœurs / endurance en survie seulement, réticule avec combo (seulement > 0) et nom du bloc visé au-dessus, actions possibles en dessous (Break / Place / Configure / Open / Launch / Sit, avec la touche).
- **Aperçu 3D du joueur** en haut à droite : une vraie réplique animée (mêmes membres, même objet en main) rendue par une caméra dédiée (`gameplay/player_preview.rs`). **Minicarte** sans à-coups (scan incrémental + texture, défilement fluide), biome et heure. Les deux se désactivent dans Settings → Interface.
- **Inventaire** à onglets (Blocks / Items & tools / Inventory, recherche) ; clic = fiche détaillée de l'entrée (forme, matière, dureté, fonction, drop…), double-clic = mettre dans la case.
- **Chat + commandes** (`T`, `/`) : /help, /time set|add|query, /gamerule doDaylightCycle, /gamemode, /tp (~ relatif), /locate biome, /give, /clear, /kill, /heal, /fly, /spawnpoint, /seed, /summon, /say. Historique avec ↑ / ↓ ; les messages du jeu s'affichent dans le fil.
- **Machines** : `E` ouvre un panneau (alimentation, carburant, stockage, actions : ajouter du carburant, charger, collecter, fabriquer un revêtement…). Modèles 3D (boîtiers) + pièces animées (moyeu du moteur, lame de la moissonneuse, ventilateur, cristal flottant, anneau, fenêtre du four). **Moteur** : hors movable, Start/Stop, vitesse (tr/min) et sens ; il fait tourner tous les blocs accrochés à sa face du dessus (rotor cinématique, collisions comprises).
- **Menus** refaits dans le style de l'éditeur ; **Quit** ferme vraiment le jeu (en sauvegardant le monde s'il est joué).
- Police du HUD (chiffres, coordonnées, FPS) : `assets/fonts/lcddot_tr.ttf`.
- Compilation : 12 erreurs corrigées (helpers de style manquants, API Bevy 0.20 `get_mut` / `shadow_maps_enabled`, emprunts) ; `check` propre, 89/89 tests.

## Nouveautés du 01/11 (2e passe)

- **Menu principal** au style du jeu (barre noire, titre en points « POLYHEDRON », police pixel originale *PolyPixel* générée par `tools/make_pixel_font.py`, « By Batpolygon », boutons noirs à onglet blanc, personnage à droite + **Personnaliser** la couleur). Derrière : un **panorama procédural** généré en direct avec les paramètres choisis.
- **Jouer → Mondes** : liste des mondes (`project/worlds/<nom>.ron` + `<nom>.meta`), **Créer un monde** (nom, graine + 🎲, type Infini / Plat / Île, mode Créatif / Survie, biome unique, niveau de la mer). Le jeu attend le terrain sous le point d'apparition, puis démarre ; le monde est **sauvegardé en quittant** (pause → Sauver, studio / menu). Revenir au studio remet le monde de l'éditeur.
- **Inventaire glisser-déposer** (`Tab` / `I`) : créatif = tous les blocs, objets et outils ; survie = ce qui a été ramassé, avec quantités. Barre d'objets en jeu au style des visuels, panneau Player / Map (minicarte) / coordonnées en haut à droite.
- **Objets lâchés** : les blocs cassés et le butin des mobs tombent au sol, tournent et sont aspirés quand on s'approche.
- **Aperçu du bloc à poser** (fantôme translucide, vraie forme et orientation), en jeu et dans l'éditeur.
- **🗡 Armurerie** (nouvel espace Items) : objets en boîtes colorées (outils = minage plus rapide, armes = combo de l'épée avec dégâts/recul, nourriture = soin), aperçu 3D, gizmo, `project/items/`. Ils se tiennent en main, se lâchent et se ramassent.
- **🏛 Atelier de structures** : la structure s'affiche directement ; poser / effacer / ancres / pipette au clic, la boîte suit le contenu. La salle dans le monde reste disponible (Avancé).
- **Mobs** : plusieurs scripts Trame par mob, vitesse ×, dégâts ×, délai d'attaque, portée ×, résistance au recul.
- **Vitrine** : hiérarchie à gauche, aperçu au centre, propriétés à droite.
- **Monde → Génération procédurale** directement dans l'onglet Gameplay de l'espace Monde.
- **Chunks** : chargement / déchargement par défaut (les colonnes modifiées restent en mémoire et reviennent intactes), insertion en bloc, plus de threads de génération, remaillage limité à quelques chunks par image (les plus proches d'abord).
- **Bloc** : une seule couleur (les éléments d'un modèle prennent la couleur du bloc).
- **Look** : herbe et feuillage vert vif, troncs orange, contours clairs, nuages cubiques, personnage jaune avec yeux, bras 1re personne en gros bloc.

## Nouveautés du 01/11

- **Liquides** (`voxel/fluids.rs`) : l'eau (et tout bloc `liquid`) s'écoule — chute d'abord, puis s'étale jusqu'à `flow_range` (eau 7, lave 4), sources infinies (2 sources voisines sur sol), assèchement quand la source disparaît. Seules les cellules « réveillées » par une modification sont simulées : un océan généré ne coûte rien. Surfaces inclinées selon les niveaux, translucides, sans collision. Le liquide qui coule n'est pas sauvegardé (seules les sources).
- **Nage** : sans collision dans l'eau, flottabilité, Espace pour remonter, sortie sur un rebord, teinte sous l'eau. Un bloc d'eau est dans la barre d'objets (slot 5).
- **Recul corrigé** : la composante verticale du recul était ré-appliquée à chaque image (propulsion énorme) ; c'est maintenant un petit saut unique + un recul horizontal plafonné, pour le joueur comme pour les mobs.
- **Facettes low-poly** : normal map Voronoi par bloc (relief, facettes/bloc, biseau, graine), réglable dans la Forge ; interrupteur global + multiplicateur dans l'espace Shader.
- **🧱 Forge** (espace Blocs) : aperçu 3D au centre (monde masqué, caméra orbitale), bibliothèque à gauche, détails à droite. Un bloc peut être un **modèle multi-boîtes** (éléments 16×16×16, clic pour sélectionner, gizmo G déplacer / S taille, magnétisme 1/16). Aperçus : bloc seul, mur 3×2, plateforme (les liquides y coulent). « Appliquer » met à jour le monde, « Enregistrer » écrit le `.ron`. Exemple fourni : *Table (forge)*.
- **🌲 Terraformeur** (espace Biomes) : aperçu 3D du biome sélectionné généré seul (relief, palette, relief spécial, eau, structures naturelles), sous la couleur de ciel du biome ; taille 32 à 96 blocs, 🎲 autre endroit. Liste + création de biomes à gauche, réglages à droite.
- **Main 3D** en 1re personne (objet tenu : vrai maillage du bloc, épée, blaster, télécommande ; balancement, coup, pose) et **corps** en 3e personne (marche, nage, attaque, objet en main).
- **Menu principal** (Jouer / Studio / Paramètres / Quitter) et **menu pause** (Échap : le jeu se fige). **Paramètres** : sensibilité, inversion Y, FOV, distance d'affichage, balancement, FPS, volumes, **touches réassignables** (2 par action). Sauvés dans `project/ui/settings.ron`.
- **Noms des outils** : Trame (scripts), Sculpteur (mobs), Forge (blocs), Terraformeur (biomes), Vitrine (interface).
- Sauvegarde des fichiers d'avant la mise à jour : `_sauvegarde_2026-11-01/`.

## Interface (refonte du 31/10)

- **Espaces de travail** dans la barre du haut : 🌍 Monde · 🧱 Blocs · 🐾 Mobs · 🧩 Scripts · 🌲 Biomes · 🏛 Structures · 🖼 Interface · 🎨 Shader. Chacun a sa propre disposition (☰ → « Réinitialiser cet espace » pour revenir au défaut).
- Bouton **▶ Jouer** (F7) et 💾 en haut à droite, menu ☰ pour le reste.
- **🐾 Sculpteur (mobs)** : preview au centre (monde masqué, caméra orbitale : clic droit tourner, molette zoom, clic molette déplacer, F cadrer), gizmos ✥ déplacer (G) · ⟳ pivoter (R) · ⤢ taille (S) · 📍 pivot (P), magnétisme 1/16 bloc et 15° (Ctrl pour l'ignorer), clic sur un cube pour le sélectionner. Modes 🧊 Modèle / 🎬 Animation (Tab) : en Animation, le gizmo crée des clés à la tête de lecture.
- 🦴 Squelette à gauche, 🎛 Détails à droite (transform, taille, rotation, pivot, couleur + palette, clé courante, réglages en jeu), 🎬 Timeline en bas (animations, lecture, pistes par cube, clés en losanges : glisser pour décaler, double-clic pour ajouter).
- Palette de blocs en tuiles colorées avec recherche et filtres.

## Points clés

- **9 formes de la spec** (Cube, HHalfCube, VHalfCube, Stairs, StairsIn/OutCorner, Ramp, RampIn/OutCorner), 24 orientations (6 faces × 4 roulis) stockées par voxel. Les anciens noms (Wedge, Cylinder…) restent lisibles via alias serde.
- **Fusion sub-voxel** : deux demi-cubes complémentaires dans une cellule, chacun avec son bloc/matériau. Culling exact des faces via table de couverture.
- **Collisions** : un `Collider::trimesh` Rapier par chunk, généré depuis le même maillage.
- **PIE** : snapshot RAM du monde + caméra ; rollback complet ; toutes les entités de jeu portent `PieOnly`.
- **Trame** : programmes globaux ou *liés* (instanciés par bloc / mob, variables locales, `g.` pour partager). Handlers On Start/Tick/Interact/Hit/Signal/Energy. Blocs : Math, Sense, SetBlock, BreakBlock, Particles, Drop, Sound, Time of day, Spawn mob, Move toward, Impulse, Play action, Set health, Attack, Tame, Set signal. Programmes fournis : *Mob Brain*, *Day Night*, *Signal Lamp*.
- **Biomes** : 11 biomes de base + Alien Crater + Mnemo Zone, sélection par climat (température/humidité), mélange des hauteurs, reliefs spéciaux, règles de structures (chance, pente, sol). Streaming infini sur threads de travail.
- **Structures** : salle isolée (4096, 256, 4096), capture, blocs d'ancrage.
- **Vitrine** : arbre de widgets, drag/resize, ancres, bindings ; la layout `HUD` est affichée en jeu.
- **Audio** : synthé DSP pur Rust sur thread dédié (pas, minage, laser, moteur RPM, UI, vent, parade…).

## Contrôles en jeu (F7)

Par défaut (réassignable dans Settings → Controls) : C personnaliser le bloc synthétique tenu · G jeter l'objet tenu (Maj : la pile) · ZQSD/WASD bouger · Espace sauter / nager (double Espace = voler en créatif) · Maj sprint / descendre · Ctrl dash / vol rapide · F5 vue 1re/3e personne · 1-9 / molette hotbar · Tab / I inventaire · T chat · / commande · Clic G miner/attaquer/pousser · Clic D poser/parer/manger · E interagir (panneaux de machines) · Maj + Télécommande piloter · Échap pause.

## Limites connues

- Restent dans le moteur, sans assemblage possible : movables, véhicules, moteurs, physique des câbles (réglages exposés), interface de l'éditeur, menu principal, glisser-déposer de l'inventaire, panneaux de machines, fenêtre des réglages.
- Ouvrir un projet, renommer ou supprimer depuis le panneau Project redémarre l'application : enregistrer d'abord.
- Renommer remplace toutes les chaînes exactement égales au nom dans le projet, y compris un libellé identique par hasard.
- Le ciel, la météo, les particules et la musique ne sont vérifiés que par les tests et un démarrage à vide : le rendu et le son en jeu restent à contrôler.
- Les gizmos 3D de manipulation (translate/rotate) des boîtes de mob restent ceux de l'éditeur existant (champs numériques) ; pas de gizmo à poignées.
- Sauvegarder le monde pendant que la salle de structures est ouverte inclut la salle.
- Les mobs ne nagent pas (ils marchent au fond de l'eau).
- Un rotor de moteur arrêté reste dans sa position ; il ne redevient pas des blocs du monde avant la fin de la partie.
- Le **nom** des clips joués par le joueur et le moment où ils jouent (marche, nage, attaque…) restent décidés par le moteur (`avatar::clips`) ; un clip vidé de ses clés ne joue simplement pas.
- Le vent / les vagues ne s'appliquent pas aux ombres (le prépass d'ombre garde le vertex standard).
- Solar, Kineticster, Atomiser, Excavator, Harvester et Vacuum sont en données ; les autres machines (Reconstructor, Combustor, Omnireactor, Repulsor, Ringer, MnemoDigestor…) restent des fonctions du moteur, leurs ports, recettes et réglages étant des données.
- Les ports s'affichent au-dessus des blocs (pas encore sur la face réelle du bloc orienté).
- Le fichier `project/blocks/cable_node.ron` peut être supprimé (bloc caché désormais).
- Les ports sont affichés près de la caméra (12 blocs) et seulement sur les blocs du monde (pas sur un movable lancé).
- Le vent ne déplace pas les ombres des feuilles.
- La recherche /locate biome se fait sur le climat (rapide) dans un rayon de 6000 blocs.
- La main 1re personne peut traverser un mur si on s'y colle.
- `src/gameplay/components.rs` n'est plus utilisé (non compilé) : peut être supprimé.
