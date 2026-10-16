The proof of concept of `ForgeDesMondes`, as decided when the factory was created. The whole record is `mock/proof-of-concept.json` under `mock/` on the integration branch.

## What the application does

### What is the application, and what problem does it solve?

Une application pour faire rouler mes campagnes ou oneshot DND. Elle offre une aide à la préparation, qui porte les joueurs, personnages, lieux, événements et les organise dans des séances, et aussi un mode pour faire rouler la session en direct.

### Who uses it?

- **Le DM** (moi) : prépare la campagne et fait rouler la session depuis la DM view, avec tous les éléments de la campagne, les suggestions de l'IA et la soundbox.
- **Les joueurs** : voient la player view, où le DM envoie des battlemaps ou des illustrations. Ils sont utilisateur spectateur, pas d'interaction.

### What do they do with it?

**Préparer une campagne ou un oneshot**
- Saisir les joueurs, personnages, lieux et événements, et les organiser dans des séances.

**Faire rouler une session**
- J'ouvre deux pages : la DM view et la player view.
- Un micro écoute ce qui se passe en temps réel ; une petite fenêtre en bas à droite me propose des éléments à ouvrir pour m'aider. Exemple : je dis un lieu, le lieu apparaît en bas à droite, je peux cliquer dessus et ça ouvre le lieu sur la page principale.
- À gauche, j'ai tous les éléments de la campagne.
- Un onglet soundbox me permet de jouer des sons ; l'IA peut aussi proposer des sons de la soundbox qui fittent avec ce qui se passe.
- Vers la player view, je peux envoyer des battlemaps ou des illustrations que j'ai préconstruites.
- Les battlemaps ont un overlay hexagonal et des tokens de personnages (joueurs ou non) que je peux déplacer sur la vue.
- Le MJ peut lancer un combat sur une battlemap, dans ce cas tout les non joueurs ont une initiative aleatoire, tout les joueurs j'ai un champ pour mettre l'initiative. Un carrousel a l'image de celui du plugin foundry apparait sur la vue joueur, je peux le voir sur la vue MJ egalement
- Lorsque le MJ deplace les tokens, un calcul automatique du nombre d'hexagone de chemin apparait en vert pendant le drag and drop. Il prend son essence ou le token etait placé et calcul par rapport a ou mon curseur est. Le curseur porte d'ailleur le token

### What does it keep, and what does it connect to?

- **Éléments de campagne** : joueurs, personnages, lieux, événements, organisés en séances.
- **Médias** : battlemaps et illustrations préconstruites ; tokens de personnages (joueurs ou non) ; sons de la soundbox.
- **Micro** : écoute en temps réel de la session, analysée par une IA pour proposer des éléments et des sons.
- **Deux vues** ouvertes simultanément : DM view et player view.

## Scope

## Goal

Ce prototype doit prouver que ForgeDesMondes divise par deux le temps de préparation d'une séance et suffit à faire rouler une vraie séance de la campagne en cours, sans autre outil.

- **Préparation.** Le DM reprend sa campagne actuelle à partir de ses notes, d'une saisie manuelle et de l'import D&D Beyond. Il prépare la séance suivante en grande partie à l'oral :
  - un brief raconté au micro devient une séance découpée en scènes ;
  - l'IA repère les PNJ, lieux et événements existants et les met à jour, avec un historique réversible ;
  - elle place les entités manquantes dans une liste « à créer » ;
  - elle propose pour les PNJ des statblocks crédibles et un background.
- **Séance en direct.** La DM view et la player view tournent dans deux onglets synchronisés de la même machine. Le micro, transcrit en local, fait remonter les entités et les sons nommés. Les battlemaps hexagonales portent les tokens, la mesure de distance et le combat avec son carrousel d'initiative. La soundbox joue les effets à la demande.
- **Après la séance.** Le journal et son résumé nourrissent la préparation suivante.

## Screens

- **Accueil des campagnes**
  - Le DM voit la liste de ses campagnes et oneshots, chacun avec son illustration de couverture.
  - Il crée une campagne ou un oneshot. Un oneshot est une campagne à une seule séance.
  - Il exporte une campagne en une archive, ou importe une archive.
  - Il accède aux **Paramètres IA**.
- **Paramètres IA**
  - Le DM choisit le fournisseur de génération : une API cloud, ou l'endpoint opencode de son serveur maison. Il renseigne l'URL, le modèle de génération, le modèle de résumé et une clé éventuelle.
  - Un bouton « Tester la connexion » vérifie le réglage.
  - Un rappel indique que la transcription et la détection restent locales et que seul du texte part vers ce fournisseur.
- **Campagne**
  - Le DM voit la liste ordonnée des séances, avec leur résumé.
  - Il voit les éléments de la campagne par onglets : joueurs, personnages (PJ et PNJ), lieux, événements, médias, sons.
  - Il crée ou ouvre une séance, ou la lance en direct. À la création d'une séance, il sélectionne dans une liste les PJ présents. Il peut en retirer ou en ajouter plus tard.
  - Il importe des notes en texte libre, tapées, collées ou dictées dans l'outil. L'IA en extrait des personnages, des lieux et des événements. Il valide la liste proposée en lot avant la création, et la note brute est conservée.
  - Il voit la liste des **entités à créer** (PNJ, lieux, événements), issue des briefs de séance.
- **Fiche d'entité (édition)**, pour un personnage, un lieu ou un événement
  - Le DM renseigne le nom, les alias et la description.
  - Pour un personnage, il précise s'il s'agit d'un PJ (lié à un joueur) ou d'un PNJ. La fiche reste simple : caractéristiques, CA, PV, sorts et capacités (nom et description). Il ajoute un portrait, le recadrage du token et des notes, les notes dans le cas d'un PNJ.
  - Un bouton micro permet de **raconter le prompt initial à l'oral**. L'IA propose ensuite un background, un comportement pour pouvoir l'interpreter et, pour un PNJ, un statblock crédible.
  - Pour un PJ : un champ URL D&D Beyond, avec les boutons « Importer » et « Resynchroniser ». La resynchronisation conserve les alias, les notes et le token locaux.
  - Quand l'IA a modifié la fiche, un bouton **« Mis à jour »** apparaît. Il ouvre le diff et l'historique des versions, avec la possibilité de revenir en arrière.
- **Préparation de séance**
  - En tête, le DM voit le résumé de la séance précédente, avec les entités mentionnées et les fils ouverts. Le journal complet est consultable dans un panneau latéral.
  - Il voit les PJ présents et la liste ordonnée des scènes. Chaque scène est liée à des lieux, des PNJ, des événements et des médias, et porte des notes libres.
  - Un bouton **« Brief oral »** lance l'enregistrement. L'IA en tire les scènes et affiche :
    - les entités existantes détectées, mises à jour si nécessaire, avec leur diff ;
    - les entités manquantes (PNJ, lieux, événements), ajoutées à la liste « à créer ».
  - Pour chaque entité manquante, le DM choisit « Créer depuis le brief », ou « Affiner » pour compléter la description à l'oral ou à l'écrit avant la proposition.
  - Il associe une instance de battlemap à une scène et y place les personnages de cette scène.
- **Médias et réglage de battlemap**
  - Le DM téléverse des battlemaps et des illustrations.
  - Pour chaque battlemap, il règle la grille hexagonale : taille, décalage, orientation, couleur et opacité. Un aperçu en direct accompagne le réglage. La règle est 1 hex = 5 pieds.
- **Soundbox (bibliothèque)**
  - Le DM téléverse des sons, les nomme et leur attribue des tags.
- **DM view (séance en direct)**
  - **À gauche** : la séance en cours en tête, scène par scène, avec ses lieux, PNJ et médias. Viennent ensuite tous les autres éléments de la campagne par type, avec une recherche approximative qui tient compte des alias.
  - **Au centre**, selon ce qu'il ouvre :
    - la fiche d'élément en lecture, optimisée pour la table, avec un bouton d'édition ;
    - ou la battlemap active.
  - **Sur la battlemap** :
    - le DM glisse-dépose les tokens sur la grille ; le curseur porte le token et le compteur vert affiche par exemple « 6 hex · 30 pi » depuis la position de départ ;
    - il dépose un personnage depuis le panneau gauche, en une seule instance ou en « × N », avec numérotation automatique et nom modifiable ;
    - chaque token a un interrupteur visible/caché, des badges d'état et, pour un PNJ, ses PV courants ;
    - les boutons « Lancer le combat » et « Réinitialiser la carte » sont disponibles.
  - **En combat** :
    - au lancement, le DM saisit l'initiative des PJ ;
    - le carrousel d'initiative s'affiche en haut, comme sur la vue joueur, et les tokens cachés y figurent avec une icône ;
    - un clic sur un participant ouvre la fiche de cette instance précise, avec ses PV modifiables ;
    - le DM dispose d'un bouton « Suivant », d'un compteur de rounds, et des actions ajouter, retirer et réordonner.
  - **En bas à droite** :
    - la pastille des suggestions empile les 5 dernières entités ou sons détectés par leur nom ou leurs alias ;
    - un clic sur une entité l'ouvre au centre ;
    - pour un son, un clic déclenche le son ;
    - Il est possible de voir l'historique des 5 dernieres entites en cliquant sur une icone en haut a droite de la pastille
    - le bouton marche/pause du micro reste toujours visible.
  - **Onglet Soundbox** :
    - des cases portant le nom de chaque son, une recherche dynamique et un filtre par tags ;
    - plusieurs sons peuvent jouer en même temps, avec un volume global, un volume par son et un bouton « Tout arrêter » ;
    - le son sort uniquement de la DM view.
  - **Envoi aux joueurs** :
    - chaque battlemap ou illustration a un bouton « Envoyer » ;
    - une illustration part soit en mode **scène**, en plein écran, soit en mode **fenêtre**, dans une fenêtre flottante.
- **Player view**
  - Une page plein écran sans interaction pour les joueurs, projetée depuis un autre onglet de la machine du DM.
  - Elle affiche la couverture de la campagne quand rien n'est envoyé. Sinon, elle affiche la battlemap ou l'illustration envoyée.
  - Sur une battlemap, elle montre la grille, les tokens visibles et leurs badges, ainsi que l'état approximatif des PNJ (indemne, blessé, en sang), sans les PV.
  - En combat, le carrousel n'affiche pas les tokens cachés.
  - Le DM peut ouvrir un onglet player view synchronisé où il agit lui-même :
    - il déplace les tokens ;
    - il déplace la fenêtre d'illustration flottante ;
    - il la redimensionne en gardant ses proportions ;
    - il zoome, dézoome et se déplace dans la scène, comme sur un canevas.

## Journeys

**1. Reprendre la campagne actuelle**
1. Le DM crée sa campagne et téléverse l'illustration de couverture.
2. Il colle ses notes en texte libre, ou les dicte dans l'outil. L'IA propose des PNJ, des lieux et des événements, et il valide la liste en lot.
3. Il saisit ses joueurs et importe chaque PJ par son URL D&D Beyond. Il ajoute des alias et recadre le portrait pour le token.
4. Il complète à la main ce qui manque, par exemple « La Taverne du Sanglier » avec l'alias « la taverne ».

**2. Préparer la prochaine séance à l'oral**
1. Le DM crée la séance suivante et sélectionne les PJ présents. Il relit en tête le résumé de la précédente et ses fils ouverts.
2. Il clique sur « Brief oral » et raconte la séance pendant quelques minutes.
3. L'application crée trois scènes ordonnées, liées à des lieux et à des PNJ. Elle met à jour un PNJ existant, et le bouton « Mis à jour » apparaît. Elle ajoute deux PNJ et un lieu manquants à la liste « à créer ».
4. Il ouvre le diff du PNJ mis à jour et annule l'un des changements.
5. Il crée le premier PNJ manquant depuis le brief. Pour le second, il choisit « Affiner » et dicte trois phrases de plus. L'IA propose un background et un statblock crédible.
6. Il téléverse une battlemap et en règle la grille : taille, décalage, orientation, puis couleur et opacité.
7. Il ajoute quelques sons tagués à la soundbox, dont « porte qui grince ».
8. Il associe une instance de la battlemap à la scène 3. Il y place les PJ, trois bandits avec « × 3 », et un chef bandit caché.

**3. Faire rouler la séance**
1. Le DM lance la séance. Il ouvre la DM view, puis la player view dans un second onglet, qu'il projette. La player view montre la couverture de la campagne.
2. Il active le micro et dit : « Vous entrez dans la taverne. » « La Taverne du Sanglier » apparaît en bas à droite. Il clique dessus et la fiche s'ouvre au centre.
3. Il envoie l'illustration du tavernier en mode fenêtre. Dans son onglet player view, il la déplace et la redimensionne. Il envoie ensuite la salle en mode scène.
4. Il dit « la porte grince ». Le son apparaît dans la pastille et il le déclenche, par-dessus un bruit de foule lancé depuis la soundbox.
5. Les joueurs évoquent Morlox, un PNJ de la séance précédente. Morlox apparaît en bas à droite et sa fiche s'ouvre au centre d'un clic. Le DM le glisse depuis le panneau gauche sur la battlemap de la scène 3.

**4. Mener un combat**
1. Le DM envoie aux joueurs l'instance de battlemap de la scène 3 : Bandit 1, Bandit 2, Bandit 3, Chef bandit 1 (caché), Morlox et les PJ.
2. Il fait glisser un PJ : le compteur affiche « 4 hex · 20 pi ». Le token se fixe et apparaît au même endroit sur la player view.
3. Il lance le combat. Les PNJ tirent d20 + DEX, et il saisit l'initiative des PJ. Le carrousel apparaît sur les deux vues ; le chef caché ne figure que sur la vue DM.
4. Il clique sur Bandit 2 dans le carrousel et retire 7 PV. Côté joueurs, Bandit 2 passe à « blessé ». Il ajoute l'état « à terre ».
5. Bandit 2 tombe à 0 PV. Le token est grisé et marqué mort, puis sauté au tour suivant. Le DM le retire d'un clic.
6. Il révèle le chef, qui prend sa place dans le carrousel joueur. Il clique sur « Suivant », et le compteur de rounds avance.
7. Il ferme l'application en plein combat. Au retour, la carte et le combat sont intacts.

**5. Après la séance**
1. Le DM met le micro en pause et clôt la séance. Le journal transcrit est conservé. Un résumé est généré avec le modèle de résumé configuré, et le DM le retouche.
2. Les PJ montent de niveau. Il clique sur « Resynchroniser » : les stats, les PV et les sorts sont mis à jour, et les alias, les notes et le token sont conservés.
3. Il exporte la campagne en archive.

## Data

- **Campagne** : nom, type (campagne ou oneshot), illustration de couverture.
- **Séance** : campagne, titre, ordre, PJ présents, brief (texte), journal de transcription horodaté, résumé modifiable, entités mentionnées, fils ouverts.
- **Scène** : séance, ordre, titre, notes, lieux, personnages, événements, médias et instance de battlemap liés.
- **Joueur** : nom.
- **Personnage** :
  - identité : nom, alias, PJ ou PNJ, joueur associé, description ;
  - aide IA : prompt initial, background ;
  - fiche : caractéristiques, CA, PV max, sorts et capacités (nom et description) ;
  - table : portrait, recadrage ou image de token, notes ;
  - D&D Beyond : URL, date de dernière synchronisation.
- **Lieu** : nom, alias, description, illustration associée.
- **Événement** : nom, alias, description.
- **Entité à créer** : type (PNJ, lieu ou événement), nom, extrait du brief, séance d'origine, description affinée.
- **Note importée** : campagne, texte brut, date, propositions extraites et leur statut.
- **Version d'entité** : entité, date, origine (IA, manuelle ou D&D Beyond), contenu avant et après, statut vu ou non vu.
- **Média** : campagne, type (battlemap ou illustration), titre, fichier.
- **Réglage de grille** : battlemap, taille d'hex, décalage, orientation, couleur, opacité.
- **Instance de battlemap** : battlemap, séance, scène.
- **Instance de token** : instance de battlemap, personnage, numéro et nom affiché, position en hex, visible ou caché, PV courants pour un PNJ, états, mort.
- **Combat** : instance de battlemap, round, participant courant, ordre des participants (instance, initiative, DEX).
- **Son** : campagne, nom, fichier, tags, volume propre.
- **Diffusion** : contenu affiché sur la player view (couverture, battlemap ou illustration), mode (scène ou fenêtre), position et taille de la fenêtre, zoom et cadrage.
- **Paramètres IA** : fournisseur, URL de l'endpoint, modèle de génération, modèle de résumé, clé.
- **Vocabulaire de détection** : dérivé automatiquement des noms et alias des entités et des noms des sons, sans stockage séparé.

## Left out

- **Suggestions contextuelles d'entités et de sons**, par exemple deviner un lieu ou proposer un son d'ambiance sans qu'on le nomme. Le PoC se limite aux noms et alias, avec correspondance approximative : c'est fiable et local. La compréhension du contexte viendra dans une version ultérieure.
- **Ambiances en boucle et musique** : la première version ne joue que des effets ponctuels.
- **Sauvegarde automatique** : l'export et l'import manuels par archive suffisent à protéger la campagne en cours.
- **Brouillard de guerre, lignes de vue, obstacles dans le calcul de distance, éditeur de battlemaps** : l'interrupteur visible/caché et la distance en ligne droite couvrent le besoin de la table, et les cartes sont préconstruites.
- **Suivi des PV des PJ** : les joueurs gèrent leurs PV sur leur propre fiche.
- **Base de sorts du SRD** : les sorts restent une liste nom + description saisie ou importée.
- **Comptes, plusieurs DM, joueurs à distance** : un seul DM sur une seule machine, avec la player view projetée depuis un onglet.
- **Fiches de personnage complètes et autres intégrations** : les fiches restent simples (stats, PV, sorts), et D&D Beyond est la seule source externe.
- **Transcription dans le cloud** : l'audio reste local. Phonon sera exploré pour la transcription.

## Proof

- **Temps de préparation divisé par deux.** Le DM chronomètre la préparation d'une séance avec sa méthode actuelle. Il chronomètre ensuite la séance suivante préparée dans ForgeDesMondes, à partir du brief oral, des entités générées et du résumé de la séance précédente. Le second temps doit être au plus la moitié du premier.
- **Reprise de la campagne actuelle.** Les notes existantes, les PJ D&D Beyond et les PNJ clés sont dans l'application. Le DM prépare les séances suivantes sans autre outil.
- **Brief oral.** Sur un brief réel :
  - les PNJ, lieux et événements existants cités sont reconnus ;
  - les manquants apparaissent dans la liste « à créer » ;
  - chaque modification automatique peut être consultée en diff, puis annulée.
- **Création assistée.** Un PNJ dicté à l'oral reçoit un background cohérent avec son prompt et un statblock que le DM juge jouable tel quel. Cela fonctionne aussi bien avec l'API cloud qu'avec l'endpoint opencode du serveur maison.
- **Suggestions en direct.** Pendant une scène jouée à voix haute en français, chaque entité ou son nommé par son nom ou un alias apparaît en quelques secondes. Un clic ouvre l'entité au centre, ou déclenche le son. Aucun audio ne quitte la machine.
- **Battlemap.** Sur trois trajets vérifiés à la main, dont un en diagonale, le compteur donne le bon nombre d'hex et de pieds. Un token déposé apparaît au même endroit sur les deux vues en moins d'une seconde.
- **Combat.** Un combat complet se déroule sur les deux vues, avec des instances multiples, un PNJ caché puis révélé, et un PNJ tué et sauté. Il survit à une fermeture de l'application en cours de route.
- **Séparation des vues.** La player view ne montre jamais les notes, les PV des PNJ ni les tokens cachés, et le son ne sort que de la DM view.
- **Verdict à la table.** Le DM joue une séance complète de sa campagne actuelle dans ForgeDesMondes et répond « oui » à la question « Est-ce que ça m'a aidé à faire rouler la séance ? ».

## Decisions

### General

- **Mono-utilisateur ou multi-DM** — (a) Un seul DM, pas de comptes pour le PoC : tout tourne pour toi seul.
  (asked: L'application sert-elle uniquement toi (un seul DM, usage local/personnel), ou doit-elle gérer plusieurs DM avec comptes et authentification ? Choix : (a) un seul DM, pas de comptes ; (b) plusieurs DM avec comptes séparés.)
- **Où tournent les deux vues** — (a) Même machine, deux fenêtres synchronisées localement : c'est le cas de table classique et le plus simple pour le PoC.
  (asked: La player view est-elle affichée sur un second écran/projecteur branché sur ton ordinateur (même machine, deux fenêtres), ou chaque joueur l'ouvre-t-il sur son propre appareil via un lien réseau ? Choix : (a) même machine, deux fenêtres ; (b) appareils distants via le réseau local ; (c) appareils distants via Internet.)
- **Périmètre du PoC** — Tout sauf les suggestions IA de sons : préparation, DM view, player view, battlemap hex + tokens + distance, combat/initiative, soundbox manuelle, et micro + suggestions d'éléments (c'est le cœur différenciant). Il faut aussi une aide a la creation des entité, comme la generation des statblock credible des PNJ, une proposition de leurs background par rapport a leur prompt initial... D'ailleur le prompt initial doit pouvoir etre comté à l'oral quand je cree un personnage. Si je cree une session, je dois pouvoir donner un brief a l'oral ca detecte les PNJ deja existant (les mets a jour si necessaire) ou manquant (dans ce cas les mets dans une liste a creer, si je demande de les creer je peux soit raffiner le descriptif avant la proposition, soit il doit se baser sur ce que j'ai decris dans la session)
  (asked: Quelles parties doivent absolument être dans la preuve de concept ? Choix : préparation de campagne, DM view avec liste des éléments, envoi vers la player view, battlemap hexagonale avec tokens et calcul de distance, combat avec carrousel d'initiative, soundbox, écoute micro + suggestions IA d'éléments, suggestions IA de sons.)
- **Hors périmètre explicite** — Tous exclus du PoC sauf fiche de personnage et import d&d beyond. Les personnages restent des fiches descriptives simples avec stats, PV et sort. L'import depuis D&Dbeyond des personnage joueurs doit se faire egalement en utilisant une URL.
  (asked: Lesquels de ces éléments sont explicitement exclus : fiches de personnage complètes (stats, PV, sorts), lancers de dés intégrés, brouillard de guerre, import depuis D&D Beyond/Foundry, génération d'images ou de battlemaps par IA, usage hors ligne, mobile ?)
- **Forme de l'application** — (a) Web app locale dans le navigateur, deux fenêtres synchronisées : c'est le plus simple à livrer, et le micro et l'audio sont bien supportés par le navigateur.
  (asked: Sous quelle forme l'application tourne-t-elle sur ta machine ? (a) Application web servie en local (localhost) et ouverte dans le navigateur, la player view étant une seconde fenêtre que tu glisses sur l'écran ou la TV des joueurs. (b) Application desktop installée (type Electron/Tauri) qui ouvre elle-même ses deux fenêtres. (c) Autre.)
- **Preuve que le PoC fonctionne** — Je l'utilise pour poursuivre ma campagne actuel et preparer les prochaines sessions, avec une reduction de 50% sur la preparation d'une session.
  (asked: Qu'est-ce qui prouvera que le PoC est réussi ? (a) Tu prépares un vrai oneshot avec l'outil (entités créées à l'oral, statblocks générés, battlemaps) puis tu le fais jouer en entier à ta table sans revenir à tes anciens outils. (b) Une démo scénarisée de chaque fonctionnalité, sans vraie partie. (c) Des critères chiffrés, par exemple : suggestion affichée moins de X secondes après qu'un lieu est nommé, et Y % des entités citées correctement détectées.)
- **Reprise de la campagne actuelle** — Saisie manuelle, brief oral et import de notes en texte libre.
  (asked: Le PoC est jugé sur ta campagne en cours. Ce qui existe déjà (PNJ, lieux, séances passées, notes) doit donc entrer dans l'application. Trois options : (a) saisie manuelle et brief oral uniquement ; (b) import de notes en texte libre (collage ou fichiers Markdown/texte) que l'IA découpe en entités (PNJ, lieux, événements, séances passées), modifiées automatiquement avec historique comme les autres propositions ; (c) import structuré depuis un outil précis (World Anvil, Notion, Obsidian…).)
- **Sauvegarde des données** — (b) Un export/import manuel par campagne en une archive. C'est simple, ça protège ta campagne en cours et ça permet de la déplacer entre machines. La sauvegarde automatique (c) viendra plus tard.
  (asked: Toutes les données (campagnes, médias, journaux de séance, historique des modifications IA) restent locales. Que se passe-t-il pour la sauvegarde ? (a) Rien de spécial, les données restent dans le dossier de l'application ; (b) un export/import d'une campagne complète en archive (données + médias), lancé à la main ; (c) une sauvegarde automatique vers un emplacement choisi, par exemple ton serveur maison.)

### Préparation de campagne

- **Campagne vs oneshot** — (a) Même modèle : un oneshot est une campagne à une séance ; plusieurs campagnes indépendantes possibles.
  (asked: Un oneshot est-il simplement une campagne d'une seule séance, ou un type distinct avec un comportement différent ? Et peux-tu avoir plusieurs campagnes en parallèle, chacune avec ses propres éléments ? Choix : (a) même modèle, une campagne contient 1..n séances, plusieurs campagnes possibles ; (b) deux types distincts.)
- **Joueurs vs personnages** — (a) Joueur et personnage distincts ; un personnage est PJ (lié à un joueur) ou PNJ ; un joueur peut avoir plusieurs personnages.
  (asked: Distingues-tu le joueur (la personne réelle) de son personnage, et un joueur peut-il avoir plusieurs personnages dans une campagne ? Les PNJ sont-ils des 'personnages' sans joueur ? Choix : (a) joueur = personne, personnage = PJ ou PNJ, PJ rattaché à un joueur ; (b) seulement des personnages, le joueur est un simple champ texte.)
- **Contenu d'une séance** — (a) Une liste ordonnée de scènes ou événements, chacune liée à des lieux, des PNJ et des médias, avec des notes libres. La DM view peut alors suivre la séance scène par scène, et le brief oral peut produire directement cette structure.
  (asked: Que contient une séance ? (a) Une liste ordonnée de scènes ou événements, chacun rattaché à des lieux, des PNJ et des médias (battlemaps, illustrations, sons), plus des notes libres. (b) Un simple regroupement d'entités existantes, sans ordre ni scènes. (c) Un texte libre de préparation où les entités sont mentionnées et liées automatiquement.)
- **Synchronisation D&D Beyond** — (b) Un import avec un bouton « Resynchroniser » manuel. Les PJ montent de niveau entre les séances : la resynchronisation met à jour les stats, PV et sorts sans perdre les alias, les notes et le token définis localement.
  (asked: L'import d'un PJ depuis une URL D&D Beyond est-il : (a) un import unique, la fiche devenant ensuite locale et éditable ; (b) un import avec un bouton « Resynchroniser » qui écrase les stats, PV et sorts depuis D&D Beyond tout en gardant les champs locaux (alias, notes, token) ; (c) une synchronisation automatique à chaque ouverture de séance ?)
- **Traitement des notes importées** — (c) Une extraction par l'IA vers une liste de propositions à valider en lot, et la note brute est conservée en plus. Pour un import massif, une validation préalable évite de polluer la campagne de doublons, contrairement aux mises à jour au fil de l'eau.
  (asked: Quand tu importes des notes en texte libre de ta campagne actuelle, que fait l'application ? (a) Elle les stocke comme une note brute de la campagne ; (b) l'IA en extrait les entités (PNJ, lieux, événements, séances passées), crée les nouvelles et met à jour les existantes, avec l'historique et le diff habituels ; (c) comme (b), mais tout passe d'abord par une liste à valider avant la création.)
- **Format des stats et des sorts** — (b) Les champs chiffrés sont structurés, car ils servent à l'initiative, aux PV en combat et à l'import D&D Beyond. Les sorts et les capacités sont une liste nom + description. La base de sorts du SRD (c) viendra plus tard.
  (asked: Comment les statblocks et les sorts sont-ils représentés sur une fiche ? (a) En texte libre ; (b) les caractéristiques, la CA, les PV et la vitesse en champs structurés, et les sorts et capacités en liste de noms avec une courte description ; (c) tout est structuré et lié à une base de sorts du SRD.)

### Micro et suggestions IA

- **Traitement de l'audio** — Local uniquement. Explorer solution Phonon
  (asked: L'audio de ta table peut-il être envoyé à un service cloud de transcription/IA (ex. API Claude + un service de speech-to-text), ou doit-il rester sur ta machine ? L'audio et les transcriptions sont-ils conservés après la session ? Choix : (a) cloud, rien de conservé ; (b) cloud, transcription conservée comme journal de session ; (c) local uniquement.)
- **Déclenchement des suggestions** — (a) Noms et alias avec correspondance approximative, chaque entité pouvant recevoir des alias. Les 5 dernières suggestions restent empilées et les plus anciennes sortent. Le (b) attend une version ultérieure.
  (asked: Qu'est-ce qui fait apparaître un élément dans la fenêtre en bas à droite ? (a) Le nom de l'entité, ou un de ses alias, est reconnu dans la transcription, avec une tolérance aux mauvaises transcriptions des noms fantaisistes. (b) Une compréhension du contexte par un modèle de langage, par exemple « la taverne de tout à l'heure » fait remonter l'entité correspondante. (c) Les deux. Et que deviennent les suggestions : une pile des N dernières, ou une disparition après X secondes ?)
- **Activation et transcription** — (b) Un bouton marche/pause visible, et (ii) une transcription conservée comme journal de séance stocké localement. Ce journal alimente la préparation de la séance suivante, on genere egalement un resumé pour une consultation rapide, ce qui sert directement l'objectif de réduire de 50 % le temps de préparation.
  (asked: Pendant la séance, le micro est-il (a) toujours actif dès l'ouverture de la DM view ; (b) activé par un bouton marche/pause bien visible, pour couper pendant les apartés ? La transcription est-elle (i) jetée après détection ; (ii) conservée comme journal de séance consultable, et servant à préparer la séance suivante ?)
- **Langue et noms propres** — (b) Français, avec un vocabulaire injecté automatiquement depuis les noms et alias de la campagne. C'est ce qui rend fiable la détection des entités à l'oral, le cœur différenciant du PoC.
  (asked: La table joue en français, mais les noms propres sont souvent inventés ou anglais (sorts, monstres, lieux). Trois options : (a) transcription en français seulement, et ce sont les alias qui rattrapent les erreurs ; (b) français, avec un vocabulaire personnalisé alimenté automatiquement par les noms et alias des entités de la campagne pour améliorer leur reconnaissance ; (c) détection automatique de la langue, français ou anglais.)
- **Résumé et suite de la séance** — (b) Un résumé généré à la clôture et modifiable, qui apparaît en tête de la préparation suivante avec les entités mentionnées et les fils ouverts. C'est le levier direct pour réduire de 50 % le temps de préparation.
  (asked: Quand et comment le résumé de séance est-il produit, et comment sert-il ensuite ? (a) Il est généré à la clôture de la séance et reste en lecture seule ; (b) il est généré à la clôture, il est modifiable, et il est proposé au début de la préparation de la séance suivante, avec la liste des entités mentionnées et des fils narratifs ouverts ; (c) il est mis à jour en continu pendant la séance.)

### Médias

- **Source des médias** — (a) Téléversés et stockés par l'application, rattachés à une campagne.
  (asked: Comment les battlemaps, illustrations, images de tokens et sons arrivent-ils dans l'application ? Choix : (a) téléversés par toi et stockés par l'application ; (b) référencés depuis un dossier local de ton ordinateur ; (c) liens externes (URL, YouTube, etc.).)
- **Image des tokens** — (b) Un recadrage du portrait du personnage par défaut, remplaçable par une image dédiée. À défaut de portrait, une pastille de couleur avec les initiales et le numéro d'instance.
  (asked: D'où vient l'image d'un token ? (a) Une image téléversée dédiée au token ; (b) un recadrage circulaire du portrait ou de l'illustration du personnage, ou une image dédiée si tu le souhaites ; (c) aucune image, une pastille de couleur avec les initiales.)

### Aide à la création IA

- **Où tourne l'IA générative** — (b) API cloud pour la génération, qui ne reçoit que du texte, tandis que transcription et détection des suggestions restent locales. C'est le meilleur moyen d'obtenir des statblocks crédibles dans le PoC. Ca doit etre parametrable, j'ai un serveur a la maison qui delivre une api endpoint opencode, je veux pouvoir l'utiliser
  (asked: La génération (statblocks, backgrounds, analyse du brief de séance pour détecter les PNJ existants ou manquants) demande un modèle de langage. L'audio reste local, c'est décidé. Pour la génération : (a) un modèle local aussi (Ollama ou équivalent), entièrement hors ligne mais de qualité moindre et exigeant en matériel ; (b) une API cloud (Claude, par exemple) qui reçoit seulement le texte transcrit, jamais l'audio ; (c) un modèle local par défaut et le cloud en option.)
- **Édition des règles de référence** — b pour le moment
  (asked: Sur quelle édition se basent les statblocks générés (CR, PV, CA, actions, sorts) ? (a) D&D 5e 2014 (SRD 5.1). (b) D&D 2024 (SRD 5.2). (c) Un choix par campagne.)
- **Validation des propositions IA** — b, elle se modifie automatiquement et un historique me permet de revenir en arriere si il y a lieu. J'ai une notification sous forme d'un bouton updated, si je clique dessus je peux voir le diff et decider de revert
  (asked: Quand le brief oral d'une séance détecte des PNJ existants à mettre à jour, comment les changements s'appliquent-ils ? (a) Chaque mise à jour est présentée comme un diff (avant/après), que tu acceptes, modifies ou rejettes une par une. (b) Les changements s'appliquent automatiquement avec un historique pour annuler. (c) L'IA se contente d'ajouter une note, sans jamais modifier la fiche. La même règle vaut pour les statblocks et backgrounds générés.)
- **Portée du brief oral de séance** — (c) Les PNJ, les lieux, les événements, et la structure en scènes. Les entités manquantes vont dans la même liste « à créer », avec le même choix : raffiner la description, ou s'appuyer sur le brief.
  (asked: Le brief oral d'une séance détecte les PNJ existants ou manquants. Que doit-il extraire d'autre ? (a) Seulement les PNJ ; (b) les PNJ, les lieux et les événements, existants ou à créer ; (c) tout cela, et en plus la liste ordonnée des scènes de la séance, déjà liées aux entités.)

### Battlemap et combat

- **Calage de la grille hexagonale** — (a) Un réglage manuel par battlemap (taille, décalage, orientation), enregistré avec la carte, et 1 hex = 5 pieds. je veux pouvoir changer la couleur de la grille egalement ainsi que son opacité
  (asked: Les battlemaps téléversées ont parfois déjà une grille dessinée, et une échelle propre. Comment la grille hex se pose-t-elle dessus ? (a) Pour chaque battlemap, tu règles la taille de l'hexagone, le décalage et l'orientation (pointe en haut ou plat en haut) pour l'aligner sur l'image. (b) Une grille fixe, identique pour toutes les cartes. (c) Une détection automatique de la grille existante.)
- **Calcul de la distance au drag** — (a) La distance en ligne droite en hex, affichée en hex et en pieds (par exemple « 6 hex · 30 pi »), sans gestion des obstacles dans le PoC.
  (asked: Pendant le glisser-déposer, le nombre vert compte : (a) la distance en hex en ligne droite entre la case d'origine et la case sous le curseur, sans tenir compte des obstacles ; (b) un chemin qui contourne les murs et les obstacles, ce qui suppose de les dessiner sur la carte ; (c) le chemin réellement tracé par le curseur, case par case. Affichage en hex seulement, ou en hex et en pieds ?)
- **Initiative et déroulé des tours** — Un d20 + modificateur de DEX pour les PNJ. Les égalités se départagent par la DEX, puis au hasard. Le tour passe sur ton bouton « Suivant », avec un compteur de rounds. Tu peux ajouter ou retirer des participants et réordonner à la main, et le carrousel se met à jour sur les deux vues.
  (asked: Quand tu lances un combat : (a) l'initiative d'un PNJ est un d20 pur, ou un d20 + modificateur de DEX tiré de son statblock ? (b) Les égalités se départagent par la DEX, par le hasard ou à la main ? (c) Le tour suivant passe sur ton clic « Suivant », ou autrement ? (d) Peux-tu ajouter ou retirer des participants en cours de combat (renforts, morts) et réordonner le carrousel à la main ?)
- **Suivi des PV et états en combat** — (b) Suivi des PV courants côté DM, avec les états en badges sur le token et dans le carrousel. Les joueurs voient un état approximatif des PNJ (indemne, blessé, en sang). On ne suit pas les PV des PJ
  (asked: Pendant un combat, l'application suit-elle (a) rien : seulement l'ordre d'initiative ; (b) les PV courants de chaque participant, modifiables depuis le carrousel côté DM, avec mort ou inconscience à 0 ; (c) PV plus états (empoisonné, à terre, etc.) affichés sur les tokens ? Et que voient les joueurs des PNJ : les PV exacts, un état approximatif (indemne, blessé, en sang), ou rien ?)
- **Instances multiples sur la carte** — (c) Glisser-déposer pour une instance, et un champ « × N » pour en placer plusieurs, toutes numérotées automatiquement et renommables. À 0 PV, le token est grisé et marqué mort, il est sauté dans l'initiative et le DM peut le retirer d'un clic.
  (asked: Un même PNJ ou monstre peut apparaître plusieurs fois sur une carte. Comment crée-t-on ces instances, et comment se comportent-elles ? (a) Glisser la fiche sur la carte crée une instance numérotée automatiquement (Gobelin 1, Gobelin 2…), avec ses propres PV issus du statblock et ses propres états, tandis que la fiche reste partagée ; (b) on choisit un nombre d'instances à placer d'un coup ; (c) les deux. Et à 0 PV : le token est marqué comme mort et reste sur la carte, ou il est retiré.)
- **Tokens cachés et carrousel** — (b) Il est absent du carrousel joueur tant qu'il est caché, mais toujours visible dans le carrousel DM, signalé par une icône. Le révéler le fait apparaître à sa place dans l'ordre d'initiative.
  (asked: Un token peut être caché sur la vue joueur. Pendant un combat, un participant caché (une embuscade, par exemple) : (a) apparaît quand même dans le carrousel joueur ; (b) n'apparaît pas dans le carrousel joueur tant qu'il est caché, et apparaît dès qu'on le révèle ; (c) apparaît comme « ??? ».)
- **Persistance de l'état de la carte** — (b) Tout l'état est conservé par battlemap, combat en cours compris. Un bouton « Réinitialiser la carte » permet de repartir à zéro. Arrêter une session au milieu d'un combat arrive souvent à une vraie table.
  (asked: Quand on quitte une battlemap ou qu'une session se termine au milieu d'un combat, que garde-t-on ? (a) Rien, chaque ouverture repart d'une carte vide ; (b) on garde la position des tokens, leur visibilité, leurs PV et leurs états par battlemap, et un combat en cours (ordre, round, tour actuel) peut être repris à la séance suivante ; (c) seulement la position des tokens.)

### DM view

- **Zone principale de la DM view** — Une fiche en lecture optimisée pour la table avec bouton d'edition. Par contre en mode combat il faut faire attention, il peut y avoir plusieurs fois le meme monstre/pnj sur le battlemap, dans ce cas je dois avoir la possibilité de selectionner une instance ou une autre. Le caroussel de combat est disponible sur la vue MJ je peux naviguer entre les instances des personnage en cliquant dessus directement, les PV restants sont editable en mode combat par le MJ.
  (asked: Quand tu ouvres un élément depuis le panneau de gauche ou une suggestion, que fait la zone principale ? (a) Elle affiche la fiche en lecture, optimisée pour la table (statblock lisible, background, notes), avec un bouton pour passer en édition. (b) Elle affiche directement le formulaire éditable. (c) Elle ouvre des onglets multiples pour garder plusieurs fiches ouvertes. Le panneau de gauche est-il organisé par type d'entité ou par la séance en cours d'abord ?)
- **Panneau gauche des éléments** — (b) La séance en cours en tête, scène par scène avec ses lieux, PNJ et médias liés, puis tout le reste de la campagne par type, avec une recherche approximative qui utilise aussi les alias. C'est ce qui permet de suivre la séance sans chercher.
  (asked: Comment sont organisés les éléments de campagne dans le panneau gauche pendant le jeu ? (a) Par type (PJ, PNJ, lieux, événements, médias) avec une recherche ; (b) d'abord la séance en cours, scène par scène, avec ses éléments liés, puis le reste de la campagne par type ; (c) une seule liste plate avec recherche.)
- **Lancement d'une séance en direct** — a
  (asked: Que se passe-t-il quand tu passes en mode jeu ? (a) Tu choisis une séance préparée et tu la « démarres » : le journal, la scène courante et l'état sont rattachés à cette séance ; (b) le mode jeu est libre, sans séance associée ; (c) les deux, avec une séance « improvisée » créée automatiquement si aucune n'est choisie.)

### Vue joueur

- **Visibilité sur la vue joueur** — (b) Un interrupteur visible/caché par token, sans brouillard de guerre dans le PoC. Quand rien n'est envoyé, la vue joueur affiche l'illustration de couverture de la campagne. La vue joueur peut aussi recevoir des illustrations (de personnage, de lieux...). envoyé depuis la vue MJ. Le MJ peut envoyer les illustration en mode scene, ca prend la vue joueur ou en mode fenetre, une fenetre volante que je peux deplacer et resize( via un drag and drop en gardant les proportion) contient l'illustration. Le MJ a aussi la possibilité d'avoir la vue joueur sur un onglet qui est synchronisé avec les autres onglet, lui permettant de bouger les token illustrations etc.
  (asked: Sur une battlemap envoyée aux joueurs, que voient-ils ? (a) Tous les tokens posés sur la carte. (b) Chaque token a un interrupteur visible/caché : les PNJ en embuscade restent visibles seulement sur la DM view. (c) Option (b) plus un brouillard de guerre qu'on révèle zone par zone. Et quand rien n'est envoyé, la vue joueur affiche-t-elle un écran neutre (logo, illustration de campagne) ?)

### Soundbox

- **Comportement des sons** — Des effets ponctuel uniquement dans la premiere version avec un classement par tag. la visualisation c'est un pannel avec des box portant le nom du son, une barre de recherche qui me permet de rechercher dynamiquement,
  (asked: Comment se jouent les sons de la soundbox ? (a) Un seul son à la fois. (b) Deux familles : des ambiances en boucle (une seule active, avec fondu enchaîné au changement) et des effets ponctuels qui se superposent, chacun avec son volume. (c) Une superposition libre de n'importe quels sons, tous en boucle possible. Les sons sont-ils classés par tags (taverne, combat, forêt) pour préparer les futures suggestions IA ?)
- **Lecture simultanée et volume** — (b) Plusieurs sons en même temps, un volume global, un volume par son et « Tout arrêter ». Le son sort uniquement depuis la DM view : sur une seule machine, une seule sortie audio suffit et on évite l'écho. La vue joueur a pour objectif d'etre egalement projeté par la machine du DM via un autre onglet.
  (asked: Pour les effets ponctuels : (a) un seul son à la fois, et un nouveau son coupe le précédent ; (b) plusieurs sons peuvent jouer en même temps, avec un volume global, un volume par son réglé à l'import et un bouton « Tout arrêter » ; (c) la même chose que (b), sans volume par son. Par ailleurs, les sons joués se font-ils entendre sur la vue joueur ou seulement sur la sortie audio de la DM view ?)

## Rules

- **R1 Plusieurs campagnes et oneshots** (business) — Le DM voit ses campagnes et oneshots avec leur illustration de couverture et en crée de nouveaux. Un oneshot est une campagne à une seule séance, et plusieurs campagnes indépendantes coexistent sans compte utilisateur. — criterion: given deux campagnes et un oneshot; when le DM ouvre l'accueil; then les trois apparaissent avec leur couverture, le oneshot n'a qu'une séance, et aucun compte n'est demandé. — on `mock/pages/campagnes.html`, `CampaignList`
- **R2 Export et import d'une campagne en archive** (business) — Une campagne s'exporte en une archive et une archive s'importe, en copie si la campagne existe déjà. Il n'y a pas de sauvegarde automatique, et la date du dernier export est rappelée. — criterion: given une campagne exportée puis importée alors qu'elle existe déjà; when l'import se termine; then une copie distincte est créée, l'originale reste intacte et la date du dernier export est affichée. — on `mock/pages/campagnes.html`, `mock/pages/campagne.html`, `CampaignList`, `CampaignOverview`
- **R3 Fournisseur de génération paramétrable : API ou binaire** (journey) — Le DM choisit entre deux modes de connexion au fournisseur de génération. En mode « API », il saisit l'URL de l'API, qu'il s'agisse d'une API cloud ou de son serveur maison. Le système détecte seul le type d'API et s'y connecte, et le DM complète le modèle de génération, le modèle de résumé et une clé éventuelle. En mode « Binaire », il choisit dans une liste déroulante l'un des deux binaires proposés pour le moment : opencode ou Claude Code. Dans les deux modes, un bouton « Tester la connexion » vérifie le réglage. Un rappel indique que seul du texte part vers ce fournisseur. — criterion: given l'écran des paramètres IA, d'abord en mode « API » avec l'URL du serveur maison saisie, puis en mode « Binaire »; when le DM teste la connexion en mode « API », puis passe en mode « Binaire », ouvre la liste déroulante, choisit « Claude Code » et teste à nouveau la connexion; then le type d'API détecté et le résultat du premier test s'affichent, la liste déroulante propose exactement « opencode » et « Claude Code », le résultat du second test s'affiche, et le rappel « seul du texte part vers ce fournisseur » reste visible — on `mock/pages/parametres-ia.html`, `AiSettings`
- **R4 Joueurs et personnages distincts** (business) — L'application conserve les joueurs, les personnages, les lieux et les événements d'une campagne, et les présente par onglets. Un personnage est un PJ lié à un joueur ou un PNJ, avec une fiche simple (caractéristiques, CA, PV, sorts et actions), un token et des notes. — criterion: given un PJ lié à un joueur et un PNJ; when le DM ouvre les onglets de la campagne; then chacun apparaît dans le bon onglet avec sa fiche (caractéristiques, CA, PV, sorts, actions), son token et ses notes. — on `mock/pages/campagne.html`, `mock/pages/fiche-entite.html`, `CampaignElements`, `EntityEditor`
- **R5 Reprise depuis des notes en texte libre** (business) — Le DM colle des notes en texte libre, éventuellement issues d'une transcription faite dans l'outil, et l'IA en extrait des personnages, des lieux et des événements. Rien n'est créé avant que le DM ait validé la liste proposée. — criterion: given des notes collées; when l'IA propose personnages, lieux et événements; then aucune entité n'est créée tant que le DM n'a pas validé la liste, et seules les entités validées le sont. — on `mock/pages/campagne.html`, `NoteImport`
- **R6 Alias et vocabulaire de détection** (business) — Chaque entité peut recevoir des alias, par exemple « la taverne » pour « La Taverne du Sanglier ». Les noms et les alias de la campagne forment automatiquement le vocabulaire injecté dans la transcription locale en français. — criterion: given l'alias « la taverne » sur « La Taverne du Sanglier »; when la transcription démarre; then son vocabulaire contient les noms et les alias de toutes les entités de la campagne. — on `mock/pages/fiche-entite.html`, `mock/pages/dm-view.html`, `EntityEditor`, `SuggestionPill`, `ElementsPanel`
- **R7 Prompt initial dicté et création assistée** (journey) — Le DM raconte au micro le prompt initial d'un personnage. L'IA propose un background cohérent et, pour un PNJ, un statblock crédible et jouable tel quel. Cela fonctionne quel que soit le fournisseur configuré : une API, cloud ou serveur maison, ou un binaire, opencode ou Claude Code. — criterion: given un prompt initial dicté pour un PNJ; when le DM demande la proposition, successivement avec une API configurée et avec un binaire configuré; then un background cohérent avec le prompt et un statblock jouable sont proposés dans chacun des deux modes — on `mock/pages/fiche-entite.html`, `EntityEditor`
- **R8 Import et resynchronisation D&D Beyond** (business) — Un PJ s'importe par son URL D&D Beyond. Le bouton « Resynchroniser » met à jour les stats, les PV et les sorts après une montée de niveau, sans toucher aux alias, aux notes ni au token définis localement. — criterion: given un PJ importé de D&D Beyond avec alias, notes et token locaux; when le DM clique « Resynchroniser » après une montée de niveau; then stats, PV et sorts sont à jour et alias, notes et token sont inchangés. — on `mock/pages/fiche-entite.html`, `EntityEditor`
- **R9 Historique, diff et retour arrière** (business) — Toute modification par l'IA ou par D&D Beyond s'applique automatiquement et est versionnée, et un bouton « Mis à jour » le signale. Le DM consulte le diff, annule un changement précis ou revient à une version antérieure. — criterion: given une modification appliquée par l'IA ou par D&D Beyond; when le DM ouvre l'historique; then une nouvelle version existe avec son diff, et annuler ce changement ou revenir à une version antérieure restaure exactement l'état d'avant. — on `mock/pages/fiche-entite.html`, `mock/pages/preparation-seance.html`, `VersionHistory`, `OralBrief`
- **R10 PJ présents à la séance** (business) — Le DM coche les PJ présents à une séance et peut en ajouter ou en retirer plus tard. Seuls ces PJ sont proposés dans la DM view et sur les battlemaps. — criterion: given une séance avec deux PJ cochés présents sur quatre; when le DM ouvre la DM view ou une battlemap; then seuls ces deux PJ sont proposés, et un PJ ajouté plus tard l'est aussi. — on `mock/pages/campagne.html`, `mock/pages/preparation-seance.html`, `SessionList`, `SceneBoard`
- **R11 Brief oral découpé en scènes** (journey) — Un brief raconté au micro devient une liste ordonnée de scènes. Chaque scène est liée à des lieux, des PNJ, des événements et des médias, porte des notes libres, et peut être réordonnée ou complétée à la main. — criterion: given un brief raconté au micro; when il est traité; then une liste ordonnée de scènes apparaît, chacune liée à ses lieux, PNJ, événements et médias, réordonnable et modifiable à la main. — on `mock/pages/preparation-seance.html`, `OralBrief`, `SceneBoard`
- **R12 PNJ existants mis à jour, PNJ manquants à créer** (business) — Le brief reconnaît les PNJ existants cités et les met à jour avec leur diff, et ajoute les PNJ manquants à la liste « à créer ». Pour chacun, le DM choisit « Créer depuis le brief » ou « Affiner » à l'oral ou à l'écrit avant la proposition de background et de statblock. — criterion: given un brief citant un PNJ existant et un PNJ inconnu; when il est traité; then le PNJ existant est mis à jour avec son diff et le PNJ inconnu est ajouté à la liste « à créer », sans création automatique. — on `mock/pages/preparation-seance.html`, `mock/pages/campagne.html`, `OralBrief`, `ToCreateList`, `VersionHistory`
- **R13 Journal et résumé de la séance précédente** (journey) — Pendant la préparation, le résumé généré et le journal transcrit horodaté de la séance précédente sont consultables dans un panneau latéral. La liste des séances affiche aussi leur résumé. — criterion: given une séance précédente close; when le DM prépare la suivante; then son résumé et son journal horodaté sont consultables dans le panneau latéral et le résumé figure dans la liste des séances. — on `mock/pages/preparation-seance.html`, `mock/pages/campagne.html`, `PreviousSummary`, `SessionList`
- **R14 Médias téléversés et grille hexagonale réglée** (business) — Le DM téléverse des battlemaps, des illustrations et des tokens rattachés à la campagne. Pour chaque battlemap, il règle la taille, le décalage, l'orientation, la couleur et l'opacité de la grille avec un aperçu en direct, sur la règle 1 hex = 5 pieds. — criterion: given une battlemap téléversée; when le DM règle taille, décalage, orientation, couleur et opacité de la grille; then l'aperçu se met à jour en direct et un hex vaut toujours 5 pieds. — on `mock/pages/medias.html`, `MediaLibrary`, `GridSettings`
- **R15 Instance de battlemap dans une scène** (journey) — Une battlemap peut avoir une instance dans une scène de séance, sur laquelle le DM place à l'avance les PJ, les PNJ et les monstres. — criterion: given une scène liée à une battlemap; when le DM y place à l'avance PJ, PNJ et monstres; then l'instance de la scène les conserve à leur position, sans modifier la battlemap d'origine. — on `mock/pages/preparation-seance.html`, `SceneBoard`, `Battlemap`
- **R16 Soundbox d'effets tagués** (business) — Le DM téléverse des sons, les nomme, les tague et règle leur volume propre. En séance, il les retrouve par une recherche dynamique et un filtre par tags, en joue plusieurs à la fois avec un volume global, un volume par son et « Tout arrêter », et le son ne sort que de la DM view. — criterion: given trois sons tagués; when le DM filtre par tag et en joue deux à la fois; then chacun respecte son volume propre et le volume global, « Tout arrêter » coupe tout, et aucun son ne sort de la player view. — on `mock/pages/soundbox.html`, `mock/pages/dm-view.html`, `SoundLibrary`, `LiveSoundbox`
- **R17 Conduite de la séance depuis la DM view** (journey) — La DM view montre à gauche tous les éléments de la campagne, ceux de la séance en tête, et les scènes à suivre une à une. Au centre s'ouvre une fiche en lecture optimisée pour la table, avec un bouton d'édition, ou la battlemap de la scène. — criterion: given une séance en cours; when le DM ouvre la DM view; then les éléments de la séance sont en tête à gauche, les scènes défilent une à une, et un clic ouvre au centre la fiche en lecture ou la battlemap de la scène. — on `mock/pages/dm-view.html`, `ElementsPanel`, `TableSheet`, `Battlemap`, `LiveSessionBar`
- **R18 Suggestions du micro en direct** (business) — Un micro avec un bouton marche/pause toujours visible transcrit la séance en local, sans qu'aucun audio ne quitte la machine. Chaque entité ou son nommé par son nom ou son alias apparaît en quelques secondes dans une pile des 5 dernières suggestions. Un clic ouvre l'entité au centre, et un son se joue au second clic. — criterion: given le micro en marche et un PNJ nommé par son alias; when la phrase est prononcée; then il apparaît en quelques secondes dans la pile des 5 dernières suggestions, et aucun audio ne quitte la machine. — on `mock/pages/dm-view.html`, `SuggestionPill`
- **R19 Tokens glissés avec mesure de distance** (journey) — Le DM glisse-dépose les tokens, le curseur portant le token. Un compteur vert affiche la distance en ligne droite depuis la position de départ, par exemple « 6 hex · 30 pi ». Le token déposé apparaît au même endroit sur la player view en moins d'une seconde. — criterion: given un token glissé de 6 hex; when le DM le déplace; then le compteur affiche « 6 hex · 30 pi », et une fois déposé le token apparaît au même endroit sur la player view en moins d'une seconde. — on `mock/pages/dm-view.html`, `mock/pages/player-view.html`, `Battlemap`, `PlayerStage`
- **R20 Dépôt de personnages en une ou plusieurs instances** (business) — Un personnage se glisse sur la battlemap, depuis la colonne de gauche ou la fiche, en une instance ou en « × N ». Les instances sont numérotées automatiquement et renommables. — criterion: given un monstre glissé « × 3 » sur la battlemap; when il est déposé; then trois instances numérotées 1 à 3 apparaissent, chacune renommable sans toucher aux autres. — on `mock/pages/dm-view.html`, `mock/pages/preparation-seance.html`, `Battlemap`, `ElementsPanel`
- **R21 PV, états, mort et visibilité des tokens** (business) — Chaque token a un interrupteur visible/caché et des badges d'état. Chaque PNJ a des PV courants modifiables côté DM. À 0 PV, le token est grisé, marqué mort et sauté dans l'initiative, et le DM peut le retirer d'un clic. Les PV des PJ ne sont pas suivis. — criterion: given un PNJ dont les PV tombent à 0; when le DM valide; then son token est grisé, marqué mort et sauté dans l'initiative, et les PV des PJ ne sont jamais suivis. — on `mock/pages/dm-view.html`, `mock/pages/player-view.html`, `Battlemap`, `PlayerStage`
- **R22 Combat et carrousel d'initiative** (business) — Au lancement du combat, les PNJ tirent d20 + DEX, le DM saisit l'initiative des PJ, et les égalités se départagent par la DEX puis au hasard. Un carrousel à la Foundry apparaît sur les deux vues, avec « Suivant », un compteur de rounds, et l'ajout, le retrait et le réordonnancement des participants. Un clic sur un participant ouvre la fiche de cette instance précise. — criterion: given deux PNJ à égalité d'initiative; when le combat démarre; then ils sont départagés par la DEX puis au hasard, le carrousel apparaît sur les deux vues, et « Suivant » fait avancer le tour et le compteur de rounds. — on `mock/pages/dm-view.html`, `mock/pages/player-view.html`, `Battlemap`, `PlayerStage`
- **R23 État de la carte conservé** (business) — Tout l'état d'une battlemap, combat en cours compris, survit à la fermeture de l'application. Un bouton « Réinitialiser la carte » permet de repartir à zéro. — criterion: given un combat en cours sur une battlemap; when l'application est fermée puis rouverte; then positions, PV, états et initiative sont identiques, et « Réinitialiser la carte » remet l'état à zéro. — on `mock/pages/dm-view.html`, `Battlemap`
- **R24 Diffusion vers la player view** (journey) — La player view affiche la couverture de la campagne quand rien n'est envoyé. Le DM y envoie une battlemap, ou une illustration en mode scène (plein écran) ou en mode fenêtre flottante. Dans un onglet player view synchronisé, il déplace les tokens et la fenêtre, la redimensionne en gardant ses proportions, zoome et se déplace. — criterion: given rien d'envoyé; when le DM envoie une illustration en fenêtre flottante puis la redimensionne; then la player view passe de la couverture à l'illustration et garde ses proportions. — on `mock/pages/dm-view.html`, `mock/pages/player-view.html`, `LiveSessionBar`, `ElementsPanel`, `TableSheet`, `Battlemap`, `PlayerStage`
- **R25 Séparation des vues DM et joueurs** (business) — La player view ne montre jamais les notes, les PV des PNJ ni les tokens cachés, absents aussi de son carrousel. Elle n'affiche qu'un état approximatif des PNJ (indemne, blessé, en sang). Un token révélé reprend sa place dans l'ordre d'initiative. — criterion: given un token caché et un PNJ blessé; when la player view s'affiche; then ni notes, ni PV de PNJ, ni token caché n'apparaissent, l'état du PNJ est seulement « blessé », et un token révélé reprend sa place dans l'initiative. — on `mock/pages/player-view.html`, `mock/pages/dm-view.html`, `PlayerStage`, `LiveSoundbox`
- **R26 Clôture de séance avec journal et résumé** (business) — Clore la séance met le micro en pause et conserve localement le journal transcrit horodaté. Un résumé est généré avec le modèle configuré, pour nourrir la préparation suivante. — criterion: given une séance en cours; when le DM clôt la séance; then le micro passe en pause, le journal horodaté est conservé localement, et un résumé est généré avec le modèle configuré. — on `mock/pages/dm-view.html`, `mock/pages/preparation-seance.html`, `LiveSessionBar`, `PreviousSummary`
- **R77 Seul du texte part vers le fournisseur** (business) — L'audio du micro est transcrit et analysé sur la machine du DM. Seul du texte est transmis au fournisseur de génération, qu'il soit joint par une API (cloud ou serveur maison) ou par un binaire (opencode ou Claude Code). L'écran des paramètres le rappelle. — criterion: given le micro actif pendant une séance ou un brief oral, et un fournisseur de génération configuré, d'abord en mode « API », puis en mode « Binaire »; when le DM parle, puis demande une génération (statblock, background, analyse du brief ou résumé) dans chacun des deux modes; then aucune donnée audio ne sort de la machine, et ce qui est transmis à l'API ou au binaire ne contient que du texte — on `mock/pages/parametres-ia.html`, `AiSettings`
- **R92 Détection automatique du type d'API** (business) — En mode « API », le DM ne renseigne que l'URL. Le système interroge cette URL, reconnaît le type d'API (par exemple une API cloud ou un endpoint opencode) et choisit lui-même le protocole de connexion, sans que le DM ait à préciser le type. Si le type n'est pas reconnu, la connexion est refusée et un message l'explique. — criterion: given deux URL valides, l'une d'une API cloud et l'autre de l'endpoint opencode du serveur maison, ainsi qu'une URL qui ne répond pas à un type d'API connu; when le DM saisit tour à tour chacune de ces URL en mode « API » et teste la connexion, sans préciser le type d'API; then le type correct est détecté et affiché pour chacune des deux premières URL et la connexion réussit, tandis que la troisième URL est refusée avec un message indiquant que le type d'API n'est pas reconnu — on `mock/pages/parametres-ia.html`, `AiSettings`
- **R93 Rail de navigation entre les fiches d'entité** (journey) — À gauche de la fiche d'entité, un rail collant liste les éléments de la campagne par catégorie : PJ, PNJ, lieux et événements. Les médias et les sons restent sur la page de la campagne. Une recherche filtre le rail à chaque frappe, avec la même logique approximative que le panneau gauche de la DM view, sur les noms et les alias. Un clic sur un élément charge sa fiche sur place et met à jour l'identifiant dans l'URL, sans rechargement de page. Des boutons « Nouveau… » en bas du rail créent un personnage, un lieu ou un événement. — criterion: given la fiche d'un lieu ouverte, et un PJ « Elowen » portant l'alias « la rôdeuse » dans la campagne; when le DM tape « elowen », puis efface et tape « rodeuse » dans la recherche du rail, et clique sur Elowen; then Elowen figure dans les résultats pour chacune des deux saisies, sa fiche remplace celle du lieu sans rechargement de page, et l'URL porte son identifiant — on `mock/pages/fiche-entite.html`, `CampaignElements`, `EntityEditor`
- **R94 Rail repliable et tiroir sur petit écran** (business) — Un bouton en haut à gauche du rail de la fiche d'entité le replie en une bande fine, et cet état est mémorisé d'une visite à l'autre. Sous 960 px de large, le rail devient un tiroir que le DM ouvre et ferme à la demande. — criterion: given la fiche d'entité ouverte avec son rail déplié; when le DM replie le rail et recharge la page, puis réduit la fenêtre sous 960 px, ouvre le tiroir et le referme; then le rail est toujours replié après le rechargement, et sous 960 px le tiroir s'ouvre au-dessus de la fiche puis se referme sans la masquer — on `mock/pages/fiche-entite.html`, `CampaignElements`
- **R95 Confirmation avant de quitter une fiche modifiée** (business) — Si la fiche ouverte a des modifications non enregistrées et que le DM choisit un autre élément dans le rail, une barre intégrée à la fiche propose « Enregistrer et continuer », « Abandonner » ou « Rester ». « Enregistrer et continuer » enregistre la fiche puis ouvre l'élément choisi ; « Abandonner » ouvre l'élément choisi sans enregistrer ; « Rester » laisse la fiche et ses modifications en l'état. — criterion: given la fiche d'un PNJ dont la description a été modifiée sans être enregistrée; when le DM clique sur un autre élément du rail et choisit tour à tour « Rester », « Abandonner » puis, après une nouvelle modification, « Enregistrer et continuer »; then « Rester » garde la fiche et la modification affichées, « Abandonner » ouvre l'élément choisi et la modification est perdue, et « Enregistrer et continuer » enregistre la modification avant d'ouvrir l'élément choisi — on `mock/pages/fiche-entite.html`, `EntityEditor`
- **R96 Sections du rail repliables par catégorie** (business) — Dans le rail de la fiche d'entité, chaque catégorie (PJ, PNJ, lieux, événements) est une section que le DM ferme ou ouvre en cliquant sur son titre. Un petit triangle à gauche du titre indique si la section est ouverte ou fermée. Fermer une section masque ses éléments sans toucher aux autres sections. — criterion: given la fiche d'entité ouverte avec son rail déplié et les sections PJ, PNJ, lieux et événements toutes ouvertes; when le DM clique sur le titre « PJ », puis clique de nouveau sur ce même titre; then au premier clic, les éléments PJ sont masqués, le triangle à gauche du titre indique que la section est fermée, et les sections PNJ, lieux et événements restent ouvertes ; au second clic, les éléments PJ réapparaissent et le triangle indique que la section est ouverte — on `mock/pages/fiche-entite.html`, `CampaignElements`
- **R97 Médias rattachés à la séance par leur lieu** (business) — Chaque média, battlemaps comprises, peut être associé à un lieu de la campagne, depuis sa carte dans la bibliothèque ou depuis le formulaire de téléversement. Un média sans lieu reste valide. Quand un lieu fait partie d'une séance, tous les médias de ce lieu sont associés à cette séance, sans geste supplémentaire. L'association entre lieu et séance est partagée entre la page Médias et la préparation de séance : ajouter un lieu à une séance depuis l'une ou l'autre page associe ses médias à la séance sur les deux pages. La page Médias indique l'association, par exemple « Associé à la séance 4 ». Si le lieu n'est pas encore dans la séance, elle permet de l'y ajouter. Changer le lieu d'un média met à jour son association à la séance. — criterion: given une battlemap et une illustration associées à « La Taverne du Sanglier », absente de la séance 4, une seconde battlemap associée à un autre lieu, et un média sans lieu; when le DM ajoute la taverne à une scène de la séance 4 depuis la préparation de séance, puis ouvre la page Médias et y change le lieu de la seconde battlemap pour « La Taverne du Sanglier »; then la battlemap et l'illustration de la taverne sont associées à la séance 4 sur les deux pages, la page Médias affiche « Associé à la séance 4 » sur leurs cartes, la seconde battlemap le devient aussi après son changement de lieu, et le média sans lieu reste valide sans association à la séance — on `mock/pages/medias.html`, `mock/pages/preparation-seance.html`, `MediaLibrary`, `SceneBoard`
- **R98 Pastille des suggestions en surcouche fixe** (journey) — Dans la DM view, la pastille des suggestions et le bouton marche/pause du micro forment une surcouche posée au-dessus de la page, ancrée en bas à droite de la fenêtre. Elle garde cette position quel que soit le contenu ouvert au centre (fiche en lecture, battlemap ou onglet Soundbox) et quand le DM fait défiler la page ou le panneau gauche. Elle passe au-dessus de ce contenu sans le décaler, et reste cliquable. — criterion: given une séance en direct dans la DM view, avec le micro en marche et une entité déjà suggérée dans la pastille; when le DM ouvre au centre une longue fiche en lecture et la fait défiler jusqu'en bas, puis ouvre la battlemap de la scène et enfin l'onglet Soundbox; then la pastille et le bouton marche/pause du micro restent affichés au même endroit en bas à droite de la fenêtre, par-dessus le contenu, sans le décaler, et un clic sur l'entité suggérée l'ouvre toujours au centre — on `mock/pages/dm-view.html`, `SuggestionPill`
- **R99 Tester la connexion au fournisseur** (journey) — Depuis les Paramètres IA, le DM vérifie d'un clic que le fournisseur choisi (API cloud ou endpoint opencode maison) répond avec l'URL, les modèles et la clé renseignés. — criterion: given le DM a renseigné l'URL de son endpoint opencode maison, un modèle de génération et un modèle de résumé; when il clique sur « Tester la connexion »; then l'écran indique si la connexion réussit ou, sinon, la raison de l'échec, sans enregistrer de réglage invalide à son insu — on `mock/pages/parametres-ia.html`, `AiSettings`
- **R100 Création assistée avec les deux fournisseurs** (business) — La proposition de background, de comportement et de statblock d'un PNJ fonctionne de la même façon avec l'API cloud et avec l'endpoint opencode du serveur maison. — criterion: given un PNJ avec un prompt initial dicté, et deux réglages valides : l'API cloud puis l'endpoint opencode; when le DM demande la proposition IA avec chacun des deux fournisseurs; then chaque fournisseur remplit le background, le comportement et un statblock complet (caractéristiques, CA, PV, capacités) dans les mêmes champs de la fiche — on `mock/pages/fiche-entite.html`, `mock/pages/parametres-ia.html`, `EntityEditor`, `AiSettings`
- **R101 Image de token par défaut** (business) — Le token d'un personnage reprend par défaut le recadrage de son portrait ; une image dédiée peut le remplacer ; sans portrait, il devient une pastille de couleur avec les initiales et le numéro d'instance. — criterion: given un PNJ « Bandit » sans portrait ni image de token, placé en trois instances; when les tokens s'affichent sur la battlemap; then chacun est une pastille de couleur portant « B » et son numéro (1, 2, 3), et dès qu'un portrait est ajouté et recadré, les tokens montrent ce recadrage — on `mock/pages/fiche-entite.html`, `mock/pages/dm-view.html`, `mock/pages/player-view.html`, `EntityEditor`, `Battlemap`, `PlayerStage`
- **R102 Lieux et événements reconnus dans le brief** (business) — Le brief oral reconnaît non seulement les PNJ, mais aussi les lieux et les événements existants, et place les lieux et événements manquants dans la liste « à créer ». — criterion: given une campagne qui contient le lieu « La Taverne du Sanglier » mais pas « Le Pont des Pendus »; when le DM raconte un brief qui cite la taverne, le pont et l'événement « l'embuscade »; then la taverne est liée à la scène correspondante comme lieu existant, et le pont et l'embuscade apparaissent dans la liste « à créer » avec leur type et l'extrait du brief — on `mock/pages/preparation-seance.html`, `mock/pages/campagne.html`, `OralBrief`, `ToCreateList`, `SceneBoard`
- **R103 Affiner une entité manquante** (journey) — Pour une entité de la liste « à créer », le DM peut compléter sa description à l'oral ou à l'écrit avant que l'IA en fasse la proposition. — criterion: given un PNJ manquant dans la liste « à créer », issu du brief; when le DM choisit « Affiner », dicte trois phrases de plus et valide; then la proposition de l'IA (background et statblock) tient compte à la fois de l'extrait du brief et des phrases ajoutées, et l'entité quitte la liste « à créer » une fois créée — on `mock/pages/preparation-seance.html`, `mock/pages/campagne.html`, `ToCreateList`
- **R105 Initiative des PNJ et départage** (business) — Au lancement du combat, chaque PNJ tire d20 + modificateur de DEX ; les égalités se départagent par la DEX, puis au hasard. — criterion: given deux PNJ qui obtiennent tous deux 14 à l'initiative, l'un avec DEX 16 et l'autre avec DEX 12, et un PJ dont le DM saisit 14; when le DM lance le combat; then chaque PNJ a une initiative comprise entre 1 + son modificateur et 20 + son modificateur, et le PNJ à DEX 16 passe avant celui à DEX 12 ; une égalité qui subsiste à DEX égale est tranchée au hasard — on `mock/pages/dm-view.html`, `Battlemap`
- **R106 Fiche d'une instance précise en combat** (journey) — Quand plusieurs instances d'un même PNJ sont sur la carte, un clic sur l'une d'elles dans le carrousel ouvre la fiche de cette instance précise, avec ses PV courants modifiables. — criterion: given un combat en cours avec Bandit 1, Bandit 2 et Bandit 3; when le DM clique sur Bandit 2 dans le carrousel et retire 7 PV; then la fiche ouverte est celle de Bandit 2, seuls les PV de Bandit 2 baissent de 7, et ceux de Bandit 1 et Bandit 3 restent inchangés — on `mock/pages/dm-view.html`, `Battlemap`, `TableSheet`
- **R107 Révélation à sa place dans l'initiative** (business) — Un token caché garde sa place dans l'ordre d'initiative ; le révéler le fait apparaître à cette place dans le carrousel joueur. — criterion: given un combat où Chef bandit 1, caché, a la deuxième initiative et figure avec une icône dans le carrousel DM seulement; when le DM le rend visible; then il apparaît en deuxième position dans le carrousel de la player view, et l'ordre des autres participants n'a pas changé — on `mock/pages/dm-view.html`, `mock/pages/player-view.html`, `Battlemap`, `PlayerStage`
- **R108 État approximatif des PNJ** (business) — La player view déduit des PV courants un état approximatif pour chaque PNJ visible : indemne, blessé ou en sang, sans jamais montrer les PV. — criterion: given un PNJ visible de 20 PV max; when ses PV courants passent à 20, puis à 13, puis à 10, puis à 0; then la player view affiche successivement « indemne », « blessé » (au-dessus de la moitié), « en sang » (à la moitié ou moins), puis le token grisé et marqué mort, sans aucun chiffre de PV — on `mock/pages/player-view.html`, `mock/pages/dm-view.html`, `PlayerStage`, `Battlemap`
- **R109 Historique des suggestions** (journey) — Une icône en haut à droite de la pastille ouvre l'historique des 5 dernières entités détectées, que le DM peut rouvrir d'un clic. — criterion: given le micro a détecté successivement la taverne, Morlox et le tavernier; when le DM clique sur l'icône d'historique de la pastille puis sur la taverne; then l'historique liste ces entités de la plus récente à la plus ancienne, et la fiche de la taverne s'ouvre au centre — on `mock/pages/dm-view.html`, `SuggestionPill`
- **R110 Recherche et tags dans la soundbox** (journey) — L'onglet Soundbox présente une case par son, filtrée dynamiquement par la recherche et par les tags. — criterion: given une soundbox qui contient « porte qui grince » (tag porte) et « bruit de foule » (tag taverne); when le DM tape « gri » dans la recherche, puis l'efface et filtre sur le tag taverne; then seule la case « porte qui grince » reste pendant la saisie, puis seule la case « bruit de foule » reste avec le filtre — on `mock/pages/dm-view.html`, `LiveSoundbox`
- **R111 Sons simultanés et volumes** (business) — Plusieurs effets jouent en même temps depuis la DM view, chacun à son volume propre multiplié par le volume global, et « Tout arrêter » les coupe tous. — criterion: given « bruit de foule » joue depuis la soundbox; when le DM déclenche « porte qui grince » depuis la pastille, baisse le volume global, puis clique sur « Tout arrêter »; then les deux sons se superposent, tous deux baissent avec le volume global en gardant leur rapport, puis plus aucun son ne joue — on `mock/pages/dm-view.html`, `mock/pages/soundbox.html`, `LiveSoundbox`, `SuggestionPill`, `SoundLibrary`
- **R112 Fenêtre d'illustration proportionnelle** (business) — La fenêtre flottante d'une illustration envoyée en mode fenêtre se déplace et se redimensionne en gardant les proportions de l'image. — criterion: given une illustration de 800 × 1200 envoyée en mode fenêtre; when le DM l'agrandit par glisser-déposer dans son onglet player view; then la fenêtre garde un rapport largeur sur hauteur de 2/3, et sa position et sa taille sont les mêmes sur la player view projetée — on `mock/pages/player-view.html`, `PlayerStage`
- **R113 Onglet player view piloté par le DM** (journey) — Dans un onglet player view synchronisé, le DM déplace les tokens, zoome, dézoome et se déplace dans la scène comme sur un canevas. — criterion: given une battlemap envoyée aux joueurs et un onglet player view ouvert par le DM; when il y déplace un token, zoome et décale le cadrage; then le token est à la nouvelle position dans la DM view, et le zoome et le cadrage sont les mêmes sur la player view projetée — on `mock/pages/player-view.html`, `mock/pages/dm-view.html`, `PlayerStage`, `Battlemap`
- **R114 Synchronisation des onglets sous une seconde** (business) — Tout changement fait dans un onglet (token, carrousel, diffusion, fenêtre) apparaît dans les autres onglets ouverts en moins d'une seconde. — criterion: given la DM view et la player view ouvertes dans deux onglets de la même machine; when le DM dépose un token sur un hex de la DM view; then le token apparaît sur le même hex de la player view en moins d'une seconde — on `mock/pages/dm-view.html`, `mock/pages/player-view.html`, `Battlemap`, `PlayerStage`, `LiveSessionBar`

## Data

Drafted and validated at creation, beside the mock-up: `mock/data/model.json` (contract M, 33 tables) and one `mock/data/aggregates/<Aggregate>.aggregate.json` per aggregate (contract I, 12 aggregates, 61 invariants). The task that builds an aggregate installs its draft at `crates/dataguard/aggregates/<Aggregate>/aggregate.json` and moves its tables into `data/model.json` with its migration.

- **Campaign** — campaign, campaign_overview_view
  - oneshot-single-session — Un oneshot a exactement une séance : sa création crée cette séance, et on ne peut pas en ajouter une autre.
  - import-creates-copy — Un import crée toujours une nouvelle campagne avec de nouveaux identifiants, même si origin_campaign_id correspond à une campagne existante. L'originale n'est jamais modifiée.
  - export-stamps-date — Un export réussi met à jour last_exported_at et ne modifie rien d'autre.
  - master-volume-range — Le volume global reste entre 0 et 1, et il multiplie le volume propre de chaque son.
- **Player** — player
  - player-not-character — Un joueur n'est pas un élément de campagne : il n'a ni fiche ni token. Il est relié à ses PJ par character.player_id.
- **Entity** — entity, character, entity_alias, entity_version, detection_vocabulary_view
  - character-is-pj-or-pnj — Une fiche de personnage n'existe que pour un élément de type « pj » ou « pnj ». player_id n'est renseigné que pour un PJ, et le joueur appartient à la même campagne.
  - alias-unique-in-campaign — Un alias, une fois normalisé, ne désigne qu'un seul élément d'une campagne et ne double pas le nom d'un autre élément.
  - every-change-versioned — Tout enregistrement de l'élément ou de sa fiche, quelle que soit sa source (DM, IA, D&D Beyond, brief, restauration), ajoute une version avec son instantané complet et son diff. Les versions ne sont jamais modifiées.
  - restore-exact — Revenir à une version, ou annuler un changement, réécrit l'élément, sa fiche et ses alias exactement comme dans l'instantané visé, et ajoute une version de source « restauration ».
  - dndbeyond-resync-scope — Une resynchronisation D&D Beyond ne change que les caractéristiques, la CA, les PV max, les sorts et les actions. Les alias, les notes, le token et le portrait restent inchangés.
  - proposal-fills-sheet-fields — Une proposition IA remplit background, comportement, caractéristiques, CA, PV max et actions dans les mêmes champs, quel que soit le fournisseur. Elle n'est construite qu'à partir de texte.
  - explicit-save-only — Seul un enregistrement explicite du DM écrit la fiche. Une modification non enregistrée n'est jamais écrite, et « Enregistrer et continuer » écrit avant la navigation.
  - default-token — Sans image de token ni portrait, le token s'affiche comme une pastille de token_color portant l'initiale du nom. Avec un portrait recadré, il affiche ce recadrage.
  - dex-modifier-derived — Le modificateur de DEX vaut toujours floor((DEX - 10) / 2) et n'est jamais saisi.
- **NoteImport** — imported_note, note_proposal
  - no-creation-before-validation — L'analyse des notes ne crée aucun élément. Seule la validation crée des éléments, et seulement pour les propositions cochées qui n'ont pas encore de created_entity_id.
- **Session** — session, session_present_character, session_roster_view, scene, scene_entity, scene_media, session_media_view, oral_brief, entity_to_create, session_journal_line
  - present-only-pj — Seuls des PJ de la même campagne peuvent être cochés présents. Les PJ proposés en DM view et sur les battlemaps sont exactement les présents, y compris ceux ajoutés plus tard.
  - scene-order-contiguous — Les rangs des scènes d'une séance vont de 1 à n, sans trou ni doublon. Un réordonnancement réécrit tous les rangs d'un coup.
  - scene-links-same-campaign — Les éléments et médias liés à une scène appartiennent à la campagne de la séance.
  - session-media-by-place — Un média est associé à une séance s'il est lié à une de ses scènes ou si son lieu figure dans une de ses scènes. Cette association n'est jamais stockée, elle suit donc tout changement de lieu. Un média sans lieu reste valide.
  - brief-no-auto-creation — L'analyse d'un brief lie les éléments existants à leurs scènes et met à jour les PNJ existants avec une version et son diff. Tout élément inconnu va dans la liste « à créer » avec son type et son extrait. Aucun élément n'est créé automatiquement.
  - to-create-leaves-once-created — Une entrée « à créer » passe à « creee » si et seulement si created_entity_id est renseigné. La proposition affinée tient compte de l'extrait et des phrases ajoutées.
  - status-lifecycle — Une séance passe de « prep » à « live » puis à « closed », sans retour en arrière. Une séance close n'accepte plus de lignes de journal.
  - close-session — Clore une séance éteint le micro, arrête les sons, conserve le journal horodaté en local et enregistre le résumé avec le modèle de résumé configuré.
  - text-only-no-audio — Aucune colonne ne contient d'audio. Les briefs et le journal ne sont conservés qu'en texte transcrit en local, et seul ce texte part vers le fournisseur.
  - previous-summary-visible — Le résumé et le journal de la séance close qui précède sont lisibles depuis la séance suivante, et le résumé figure dans la liste des séances.
- **Media** — media, battlemap_grid
  - grid-only-for-battlemap — Seule une battlemap a un réglage de grille, et elle en a toujours un dès son téléversement.
  - hex-is-five-feet — Un hex vaut toujours 5 pieds, quels que soient sa taille, son décalage ou son orientation.
  - grid-values-range — La taille d'un hex est strictement positive et l'opacité reste entre 0 et 1.
  - media-place-is-lieu — Le lieu d'un média, s'il est renseigné, est un élément de type « lieu » de la même campagne.
- **BattlemapInstance** — battlemap_instance, token, combat, initiative_entry, initiative_order_view
  - instance-isolated — Les tokens, PV, états et le combat appartiennent à l'instance. La battlemap d'origine et sa grille ne sont jamais modifiées par une instance.
  - one-instance-per-scene — Une scène a au plus une instance de battlemap. La battlemap d'origine est liée à la scène et appartient à la campagne de la séance.
  - token-roster — Un PJ ne peut être posé que s'il est présent à la séance, et un PJ n'a qu'un token par instance.
  - multi-drop-numbering — Déposer un personnage « × n » crée n tokens numérotés à la suite des numéros déjà pris pour ce personnage (1 à n la première fois). Les numéros sont uniques par instance et par personnage. Renommer un token ne touche que lui.
  - pj-hp-not-tracked — hp_current et hp_max restent vides pour un token de PJ. Pour un PNJ, hp_max est copié de la fiche à la pose et 0 ≤ hp_current ≤ hp_max.
  - hp-change-one-token — Un changement de PV ne touche que le token visé, jamais les autres instances du même personnage.
  - dead-derived — Un PNJ est mort si et seulement si ses PV courants valent 0. Il est alors grisé et sauté par « Suivant ».
  - players-see-band-only — La player view ne reçoit que health_band : jamais de chiffre de PV, jamais de notes, jamais de token caché.
  - npc-initiative-bounds — Pour un PNJ, roll est entre 1 et 20 et initiative = roll + modificateur de DEX. Pour un PJ, roll est vide et initiative est saisie par le DM.
  - initiative-tiebreak — Au lancement, rank suit l'initiative décroissante, puis la DEX décroissante, puis le tirage aléatoire.
  - hidden-keeps-rank — Un token caché garde son rank. Le révéler ne change ni son rang ni celui des autres participants.
  - next-turn — « Suivant » passe au prochain participant vivant dans l'ordre des rangs. Il incrémente le round quand il revient au premier.
  - one-active-combat — Une instance a au plus un combat sans ended_at.
  - state-persisted-reset — Positions, PV, états et initiative sont persistés à chaque action et identiques après réouverture. « Réinitialiser la carte » supprime tokens, combat et initiative de l'instance.
  - single-position-source — La position (q, r) d'un token est la seule source pour la DM view et la player view. Le dépôt est diffusé aux onglets en moins d'une seconde, et la distance affichée pendant le glisser vaut la distance hex entre l'origine et le curseur, multipliée par 5 pieds.
- **Sound** — sound
  - sound-volume-range — Le volume propre reste entre 0 et 1. Le volume effectif vaut volume × volume global, ce qui garde le rapport entre deux sons.
  - tags-normalized — Les tags sont en minuscules, sans doublon. Le filtre par tag et la recherche sur le nom ne lisent que ces champs.
- **SoundPlayback** — sound_playback
  - simultaneous-playback — Plusieurs lectures sans stopped_at peuvent coexister. Un son déclenché depuis la pastille ajoute une lecture sans arrêter les autres.
  - stop-all — « Tout arrêter » renseigne stopped_at sur toutes les lectures en cours de la séance.
  - dm-side-only — Une lecture n'est jouée que par la DM view. La player view ne reçoit aucune lecture.
- **Suggestion** — live_suggestion
  - suggestion-target — Une suggestion vise exactement un élément ou un son de la campagne de la séance. La détection ne lit que le vocabulaire de la campagne.
  - pile-and-history-derived — La pile de la pastille est formée des 5 suggestions les plus récentes, et l'historique les liste toutes par detected_at décroissant. Rien n'est stocké en plus des suggestions.
  - live-only-local-audio — Une suggestion n'est produite que pendant une séance « live » avec le micro allumé. Elle vient d'une transcription locale, et aucun audio ne quitte la machine.
- **Diffusion** — diffusion
  - base-coherent — « cover » n'a ni média ni instance en fond. « illustration » a une illustration en base_media_id. « battlemap » a une instance de la séance en base_instance_id.
  - window-proportional — La fenêtre flottante montre une illustration. Sa hauteur n'est pas stockée : elle vaut window_width × height_px / width_px, ce qui garde les proportions du média (2/3 pour 800 × 1200).
  - window-fields-together — window_x, window_y et window_width sont tous renseignés si une fenêtre est ouverte, et tous vides sinon.
  - single-diffusion-source — La player view projetée et l'onglet player view du DM lisent la même ligne. Position, taille, zoom et cadrage y sont donc identiques.
- **AiSettings** — ai_provider, ai_settings
  - mode-fields — En mode « api », url et detected_api_type sont renseignés et binary est vide. En mode « binaire », binary vaut exactement « opencode » ou « claude-code » et url est vide.
  - api-type-recognized — Une URL dont le type d'API n'est pas reconnu ne peut pas être enregistrée. Le test la refuse avec le message « type d'API non reconnu ».
  - test-does-not-save — « Tester la connexion » n'écrit que les champs last_test_*. Il n'enregistre jamais URL, modèles ni mode, donc aucun réglage invalide n'est enregistré à l'insu du DM.
  - single-settings-row — Il existe une seule ligne ai_settings (id = 1), et elle pointe vers un fournisseur existant.
  - text-only-payload — Toute requête vers le fournisseur actif, API ou binaire, ne porte que du texte : prompts transcrits, briefs, notes, journal. Jamais d'audio.

## Ports

Everything outside the product, each behind a port (`ports.json`, contract Q): its adapter under `crates/ports/<port>/<provider>/`, its fake and contract tests in `crates/ports/<port>/`, its secrets by name — a human provides them.

- **database** — PostgreSQL 16 (sql postgres), keeps state: The product's store: the Data layer reads it, the DataGuard writes it — listed, not abstracted.
- **llm** — Anthropic Claude API (default; claude-sonnet-5-5 for generation, claude-haiku-4-5-20251001 for summaries), switchable to the DM's home opencode server endpoint or to the opencode / Claude Code binaries (rest Anthropic Messages API + OpenAI-compatible chat completions (opencode server); CLI adapters for opencode and claude): Text generation from text only: extract entities from imported notes, turn an oral brief into scenes and detect existing or missing NPCs, places and events, propose backgrounds, behaviour and credible NPC statblocks, generate the end-of-session summary, and test the connection. It is reached either by an API URL whose type is auto-detected (cloud API or the home opencode server endpoint) or by a local binary (opencode or Claude Code). — used by ai.testConnection, ai.extractFromNotes, ai.analyzeBrief, ai.createFromBrief, ai.proposeCharacter, session.closeSession
- **speech-to-text** — Phonon (local, to be evaluated as the decision requests) (other Local Phonon runtime as a sidecar process, streaming transcription): Local French transcription of the microphone, with vocabulary injected from campaign names, aliases and sound names. It feeds live suggestions, the timestamped session journal, the oral brief, dictated character prompts, refinements and dictated notes. No audio leaves the machine. — used by live.setMicrophone, ai.analyzeBrief, ai.proposeCharacter, ai.createFromBrief, ai.extractFromNotes
- **character-sheets** — D&D Beyond (public character JSON service) (rest character-service.dndbeyond.com/character/v5/character/{id} (unofficial, public sharing required)), keeps state: Import a player character by its D&D Beyond URL, and resynchronise stats, HP and spells after a level-up. Local aliases, notes and the token are kept, and every change is versioned locally. — used by dndbeyond.importCharacter, dndbeyond.resyncCharacter

## Mock-ups

The screens as the human accepted them, under `mock/` on the integration branch (contract N).

- Accueil des campagnes — `mock/pages/campagnes.html` — Le DM retrouve ses campagnes et oneshots avec leur couverture, en crée une, exporte une campagne en archive ou en importe une
- Paramètres IA — `mock/pages/parametres-ia.html` — Le DM choisit l’API cloud ou l’endpoint opencode de son serveur maison, renseigne URL, modèles et clé, et teste la connexion
- Campagne — `mock/pages/campagne.html` — Le DM parcourt les séances et les éléments de sa campagne, crée ou lance une séance, importe ses notes et traite les entités à créer
- Fiche d’entité — `mock/pages/fiche-entite.html` — Le DM renseigne un personnage, un lieu ou un événement, dicte le prompt initial pour obtenir background et statblock, importe un PJ de D&D Beyond et annule un changement de l’IA
- Préparation de séance — `mock/pages/preparation-seance.html` — Le DM relit la séance précédente, raconte son brief au micro, vérifie les scènes et les entités détectées, crée les manquantes et place les tokens sur la battlemap d’une scène
- Médias et réglage de battlemap — `mock/pages/medias.html` — Le DM téléverse ses battlemaps et illustrations et règle la grille hexagonale de chaque carte avec un aperçu en direct
- Soundbox — `mock/pages/soundbox.html` — Le DM téléverse ses effets sonores, les nomme et leur donne des tags
- DM view — `mock/pages/dm-view.html` — Le DM fait rouler la séance : il ouvre les éléments suggérés par le micro, déplace les tokens, mène le combat, joue les sons et envoie cartes et illustrations aux joueurs
- Player view — `mock/pages/player-view.html` — Les joueurs voient la couverture, la battlemap ou l’illustration envoyée par le DM ; dans l’onglet piloté, le DM y déplace tokens et fenêtre et cadre la carte

## Micro-frontends

As `mock/pages.json` lists them, with the Capabilities they read (`needs`, each answered by `mock/fixtures/<Capability>.json`) and the DataCapabilities they trigger (`actions`). Every write's effect and the fields it touches are in `mock/proof-of-concept.json` (`writes`).

- **CampaignList** (campaign) — Les campagnes et oneshots avec leur couverture ; création, export et import d’archive — needs campaign.listCampaigns — actions campaign.createCampaign, campaign.exportCampaign, campaign.importCampaign
- **AiSettings** (ai) — Le fournisseur de génération (API cloud ou opencode maison), ses modèles et sa clé, le test de connexion et ce qui sort de la machine — needs ai.getSettings — actions ai.saveSettings, ai.testConnection
- **CampaignOverview** (campaign) — La couverture, le résumé chiffré de la campagne, l’accès à la prochaine séance et l’export en archive — needs campaign.getCampaign — actions campaign.exportCampaign
- **SessionList** (session) — Les séances dans l’ordre avec leur résumé et leurs PJ présents ; création d’une séance avec choix des PJ — needs session.listSessions, campaign.listCharacters — actions session.createSession
- **CampaignElements** (campaign) — Les éléments de la campagne : en grille à onglets (joueurs, personnages, lieux, événements, médias, sons) ou en rail collant à gauche d’une fiche (PJ, PNJ, lieux, événements ; recherche floue, repliable, tiroir sous 960 px) — needs campaign.listPlayers, campaign.listCharacters, campaign.listPlaces, campaign.listEvents, media.listMedia, soundbox.listSounds
- **NoteImport** (ai) — L’import de notes en texte libre, tapées, collées ou dictées ; extraction par l’IA puis validation en lot, la note brute conservée — needs campaign.listImportedNotes — actions ai.extractFromNotes, campaign.createEntitiesFromNote
- **ToCreateList** (ai) — Les PNJ, lieux et événements issus des briefs, à créer depuis le brief ou après une description affinée à l’oral ou à l’écrit — needs campaign.listEntitiesToCreate — actions ai.createFromBrief
- **EntityEditor** (campaign) — La fiche d’un personnage, d’un lieu ou d’un événement : identité, alias, prompt initial dicté, fiche de stats, token, notes et D&D Beyond ; s’ouvre sur place depuis le rail, avec une barre de confirmation si la fiche est modifiée et non enregistrée — needs campaign.getEntity, campaign.listPlayers, media.listMedia — actions campaign.saveEntity, ai.proposeCharacter, dndbeyond.importCharacter, dndbeyond.resyncCharacter
- **VersionHistory** (campaign) — Le bouton « Mis à jour » d’une fiche modifiée par l’IA, son diff champ par champ et l’historique des versions avec retour en arrière — needs campaign.listEntityVersions — actions campaign.restoreEntityVersion
- **PreviousSummary** (session) — Le résumé de la séance précédente, ses entités mentionnées, ses fils ouverts et son journal transcrit en panneau latéral — needs session.getPreviousSessionSummary, session.getSessionJournal
- **OralBrief** (ai) — Le brief raconté au micro, transcrit en local, puis découpé en scènes avec les entités reconnues, mises à jour ou manquantes — needs session.getLastBrief — actions ai.analyzeBrief
- **SceneBoard** (session) — Les PJ présents et les scènes ordonnées de la séance, avec leurs lieux, PNJ, événements, médias, notes et instance de battlemap — needs session.getSession, session.listScenes, campaign.listCharacters, media.listMedia, table.getBattlemapInstance — actions session.addPresentCharacter, session.removePresentCharacter, session.addScene, session.reorderScenes, table.createBattlemapInstance
- **Battlemap** (table) — Une instance de battlemap : grille hexagonale, tokens glissés avec le compteur « hex · pi », instances × N, fiche d’instance, combat et carrousel d’initiative — needs table.getBattlemapInstance, media.getBattlemap, campaign.listCharacters — actions table.moveToken, table.placeTokens, table.removeToken, table.renameToken, table.setTokenVisibility, table.setTokenCondition, table.changeHitPoints, table.startCombat, table.nextTurn, table.reorderInitiative, table.addParticipant, table.removeParticipant, table.endCombat, table.resetBattlemap, live.showOnPlayerView
- **MediaLibrary** (media) — Le téléversement et la liste des battlemaps et illustrations de la campagne, chacune rattachable à un lieu ; si le lieu est dans la séance, le média l’est aussi — needs media.listMedia, session.listScenes, campaign.listPlaces — actions media.uploadMedia, media.setMediaPlace, session.addPlaceToSession
- **GridSettings** (media) — Le réglage de la grille hexagonale d’une battlemap (taille, décalage, orientation, couleur, opacité) avec un aperçu en direct — needs media.getBattlemap — actions media.saveGridSettings
- **SoundLibrary** (soundbox) — La bibliothèque des effets sonores : téléversement, nom, tags et volume propre — needs soundbox.listSounds — actions soundbox.addSound, soundbox.updateSound, soundbox.removeSound
- **LiveSessionBar** (session) — La séance en direct : ses scènes, ce que voient les joueurs, l’ouverture de la player view et la clôture avec le résumé à retoucher — needs session.getLiveSession, live.getDiffusion — actions live.showCover, live.closeWindow, session.closeSession
- **ElementsPanel** (live) — Le panneau gauche de la DM view : la séance scène par scène, puis toute la campagne par type, avec une recherche approximative sur les noms et alias — needs session.listScenes, campaign.listCharacters, campaign.listPlaces, campaign.listEvents, media.listMedia — actions live.showOnPlayerView, live.showWindowOnPlayerView
- **TableSheet** (live) — La fiche d’un élément en lecture, optimisée pour la table, avec édition et envoi de l’illustration aux joueurs — needs campaign.getEntity, media.listMedia — actions live.showOnPlayerView, live.showWindowOnPlayerView
- **LiveSoundbox** (soundbox) — L’onglet Soundbox de la DM view : cases de sons, recherche, filtre par tags, volume global et par son, « Tout arrêter » — needs soundbox.listSounds — actions soundbox.playSound, soundbox.stopAll, soundbox.setSoundVolume, soundbox.setMasterVolume
- **SuggestionPill** (live) — La pastille en bas à droite : les 5 dernières entités ou sons entendus au micro, leur historique, et le bouton marche/pause du micro — needs live.listSuggestions — actions live.setMicrophone, soundbox.playSound
- **PlayerStage** (live) — L’écran des joueurs : couverture, battlemap ou illustration en scène, et fenêtre d’illustration flottante déplaçable et redimensionnable — needs live.getDiffusion, campaign.getCampaign, media.listMedia — actions live.moveWindow, live.resizeWindow

## Design system

On the `design-system` branch, carried onto the integration branch. Validated 2026-10-13T17:38:34Z. 30 token(s); the kit: Button, Input, Select, Checkbox, Radio, Toggle, Menu, Tabs, Card, Table, Badge, Breadcrumb, HexMap, Token, MicButton, DiffView, Slider, Dialog, TextArea, FileDrop, StatBlock.

## Requests left on the mock-up

Pins the human left on the pages, that no backlog issue carries: intentions for the development to take up.

- *request* on `mockup--main-agent--dm-view`: Ca doit etre en overlay au dessus de la page et rester en position fixe en bas a droite.
- *request* on `mockup--main-agent--dm-view` at `id:dm-elements`: Les composants a l'interieur de cette div semble depasser ne permettant pas d'etre visible pour l'utilisateur. Peux tu aggrandir cette barre
- *request* on `mockup--main-agent--fiche-entite` at `id:page-main`: Rail de navigation des éléments sur la fiche d'entité  **Changement** : ajouter à `CampaignElements` un mode « rail », injecté à gauche de `fiche-entite.html` (collant, en tiroir sous 960 px). La grille à onglets reste sur `campagne.html`. Cliquer un élément charge la fiche sur place et met à jour `?id=`, sans rechargement de page.  **Pourquoi** : passer d'une fiche à l'autre sans retourner à la campagne.  **Contenu du rail** - Catégories : PJ, PNJ, Lieux, Événements (Médias et Sons restent sur `campagne`). - Recherche filtrant à chaque frappe, avec la logique floue d'`ElementsPanel`. - Boutons « Nouveau… » en bas. - Bouton en haut à gauche pour replier en bande fine ; état mémorisé via `Mock.store`.  **Contrainte ouverte** : si la fiche a des modifications non enregistrées, une barre intégrée propose « Enregistrer et continuer », « Abandonner » ou « Rester ». Ce comportement reste à valider.  **Critères de fin** - Le repli persiste après rechargement. - « elowen » et « rodeuse » trouvent Elowen. - Un clic change la fiche et l'URL sans rechargement. - Le tiroir mobile s'ouvre et se ferme correctement sous 960 px. - La barre de confirmation fonctionne pour ses trois choix, « Enregistrer et continuer » compris, non testé jusqu'ici.
- *request* on `mockup--main-agent--fiche-entite` at `id:rail-body`: Il faut en cliquant sur le titre, comme PJ fermer et ouvrir la section dans ce format du composant. Un petit triangle indique qu'il est fermé ou ouvert à gauche du titre
- *request* on `mockup--main-agent--medias`: Associer un lieu aux médias et rattacher automatiquement leurs battlemaps à la séance  **Ce qui doit changer** - Chaque média (battlemaps comprises) peut être associé à un lieu, depuis sa carte dans la bibliothèque et depuis le formulaire de téléversement. - Quand un lieu fait partie d'une séance, tous les médias de ce lieu, battlemaps incluses, sont associés directement à cette séance, sans geste supplémentaire. - La page Médias indique clairement l'association (ex. « Associé à la séance 4 ») et permet d'ajouter le lieu à la séance si ce n'est pas fait.  **Pourquoi** Le MJ prépare une séance autour de ses lieux. Les battlemaps doivent suivre le lieu pour ne pas être rattachées une à une.  **Contraintes** - L'association lieu ↔ séance doit être partagée entre la page Médias et la préparation de séance (`SceneBoard`). Dans la maquette actuelle, elle n'existe que dans le composant Médias : ajouter un lieu à une scène ne se répercute pas sur la page Médias. - Un média sans lieu reste valide. - Le comportement n'a été vérifié que par la syntaxe et la validité des JSON, pas dans un navigateur.  **Critères de fin** - Ajouter un lieu à une séance, depuis l'une ou l'autre page, associe ses médias à la séance sur les deux pages. - Changer le lieu d'un média met à jour son association à la séance. - Vérifié en navigateur.
- *request* on `mockup--main-agent--parametres-ia`: Il faut revoir ce micro-frontend. Serveur maison ou api cloud, on doit y mettre une URL d'api. Le systeme doit detecter automatiquement quel type d'api c'est est le connecter. Cependant il doit aussi fonctionner avec un binaire donc on a deux options, api et binaire. dans le cas binaire une droplist avec deux choix pour le moment, opencode et claude code.
