# UI audit — the nine screens at 360 px, 768 px and 1280 px

A written, factual picture of the interface as it stands. It describes and measures; it gives no direction. The
type scale, the accent rule, the treatment of states and the DM-view brief are the next slices' to argue
(`DESIGN-DIRECTION.md`), from this file.

- Audited tree: the tip of `milestone/739-ui-audit-and-design-direction-written-in-the-repository`, `36766f7` (the parent of the
  commit that adds this file), 2026-10-29.
- Nothing was changed to take these measures: no token, kit component, mock file or `apps/` file.

## How it was measured

- **Pages.** The nine pages of `mock/pages/`, in the order the DM uses them. Each one composes the Micro-UIs that
  `apps/screens/<screen>.json` lists; the Micro-UIs' real code is bundled in `mock/components/<Name>.js`, so what
  was rendered is what `apps/` draws, filled with the mock-up's fixtures.
- **Renderer.** Headless Chromium (Playwright) on macOS, `mock/` served by a static file server. Viewports
  **360 × 800** (phone), **768 × 1024** (tablet, portrait) and **1280 × 800** (desktop). Each page was loaded, left
  1.2 s to settle, and measured in its default state, with the DM's context the fixtures give (campaign *Les
  Cendres de Valombre*, session 4 live). The server answered 404 for the mock server's per-unit handler files
  (`handlers/*.js`); the pages fell back to their fixtures. That is an artefact of the static server, not a
  finding.
- **What each width means here.** The shared CSS changes layout at 960 px (`mock/style.css`: `.layout-2`,
  `.dm-layout`, `.entity-layout`, `.page-main` padding, the menu) and at 560 px (the menu padding). So **768 px
  renders the same single-column layouts as 360 px**, on wider panels; 1280 px renders the two-column ones.
  Widths between 961 and 1279 px (a tablet in landscape) were not rendered. The Micro-UIs add breakpoints of their own
  (`max-width` 760, 900, 960, 1100; `min-width` 961), the kit one at 420 px (`kit.css`), and the shell host one at 560 and 960 px
  (`apps/shell/src/client/shell.css`, not loaded by the mock pages); the renders include those of the pages and the kit.
- **Contrast** is the WCAG 2.x relative-luminance ratio of the token hex values of `mock/design/tokens.css`. For text
  in a page, the computed colour of every visible text node against the nearest opaque ancestor background;
  text over a gradient (the covers, the player-view) was sampled from the rendered pixels.
- **"Small target"** means an interactive element under 44 × 44 CSS px in either dimension (the common touch
  guideline; WCAG 2.2 AA asks for 24 × 24). It is given as a count, not as a failure.
- **Not rendered.** States the fixtures do not produce by themselves (an error, a refused command, a dialog, a
  map sent to the players beyond the one rendered in section 9) were read from the code of `apps/`, not seen. The
  screenshots were taken to read, and are not kept: this change adds one file.
- Heights below are the page's scroll height, in CSS px, for the 800 (1024 at tablet) px-high viewport.

### Reading the screen sections

Each screen has the same parts: what the DM does there; its three widths, each with **Fits**, **Overflows, wraps
or hides** and **Stays usable**; **What slows the DM down**, each point naming the element of the page; and **What
differs from the other screens**.

Every screen but the last shows the same site menu (`#site-header`, kit Menu: brand, *Campagnes*, *Médias*,
*Soundbox*, *Paramètres IA*). Its measures are in section 10.1 and apply to every
screen section that does not say otherwise.

---

## 1. campagnes

`pages/campagnes.html` — `CampaignList`.

### What the DM does there

Finds their campaigns and oneshots by cover, creates one (*Nouvelle campagne*), imports an archive (*Importer
une archive*), opens one (*Ouvrir*) or exports it (*Exporter*). It is the entry of the product.

### 360 px

- **Fits.** One column of three cards, each 328 px wide at `x = 16`; the document is 360 px wide, no horizontal
  overflow. The page is 1819 px high. The two toolbar buttons stack.
- **Overflows, wraps or hides.** The title of the third card wraps to two lines. The fourth menu link, *Paramètres
  IA*, is entirely off screen: the nav scrolls sideways (344 px visible of 479 px) with its scrollbar hidden.
- **Stays usable.** Every action is reachable by scrolling. 9 of the 13 interactive elements are under 44 px.

### Tablet — 768 px

- **Fits.** Two columns of cards, 356 px wide (at `x = 16` and `x = 396`); the third card sits alone on the second
  row. No overflow. 1243 px high. The four menu links all fit.
- **Overflows, wraps or hides.** The menu is on two rows (brand, then nav): the sticky header is 98 px high, as at
  360 px.
- **Stays usable.** Yes. 9 of 13 interactive elements under 44 px.

### 1280 px

- **Fits.** Three cards 282 px wide at `x = 40 / 346 / 652`, on one row, with the toolbar buttons beside the
  section title. The page is 834 px high, a viewport plus 34 px.
- **Overflows, wraps or hides.** The three cards fill 894 px (x = 40 to 934) of the 1200 px content width; 306 px on the right
  stay empty. Two cards have a two-line title and one a one-line title: the description of the first starts higher
  than the other two.
- **Stays usable.** Yes. The header is 76 px (menu on one row). All 13 interactive elements are under 44 px
  in one dimension.

### What slows the DM down

- **`.kit-card` ×3, button *Ouvrir*.** All three cards carry the same filled primary button; with *Nouvelle
  campagne* that is four filled orange buttons at once at 1280 px, plus the filled *Campagne* badge of two of the cards. The card of the next session is not
  distinguished from the old campaign (*Les héritiers de la Tour d'Ébène*, 23 séances) except by a text line
  (*Prochaine séance : 17 octobre 2026*).
- **Card meta line.** It begins with a middle dot (`· jouée le 3 octobre 2026`) and is 14 px.
- **Card cover.** The cover is a gradient with one initial letter (V, C, É); it carries no information the title
  does not.
- **`h2` *Campagnes et oneshots*.** It sits right under the `h1` *Vos campagnes* and repeats it; the page's two
  actions are on the `h2` row.
- **Menu, link *Paramètres IA* at 360 px.** Off screen, with no visible cue that the nav scrolls.

### What differs from the other screens

The only page that is a grid of repeated cards at page level. No breadcrumb (also absent from *medias*,
*soundbox*, *parametres-ia* and the DM view). The only page whose single action on each item (*Ouvrir*) is the
primary button. No destructive action.

---

## 2. campagne

`pages/campagne.html` — `CampaignOverview`, `SessionList`, `CampaignElements` (tabs), `ToCreateList`,
`NoteImport`.

### What the DM does there

Browses the sessions and the elements of one campaign, launches (*Lancer la séance 4 en direct*) or prepares (*Préparer
la séance 4*) a session, creates a session (*Nouvelle séance*), imports their notes (*Extraire les éléments*) and
treats the entities to create (*Créer depuis le brief*, *Affiner*, *Ouvrir la fiche*).

### 360 px

- **Fits.** The hero (`CampaignOverview`, 328 px), the elements tabs, *Entités à créer* and *Importer des notes*
  fit in 328 px. 4819 px high.
- **Overflows, wraps or hides.** **The document is 637 px wide.** The sessions panel (`#session-list`) is 621 px
  wide, its four items 571 px, their text 477 px. The primary button `#sessions-new` (*Nouvelle séance*, right
  edge at 612 px) and the badges *JOUÉE* / *SAM. 5 SEPTEMBRE* (right edge 595 px) are off screen to the right;
  the summaries and the *PJ présents* lines are cut at the viewport edge. The six element tabs (*Joueurs (4)*,
  *Personnages (9)* …) wrap their label onto two lines. The hero counters wrap onto three lines, and
  *Lancer la séance 4 en direct* wraps onto two.
- **Stays usable.** Only by scrolling sideways: the create button and the right half of every session are beyond
  the edge. 19 of 41 interactive elements under 44 px.

### Tablet — 768 px

- **Fits.** No overflow: one column, panels 736 px wide, the session items 686 px. In the hero the cover (237 px)
  is beside the text (which starts at `x = 302`); at 360 px the cover (326 × 200 px) is above it. 3700 px high.
- **Overflows, wraps or hides.** *Entités à créer* and *Importer des notes* fall below the elements tabs (they
  are the right column at 1280 px): the DM reaches the entities to create after the sessions and the elements.
- **Stays usable.** Yes. 20 of 41 under 44 px.

### 1280 px

- **Fits.** Hero across 1200 px; then two columns, 784 px (sessions, elements tabs) and 392 px (*Entités à
  créer*, *Importer des notes*). 2243 px high; the sessions fill the first viewport.
- **Overflows, wraps or hides.** The tab *Personnages (9)* wraps onto two lines inside its pill.
- **Stays usable.** Yes. 24 of 41 under 44 px.

### What slows the DM down

- **Hero buttons `Lancer la séance 4 en direct` / `Préparer la séance 4` and session-4 buttons `Lancer en
  direct` / `Préparer`.** The same two actions twice on the first screen; eight filled orange elements at 1280 px
  (those two, *Nouvelle séance*, the active tab *Personnages (9)*, two *Créer depuis le brief*, *Extraire les
  éléments*, and the filled *Campagne* badge of the overview).
- **Active tab `PERSONNAGES (9)`.** The selected tab has the same orange fill as the action buttons.
- **Sessions 1–3, button *Résumé et journal*.** The only action on a played session is a text button with no fill
  and no outline.
- **`ToCreateList` items.** Each repeats *Séance 4 — L'embuscade du Gué* and a quote; the two actions beside it
  (*Créer depuis le brief*, *Affiner*) have two different looks (filled, outlined).
- **Add buttons (*Nouveau PNJ*, *Nouveau lieu*, *Nouvel événement*).** Three actions of the same kind in three
  looks: outlined, then two bare-text.
- **Textarea *Notes en texte libre*.** Shows about 7 lines of a longer text and cuts it at its own height (a resize handle only).
- **Sticky header at 360 / 768 px.** 98 px of the viewport (12%).

### What differs from the other screens

The only page with five Micro-UIs and a hero panel carrying the page's actions (the `h1` is the campaign's name,
not a page title; other pages put actions on a `page-head`). With *soundbox*, one of the two pages whose layout
breaks at 360 px by overflow. The only page that shows a session's life cycle as badges (*JOUÉE*, *EN PRÉPARATION*,
dates) and a `1…4` counter disc in accent.

---

## 3. fiche-entite

`pages/fiche-entite.html` — `CampaignElements` (as the rail), `EntityEditor`, `VersionHistory` (a dialog).

### What the DM does there

Fills in a character, a place or an event (identity, *personnage* PNJ/PJ, description), dictates the initial prompt
and has the AI propose background, behaviour and statblock, imports a PJ from D&D Beyond, tunes the table's token
and portrait, and undoes an AI change from the versions history (*Mis à jour*). *Enregistrer la fiche* saves.

### 360 px

- **Fits.** One column, 328 px, no overflow; four panels. 4844 px high. The rail is folded into a button
  `☰ Éléments` (109 px wide, 31 px high).
- **Overflows, wraps or hides.** The whole elements list (search, PJ / PNJ groups, *Nouveau PNJ / lieu /
  événement*) is hidden until the drawer opens.
- **Stays usable.** Yes, but **the folded rail button sits at `top: 8px` under the sticky header (98 px)**: after
  scrolling, the button is behind the header (measured at scroll 900: the element at the button's centre is
  `#site-header`). 29 of 40 interactive elements under 44 px.

### Tablet — 768 px

- **Fits.** Same single column on 736 px panels; no overflow; 3755 px high. The rail is the same folded button.
- **Overflows, wraps or hides.** Same as 360 px: the rail is hidden, and its button is covered by the 98 px header
  once the page is scrolled (measured at scroll 900).
- **Stays usable.** Yes. 29 of 40 under 44 px.

### 1280 px

- **Fits.** Three columns: the open rail (280 px, 791 px high), the editor panels (581 px) and the *À la table*
  panel (291 px). 2912 px high; header 76 px.
- **Overflows, wraps or hides.** The rail is sticky at `top: 16px` while the header is 76 px high: at scroll 900
  the top 60 px of the rail (its title, the fold button and the search field) lie behind the header. The
  background and behaviour textareas cut their text (fixed row count). *Enregistrer la fiche* is at 2792 px, the
  very bottom of the page.
- **Stays usable.** Yes. 42 of 65 under 44 px.

### What slows the DM down

- **`Enregistrer la fiche` (button, bottom of the page).** At 1280 px it is 2792 px down, and 4800 px at 360 px;
  it is not sticky. The page shows a *modified* marker (`.mf-entity__unsaved`) in the editor, not at the button.
- **`button#versions-updated` (*Mis à jour*).** A state (the AI changed this sheet) drawn as a filled orange
  button, identical to *Proposer background, comportement et statblock* and *Enregistrer la fiche*.
- **`#entity-prompt-text` and its generate button.** *Proposer background, comportement et statblock* is a long
  label on a 328 px screen; the results (*Background*, *Comportement, pour l'interpréter*) are two more textareas of
  the same look as the prompt, so the AI's output and the DM's input are not told apart by form.
- **`kit-statblock__abbr` (*FOR, DEX, CON, INT, SAG, CHA*).** 14 px, `--color-secondary` on `--color-surface-2`:
  3.19:1 (see 10.2).
- **Sliders in *À la table* (*Recadrage — zoom*, *Centre horizontal*, *Centre vertical*).** Each shows its value
  (`150 %`, `50 %`, `38 %`) in accent beside the label and the 0 / 100 % ends below the track: three lines per
  slider.
- **Rail, row *Nouveau PNJ / Nouveau lieu / Nouvel événement*.** One outlined, two bare-text.

### What differs from the other screens

The only page with a rail (`CampaignElements` in `rail` mode) beside the content; the only one that asks
*confirmer avant de quitter* when a sheet is modified (R95); the only one where the page's `h1` is a content name
with a portrait ring beside it. On this page the statblock (`kit-statblock`) is shown a second time below the form, as the sheet
will read.

---

## 4. preparation-seance

`pages/preparation-seance.html` — `PreviousSummary`, `OralBrief`, `VersionHistory`, `ToCreateList`, `SceneBoard`,
`Battlemap`.

### What the DM does there

Rereads the previous session (summary, open threads), tells the brief of the session at the microphone, checks the
scenes and the entities the AI detected, creates the missing ones, adds the PJ present, links a battlemap to a
scene and places the tokens, then *Lancer la séance en direct*.

### 360 px

- **Fits.** One column, 328 px, no overflow; seven panels. **8818 px high** (11 viewports), the longest page. The
  battlemap preview is 278 × 231 px.
- **Overflows, wraps or hides.** Nothing hidden. The right column of 1280 px is placed after the scenes: *Fiches
  modifiées par le brief* at 6503 px, *Entités à créer* at 6710 px, the battlemap at 7830 px, the brief at 1593 px.
- **Stays usable.** Yes. 59 of 82 interactive elements under 44 px. The zoom buttons (− / *Ajuster* / +) and the
  scene arrows (▲ ▼) are small.

### Tablet — 768 px

- **Fits.** One column, panels 736 px; 6167 px high; no overflow.
- **Overflows, wraps or hides.** Same order as 360 px: brief at 952 px, *Entités à créer* at 4271 px (3300 px
  after the brief that detects them), battlemap at 5050 px.
- **Stays usable.** Yes. 72 of 82 under 44 px.

### 1280 px

- **Fits.** Summary across 1200 px; the brief (784 px) beside *Fiches modifiées* and *Entités à créer* (392 px); the
  scenes and battlemap across 1200 px. 4577 px high; brief at 749 px and *Entités à créer* at 956 px, side by side.
- **Overflows, wraps or hides.** The battlemap is the last block, at 3779 px (83% down). The brief's result
  columns sit side by side in 784 px.
- **Stays usable.** Yes. 76 of 82 under 44 px.

### What slows the DM down

- **`OralBrief` result columns.** Three columns (*3 scènes créées*, *6 entités reconnues*, *3 manquantes*) of small text
  (13.33 px `small` lines) with badges (*EXISTE*, *À CRÉER*, *MIS À JOUR*); the *Voir le diff* link and a *Mis à jour*
  button sit inside the recognised-entities column.
- **Button *Terminer et analyser* (`#brief-analyse`).** A primary button, disabled while nothing is recorded: opacity .55,
  its text about 3.3:1.
- **`ToCreateList` far from the brief.** At 360 and 768 px the entities to create are 3300–5100 px below the
  brief that lists them as *À CRÉER*.
- **`SceneBoard`, each scene block.** The same four lines repeat in the scenes without a map: *Aucune battlemap
  pour cette scène*, a select, *Créer l'instance*, a 13 px explanation. The scene's chips (*lieux*, *PNJ*,
  *événements*, *médias*, *à créer*) are outlined pills whose border colour tells the kind (accent, secondary,
  primary, dashed accent).
- **Battlemap tokens `kit-token__initials`.** White initials (20 px) on bright discs: 1.54–2.69:1 (see 10.2).
- **`Battlemap` as the last panel.** It needs the scene's instance (*Placer les personnages*) but sits below all
  scenes; the characters it places (*Personnages de la scène*) are listed in its own side block.
- **Panel count.** Seven `.panel`s, 8 `h2`, 13 `h3` and 82 interactive elements on one page.

### What differs from the other screens

The longest and densest page (7 panels, six Micro-UIs). The only page that combines the microphone with a
review step (*Brief oral*), and the only one where the Battlemap is a section of the page, not a tab or a full
screen. Its page head carries the *Lancer la séance en direct* primary action, and it is the page that shows `kit-token` discs by
default.

---

## 5. medias

`pages/medias.html` — `MediaLibrary`, `GridSettings`.

### What the DM does there

Uploads battlemaps and illustrations (type, optional place, title), attaches a place to a media or a media to the
current session, then adjusts the hexagonal grid of a battlemap (size, offsets, orientation, colour, opacity)
with a live preview, and saves the setting.

### 360 px

- **Fits.** One column, 328 px, no overflow. 5118 px high. The upload zone and its form stack; the seven media
  cards stack.
- **Overflows, wraps or hides.** The grid panel (`#grid-title`) is at 3440 px, below the 7 cards; the preview is
  under the four sliders.
- **Stays usable.** Yes. 30 of 36 interactive elements under 44 px.

### Tablet — 768 px

- **Fits.** One column, panels 736 px; 3665 px high; no overflow.
- **Overflows, wraps or hides.** Nothing hidden, no overflow.
- **Stays usable.** Yes. 28 of 36 under 44 px.

### 1280 px

- **Fits.** Two full-width panels (1200 px). In the library the upload zone (left) and the form (right) share a
  row; the two battlemaps are on one row, the five illustrations on another. 2497 px high.
- **Overflows, wraps or hides.** Under the upload zone there is a blank area (the form on the right is taller);
  the place selects of the illustration cards cut their option (*La Taverne du Sangli…*). Select values and
  file-size lines wrap inside the 217 px cards.
- **Stays usable.** Yes. 32 of 36 under 44 px.

### What slows the DM down

- **`Régler la grille` (card button) → `#grid-title` panel.** Choosing a battlemap acts on a panel 1551 px lower
  at 1280 px (3440 px at 360 px): the result is outside the screen that holds the click.
- **Cards `Associé à la séance 4` / `Ajouter le lieu à la séance`.** One is an outlined chip, the other a
  bare-text button two lines high; the *via Valombre* line beside the chip is 13.33 px.
- **Battlemap cards.** The selected one has a primary border and a filled *Régler la grille*; the other an
  outlined one: the same button changes look with selection.
- **Slider rows (*Taille d'un hex*, *Décalage horizontal / vertical*, *Opacité*).** Each is a label, a value in
  accent, the track and a min / max line.
- **`Annuler les changements`.** A bare-text button under the primary *Enregistrer le réglage*.

### What differs from the other screens

The only page with sliders, a live preview and a media grid; the only one where a selected card changes the
content of a different panel. At 1280 px both panels are full-width: it does not use the 2/3 + 1/3 split of
*campagne*, *soundbox* and *parametres-ia*. The type radio's hint (*grille hexagonale*) is cyan (see 10.2).

---

## 6. soundbox

`pages/soundbox.html` — `SoundLibrary`, `LiveSoundbox`.

### What the DM does there

Uploads sound effects, names and tags them, edits or deletes one (*Modifier*); plays sounds, several at once,
sets a per-sound and a global volume, filters by tag or name and stops everything (*Tout arrêter*).

### 360 px

- **Fits.** The panels of the page are 328 px; the live soundbox tiles are 328 px wide, one per row (12 tiles);
  4431 px high.
- **Overflows, wraps or hides.** **The document is 724 px wide.** The library table (`#sounds-table-grid`) is 683
  px wide inside a 328 px panel with no scroll container: of its columns only *Nom* and *Tags* are in view;
  *Fichier*, *Durée*, *Volume* and the *Modifier* button (the table's only action) are off screen. The search
  input `#sounds-search-input` is 683 px wide; the counter *12 sons · 17 tags* ends at 724 px.
- **Stays usable.** The live soundbox is usable; the library's editing is reachable only by scrolling the page
  sideways. 78 of 95 interactive elements under 44 px.

### Tablet — 768 px

- **Fits.** No overflow: the table (736 px panel) shows all columns; 3120 px high. The live tiles are 3 per row,
  235 px.
- **Overflows, wraps or hides.** Nothing: no overflow and no tile clipped (at 1280 px all 12 tiles clip their content).
- **Stays usable.** Yes. 78 of 95 under 44 px.

### 1280 px

- **Fits.** The library (784 px) and the upload panel (392 px) side by side; below, the live soundbox across 1200
  px with the tag chips on two rows and 12 tiles, 6 per row, 187 px wide. 1986 px high.
- **Overflows, wraps or hides.** All 12 tiles clip their content (their scroll width exceeds their box): *Bruit de foule en taver…*, *porte · bois · intérieur ·
  2,4 …*, *Hurlement de loup … 4,5…*; the 187 px tiles cut the title and the meta line.
- **Stays usable.** Yes. 82 of 95 under 44 px.

### What slows the DM down

- **Two `Soundbox` blocks.** The `h1` *Soundbox* and the `h2` *Soundbox* (second block) both read the same, and
  the same 12 sounds are listed twice (`#sounds-table-grid` and the tiles), each with its own search field
  (`#sounds-search-input`, *Rechercher un son*).
- **Live soundbox below the table.** The player of the sounds starts at 1414 px at 1280 px (2118 px at 768, 2241 px at 360), under the
  12 rows of the library (the intro says sounds are played from the DM view tab, or from a click when the microphone hears one named).
- **Table `.kit-table`.** Header filled in primary orange with dark text; the first column in accent; *Durée*
  and *Volume* in `--color-success` green; tags in outlined chips: four colours in one table.
- **Row button *Modifier*.** Dark ghost button on every row (12), the only way to delete (*Supprimer*, in the
  edit panel).
- **Tile `Vol.` slider.** Each of the 12 tiles carries its own volume slider inside the clipped tile.
- **Button `Tout arrêter`.** A filled red button at the right of the global volume slider, in the live soundbox, with no confirmation.

### What differs from the other screens

The only page with a data table, and the only one that lists the same data twice. With the DM view, one of the two pages
with a red filled button (*Tout arrêter*, *Supprimer*). The only page whose volume values are
shown in green (success) as plain data.

---

## 7. parametres-ia

`pages/parametres-ia.html` — `AiSettings`.

### What the DM does there

Chooses between a cloud API and the binary / endpoint of their own server (*Mode de connexion*), fills in URL, API
key and the generation and summary models, tests the connection (*Tester la connexion*) and saves (*Enregistrer*).

### 360 px

- **Fits.** One column, 328 px, no overflow; the form fields stack; 2108 px high. The privacy panel (*Ce qui sort de
  la machine*) is under the form.
- **Overflows, wraps or hides.** Nothing; the four inputs are 278 px wide, one per row.
- **Stays usable.** Yes. 9 of 13 interactive elements under 44 px.

### Tablet — 768 px

- **Fits.** One column, 736 px; fields in two columns of 335 px (URL / key, then the two models); 1380 px high.
- **Overflows, wraps or hides.** Nothing.
- **Stays usable.** Yes. 8 of 13 under 44 px.

### 1280 px

- **Fits.** The form panel (784 px) and the privacy panel (392 px); the whole page is 931 px, one viewport plus 131
  px.
- **Overflows, wraps or hides.** Nothing.
- **Stays usable.** Yes. 12 of 13 under 44 px high.

### What slows the DM down

- **Radio group *Mode de connexion*.** Each option's hint (*une URL, le type d'API est détecté*) is right-aligned
  and the checked one's is cyan; at 360 px the hint wraps under the label.
- **Badge *Type d'API détecté : endpoint opencode du serveur maison*.** A green outlined uppercase Impact badge placed between the
  fields and the buttons.
- **`Tester la connexion` / `Enregistrer`.** The test result (*Dernier test réussi le 14 octobre 2026 à 19:02 —
  réponse en 0,84 s.*) appears below the buttons, in a box with a green outline, and no state is shown on the
  buttons themselves.
- **Key field.** The helper says the stored key is never sent back to the screen, so the field cannot show whether one is stored;
  its placeholder reads *Aucune clé*.

### What differs from the other screens

The lowest-density page (13 interactive elements) and the only one that is purely a settings form with no
entities, no list and no media. The privacy panel is the only place where list ticks (✓ in `--color-success`) and
bullets (• in `--color-secondary`) are used.

---

## 8. dm-view

`pages/dm-view.html` — `LiveSessionBar`, `ElementsPanel`, `TableSheet`, `Battlemap`, `LiveSoundbox`,
`SuggestionPill`. R98: the suggestions and the microphone's play / pause button form an overlay anchored at the
bottom right of the window; R17: elements on the left, the opened sheet or the battlemap in the centre.

### What the DM does there

Runs the session: opens the elements the microphone suggests, reads the sheet at the table, moves the tokens
and runs the combat, plays sounds, sends maps and illustrations to the players (*Ouvrir la player view*, *Envoyer
l'illustration*), and closes the session (*Clore la séance*).

### 360 px

- **Fits.** One column, 328 px, no overflow. 3332 px high. The sticky header (menu + `LiveSessionBar`) is **517 px
  high**; the overlay `#suggestions` is **320 × 524 px** at `x = 24, y = 260`, over the bottom of the 800 px
  viewport.
- **Overflows, wraps or hides.** The header's scene chips, badges and buttons wrap onto many rows. The centre (tabs
  *Fiche / Battlemap / Soundbox*) comes after the whole elements panel: the tab bar is 2256 px down the page. The
  *Battlemap* and *Soundbox* panels are hidden until their tab is selected.
- **Stays usable.** Hardly: the header (0–517 px) and the overlay (260–784 px) together cover 784 of the 800 px of the
  viewport; the page content shows only in the 16 px under the overlay. 61 of 90 interactive elements under 44 px.

### Tablet — 768 px

- **Fits.** One column, 736 px, no overflow; 2621 px high. Header 335 px (the scene chips wrap to two rows; three
  controls on a row below).
- **Overflows, wraps or hides.** The overlay (320 × 524 px at `x = 432`) covers the right 320 px of the left panel
  (in the 1024 px-high render the *Scène / Fenêtre* chips of the *Médias* rows are under it); the tab bar is 1906 px down.
- **Stays usable.** Partly: the overlay stays on top of the scroll position, the header takes 335 px. 75 of 90
  under 44 px.

### 1280 px

- **Fits.** Two columns: the elements panel (440 px, sticky, scrolls inside, up to 760 px) and the centre (792
  px). 1023 px high. Header 231 px (menu 76 px + the session bar 155 px on three rows).
- **Overflows, wraps or hides.** The overlay (`x = 944–1264`, `y = 260–784`) covers the *Soundbox* tab (the element
  at its centre is the overlay), the right of the sheet's title (*LA TAVERNE DU SANGLI…*) and the end of its action
  row (*Envoyer l'illustration* is cut). The *Battlemap* and *Soundbox* panels are hidden until their tab is
  selected.
- **Stays usable.** Yes, once the right 320 px are accepted as covered. 79 of 90 under 44 px.

### What slows the DM down

- **Sticky `#site-header` (menu + live bar).** 231 px of 800 px at 1280 px (29%), 335 px at 768 px, 517 px at 360
  px: the always-visible area for the sheet shrinks by that much. The menu (*Campagnes*, *Médias*, *Soundbox*,
  *Paramètres IA*) is in it during a live session.
- **`#suggestions` overlay.** Covers the *Soundbox* tab and the sheet title at 1280 px; its first row carries an
  accent outline; the *Micro de séance* button under it is another floating pill.
- **Session bar controls.** *Ouvrir la player view* (outlined), *Player view pilotée* (bare text) and *Clore la
  séance* (red) sit side by side on one row, the session-ending action next to a harmless one; *Clore la séance*
  opens a confirmation dialog.
- **`ElementsPanel` rows.** Three nesting levels (scene → group → element), small `Scène` / `Fenêtre` chips on the *Médias* rows, and
  drag handles (⠿) on the PNJ rows.
- **Tabs `FICHE / BATTLEMAP / SOUNDBOX`.** The active tab is a filled orange pill (one third of the bar); at 360 and 768 px the tabs are
  reached after the elements panel.
- **`TableSheet` text.** Sheet body at 20 px, the largest running text of the product (read at the table);
  its actions (*Modifier*, *Envoyer l'illustration*) are 31–42 px high, bare or outlined.

### What differs from the other screens

No footer, no breadcrumb and no `.panel` (surfaces are the rail and the centre card). It is the only page with a
fixed overlay, with tabs, with a sticky rail and with a header made of the site menu and a live-session bar. The
only page whose single red button ends something (*Clore la séance*).

---

## 9. player-view

`pages/player-view.html` — `PlayerStage` (and `Battlemap` when a map is sent). **Judged as the players read it:
a second screen on the table, looked at from a distance, never worked in.** The DM does not use it as a screen
to work in (they drive it from the DM view, or from the synchronised tab, R113).

### What the DM does there

Nothing on this screen: it is projected. The players see the campaign's cover when nothing is sent, or the map, the
illustration (full-screen scene mode) or a floating window the DM sends (R24). In the DM-driven tab the DM moves
tokens, the window and the framing.

### 360 px

- **Fits.** Cover state: the whole page is the 360 × 800 viewport (`height: 100vh; overflow: hidden`), 0 interactive
  elements. The title `h1` is 64 px (Impact, `--color-primary`) over the dark lower hill: it wraps to three lines
  (*LES / CENDRES DE / VALOMBRE*); the subtitle *Séance 4 — L'embuscade du Gué* is 20 px.
- **Overflows, wraps or hides.** The title and subtitle boxes run from 40 px to the right edge (360 px) with no right
  margin: *CENDRES DE* touches the screen edge. A sent map keeps its tokens at a fixed size (20 px initials, 14 px
  names, 11.2 px status), so on a 360 px map a disc (about 58 px) is 16% of the width.
- **Stays usable.** As a read-only picture, yes. Nothing scrolls.

### Tablet — 768 px

- **Fits.** Cover: the 128 px initial, the 64 px title on one line (box 40–756 px) and the 20 px subtitle. Map: 768 ×
  1008 px.
- **Overflows, wraps or hides.** Nothing hidden; the same fixed sizes as at 360 px (64 / 20 px; 20 / 14 / 11.2 px
  on the tokens).
- **Stays usable.** Yes.

### 1280 px

- **Fits.** The cover fills the viewport; the title is on one line at the bottom left, 64 px high, the subtitle 20
  px; the 128 px initial is in the middle. A sent map fills 1280 × 784 px.
- **Overflows, wraps or hides.** On the map, the name of a token overlaps its neighbours when characters start
  close together (*Kael* is hidden under *Sirielle*); status lines (*Indemne*) are 11.2 px.
- **Stays usable.** Yes.

### Read at a distance

- **Sizes do not follow the viewport.** The title is 64 px and the subtitle 20 px at 360, 768 and 1280 px; the
  tokens' initials, names and status are 20, 14 and 11.2 px on every width. At 1280 px the title is 8% of the viewport height and the subtitle 2.5%; the 14 px names and 11.2 px status are the smallest text on the screen.
- **Contrast of the cover's text** (sampled pixels): the title `--color-primary` over the dark lower hill is about
  5.4–6.0:1; the subtitle `--color-text` about 15:1. The 128 px initial is a translucent glyph on the gradient.
  **Contrast on the map**: token initials in white on the bright discs 1.54–2.69:1 (10.2); names in white with a
  text shadow on a dark map.
- **Corner status line.** If the diffusion cannot be read, `#player-status` shows *Impossible de lire la
  diffusion.* in a 14 px, red-outlined box at the bottom left: DM-facing wording on the players' screen, at the
  smallest text size.
- **No chrome.** No menu, no controls, no notes, no hit points, no hidden token (the page's own comment), so there
  is no navigation to get out of the view.

### What slows the DM down

- **Nothing slows the DM on this screen,** they do not work in it. What costs the table, reading from a distance, is
  named element by element:
- **`h1` (the cover's title, 64 px).** At 360 px it wraps onto three lines and its box has no right margin.
- **`#player-status` (14 px red-outlined line).** The DM's wording (*Impossible de lire la diffusion.*) on the players' screen,
  at the smallest size of the cover state.
- **`kit-token__name` (14 px) and the status line under it (*Indemne*, 11.2 px).** The smallest text of a sent map; names
  overlap where tokens start close (*Kael* under *Sirielle*).
- **`kit-token__initials` (20 px white on a bright disc).** 1.54–2.69:1.

### What differs from the other screens

The only page with no menu, no footer, no interactive element and no scrolling; the only one that is
`height: 100vh`; the only one with a 64 px and 128 px type size. It is the only screen read by someone other than the
DM.

---

## 10. Cross-screen findings

### 10.1 The frame every DM screen shares

- **Site menu** (kit Menu in `#site-header`, sticky, `z-index: 50`). 76 px high at 1280 px; 98 px at 360 and 768 px
  (the nav is a second row). At 360 px the nav is 479 px wide in 344 px: *Paramètres IA* is not visible and the nav
  scrolls with the scrollbar hidden. The current page is a filled orange pill.
- **Page head.** `h1` in `--color-primary` Impact 32 px uppercase, then a `--color-text-muted` paragraph (max 70ch) on
  *campagnes*, *medias*, *soundbox*, *parametres-ia*; a breadcrumb on *campagne*, *fiche-entite*,
  *preparation-seance* only; the `h1` is a content name on *campagne*, *fiche-entite*, *dm-view*, *player-view*.
- **Footer** (`.page-footer`, 14 px muted) on the seven pages other than *dm-view* and *player-view*.
- **Layout grid.** `.layout-2` is 2fr + 1fr at 1280 px (784 + 392 px) on *campagne*, *preparation-seance*, *soundbox*,
  *parametres-ia*; *medias* uses two full-width panels; *fiche-entite* uses rail + editor + side panel; *dm-view* its own
  grid. All single column at ≤ 960 px.
- **Heights and small targets** (interactive elements under 44 px / all):

| screen | 360 px: height, small | 768 px: height, small | 1280 px: height, small |
|---|---|---|---|
| campagnes | 1819, 9/13 | 1243, 9/13 | 834, 13/13 |
| campagne | 4819, 19/41 | 3700, 20/41 | 2243, 24/41 |
| fiche-entite | 4844, 29/40 | 3755, 29/40 | 2912, 42/65 |
| preparation-seance | 8818, 59/82 | 6167, 72/82 | 4577, 76/82 |
| medias | 5118, 30/36 | 3665, 28/36 | 2497, 32/36 |
| soundbox | 4431, 78/95 | 3120, 78/95 | 1986, 82/95 |
| parametres-ia | 2108, 9/13 | 1380, 8/13 | 931, 12/13 |
| dm-view | 3332, 61/90 | 2621, 75/90 | 1023, 79/90 |
| player-view | 800, 0/0 | 1024, 0/0 | 800, 0/0 |

- **Horizontal overflow at 360 px:** *campagne* (document 637 px) and *soundbox* (724 px). None at 768 or 1280 px.
- **Button looks.** Four looks of one component: filled orange (primary), outlined orange (secondary), bare text with
  no fill and no outline (ghost / `kit-button--bare`: *Modifier*, *Résumé et journal*, *Exporter l'archive*,
  *Player view pilotée*, *Annuler les changements*, *Ajouter le lieu à la séance*), and filled red (danger: *Clore la
  séance*, *Tout arrêter*, *Supprimer*, *Retirer de la carte*, *− Dégâts*). Filled orange is also used for the
  active tab, the current menu link, the state button *Mis à jour* and the table header of *soundbox*.
- **Filled-orange elements visible at 1280 px** (buttons and pills, menu excluded; the kit badge `.kit-badge .primary`, `kit.css:136`,
  is a filled orange pill: the *Campagne* badge of campaign-list cards, two of the three in the fixtures, and the one of campaign-overview):
  *campagnes* 6, *campagne* 8, *fiche-entite* 3,
  *preparation-seance* 4, *medias* 3, *soundbox* 1, *parametres-ia* 1, *dm-view* 1 (the active tab).

### 10.2 Contrast of the tokens on the dark surfaces

Tokens (`mock/design/tokens.css`): `--color-surface #120a1c`, `--color-surface-2 #1f1331`. Ratios are WCAG 2.x; AA is
4.5:1 for body text, 3:1 for large text (≥ 24 px, or ≥ 18.66 px bold) and for UI components.

| token | hex | on `--color-surface` | on `--color-surface-2` | AA body text (4.5) | AA large text / UI (3) |
|---|---|---|---|---|---|
| `--color-text` (body text) | `#f5ecf7` | 16.78 | 15.24 | pass | pass |
| `--color-text-muted` | `#b9a6c9` | 8.62 | 7.83 | pass | pass |
| `--color-accent` | `#f5b83d` | 10.87 | 9.87 | pass | pass |
| `--color-primary` | `#e8702a` | 6.25 | 5.67 | pass | pass |
| `--color-secondary` | `#8b3fd9` | 3.51 | 3.19 | **fail** | pass |
| `--color-success` | `#3ecf8e` | 9.69 | 8.80 | pass | pass |
| `--color-danger` | `#ff5a5f` | 6.34 | 5.75 | pass | pass |
| `--color-border` | `#3d2b57` | 1.55 | 1.41 | **fail** | **fail** (UI components) |

- **`--color-text-muted` and the body text pass AA on both dark surfaces** at every size, including the 13.33 px and
  11.2 px text. Neither fails.
- **Fails on the dark surfaces:** `--color-secondary` as text (4.5:1 needed; 3.51 and 3.19). It is used as text for the
  statblock abbreviations `kit-statblock__abbr` (*FOR … CHA*, 14 px: 3.19:1 on `--color-surface-2`; 6 elements on
  *fiche-entite*) and for the bullets of the privacy panel. `--color-border` (1.55 / 1.41) is below 3:1 as the outline
  of inputs, cards, chips and panels; the surfaces themselves differ by 1.10:1.
- **Text on a filled token** (not a dark surface): `--color-primary-contrast #14081f` on primary 6.26, accent 10.89,
  success 9.70, danger 6.35, **secondary 3.51**; `--color-text` on primary 2.69, accent 1.54, success 1.73, danger 2.65,
  secondary 4.78.
- **Token initials** (`kit-token__initials`, 20 px bold Impact, `--color-text`) on the bright discs: success 1.73,
  primary 2.69, accent 1.54, danger 2.65. 8 text nodes on *preparation-seance* and, on a sent map, the discs of the
  *player-view*; `style.css` has an `.is-dark` variant (`--color-primary-contrast`) that these discs do not use.
- **Reduced opacity.** The input placeholder (`--color-text-muted` at .7) is 4.70 on `--color-surface` and 4.48 on the
  inputs' `--color-surface-2`. A disabled button or input (`.55` / `.6`) is 3.3–3.8.
- **A colour outside the tokens.** `kit.css` reads `--color-accent-cyan` seven times (focus ring of buttons, inputs,
  selects, checkboxes, radios, toggles; the checked radio's hint text); `tokens.css` does not define it, so the
  fallback `#27d7f0` is what renders (11.11 on surface, 10.09 on surface-2).
- **Automated pass** over every visible text node at all three widths: 14 failing nodes in total (6 on *fiche-entite*,
  8 on *preparation-seance*), the same at 360, 768 and 1280 px; none on the other seven pages. Text over the cover
  gradients was sampled separately (section 9).

### 10.3 Where the accent colour is used

`--color-accent #f5b83d` (amber). Every declaration that reads it, by file; `--color-primary` is the orange of
the buttons and `h1`, which is not counted here.

**Global (`mock/style.css`, 14).** Links (`a`); every `h2` (text); hover colour of `.chip` and `.icon-btn`;
`.chip--todo` (border and text, dashed); `.chip--lieu` (border); focus outline of `.num-input`; the
`confirmed` state of the command toast (left border and state text); the index page's `b`; the illustrations'
`--art-a` / `--art-b` gradient stops (`.art--gold`, `.art--dusk`); the battlemap's `.bm-ford` fill.

**Kit (`mock/kit/kit.css`, 37).**
- Text: `.kit-card h3`; `.kit-select > span`; `.kit-checkbox legend`; `.kit-radio legend`; `.kit-table tbody td:first-child`;
  `.kit-badge .accent`; `.kit-slider .kit-slider__value`; `.kit-dialog__title`; `.kit-diffview__title`;
  `.kit-statblock__vital dt`, `.kit-statblock__feature strong`; `.kit-filedrop__tag`; `.kit-textarea__counter.is-near`;
  `.kit-token__num`, `.kit-token__badge--br`; `.kit-micbutton[data-state="paused"]` (button text and status);
  `.kit-button .ghost:hover`.
- Fills: `.kit-menu .end button` (border and background); `.kit-breadcrumb [aria-current="page"]`;
  `.kit-token__gauge-fill--mid`; `.kit-statblock__rule`; `.kit-dialog__btn--primary:hover`.
- Borders and shadows: `.kit-table th` (bottom border); `.kit-badge .accent`; `.kit-token__badge--br`;
  `.kit-token__item--current .kit-token__disc` (border and glow); micbutton paused (border).
- Focus rings: menu links, tabs, breadcrumb, checkbox-in-diffview, micbutton.
- Map: `.hm-hill` (fill), `.hm-road` (stroke); a gradient stop at `kit.css:593`.

**Micro-UIs (`apps/*/*/src/style.ts`, 81).**
- `h2` text in the style of 17 Micro-UIs: ai-settings, note-import, oral-brief (also its `h3`), to-create-list,
  campaign-elements, campaign-list, entity-editor, version-history, suggestion-pill, grid-settings, media-library,
  previous-summary, scene-board, session-list, live-soundbox, sound-library, battlemap.
- Hover colour of `.chip` / `.icon-btn` / ghost links in: campaign-elements, entity-editor, elements-panel, suggestion-pill,
  table-sheet, live-session-bar, previous-summary, scene-board, live-soundbox, sound-library, battlemap.
- Borders: to-create-list `__item` (dashed), entity-editor `__notice` and `__unsaved` (left), version-history
  `__item.is-unseen`, player-stage `.pv-window` (the floating window), table-sheet `__notes` and `__status`,
  suggestion-pill first row, previous-summary open threads (left), `.chip--todo` and `.chip--lieu` of scene-board,
  media-library and previous-summary.
- Text and numbers: session-list `__num` (the session's number), live-session-bar `__broadcast b` (*la couverture de la
  campagne*), battlemap `__round b` and `.bm-init`, version-history item titles, table-sheet block titles
  (`h3`) and `summary`, entity-editor feature names (`b`), to-create-list proposal `dt` and statblock vitals,
  note-import duplicate marker, player-stage window bar.
- Fills / outlines: player-stage `.pv-window__resize`; focus outline of `select` in ai-settings, entity-editor,
  grid-settings, media-library, scene-board; battlemap drag-over outline.
- Art: `.art--gold` and `.art--dusk` gradient stops in campaign-elements, campaign-list, campaign-overview, media-library,
  player-stage, table-sheet.

**Where it shows on the nine pages at 1280 px** (computed colour equal to the accent): *campagnes* 3 text elements + 1
border; *campagne* 6 text, 3 border (the two to-create cards, the *accent* badge) + 1 fill; *fiche-entite* 13 text (all the `h2`,
legends, labels, slider outputs, token number) + 1 fill; *preparation-seance* 15 text, 11 border, 2 fill; *medias* 14
text (labels, slider outputs, filedrop tags, `h2`), 1 border; *soundbox* 6 text (first column cells,
`h2`, slider output), 2 borders (table header cells); *parametres-ia* 3 text (the `h2` and the legend); *dm-view* 5
text, 2 border; *player-view* none.

The accent is used for the headings of every panel (`h2`), for links and for hover, for decoration of the covers,
for map features, and for four state-like uses: the *confirmed* toast, the *to create* chips and cards (`chip--todo`, dashed), an unread
version (`is-unseen`) and the first suggestion.

### 10.4 Type sizes and weights

Tokens: `--font-size-small .875rem`, `-base 1rem`, `-large 1.25rem`, `-heading 2rem`; `--font-weight-heading 700`;
`--font-family-heading` Impact → Arial Narrow Bold → Helvetica Neue → Arial; `--font-family` system-ui. Rendered
here with Impact installed (macOS); the fallback was not rendered.

- **Elements:** `h1` 32 px Impact 700, uppercase, `letter-spacing .03em`, `line-height 1.15`, `--color-primary`; `h2` 20 px,
  accent; `h3` 16 px, text. Running text 16 px, `line-height 1.5`; small 14 px.
- **Sizes measured at 1280 px, over the nine pages' default states** (number of text elements): 11.2 px (9: token numbers,
  HP labels), 11.67 px (4: rail group counts), 13.33 px (78: `small` and unstyled `button`), 14 px (481), 16 px (372),
  20 px (70), 32 px (41). Four of the seven sizes are tokens (14, 16, 20, 32); 11.2, 11.67 and 13.33 px are not.
  *player-view* adds 64 px (the title) and 128 px (the cover's initial), and on the map 11.2, 14 and 20 px.
- **Weights:** 400, 600 and 700. Only 700 is a token; 600 is the weight of the kit buttons and form labels.
- **Families:** Impact for the headings, badges, table headers, tabs and legends (95 elements at 14 px Impact 700);
  system-ui for the rest. Impact appears at 14, 16, 20 and 32 px (and 64 / 128 px on the player-view).
- **Per screen**, sizes used at 1280 px: *campagnes* 14 / 16 / 20 / 32; *campagne* 13.33 / 14 / 16 / 20 / 32;
  *fiche-entite* 11.2 / 11.67 / 13.33 / 14 / 16 / 20 / 32; *preparation-seance* 11.2 / 13.33 / 14 / 16 / 20 / 32;
  *medias* 13.33 / 14 / 16 / 20 / 32; *soundbox* 13.33 / 14 / 16 / 20 / 32; *parametres-ia* 14 / 16 / 20 / 32;
  *dm-view* 13.33 / 14 / 16 / 20 / 32; *player-view* 20 / 64 / 128.

### 10.5 Spacing

Tokens `--space-1 … 5` = 4, 8, 16, 24, 40 px. Over every visible element at 1280 px (margin, padding, gap), all values are on
the tokens except: 1 px (6 on *preparation-seance*, 6 on *medias*), 2 px (8 on *preparation-seance*, 48 on *soundbox*), 6 px (6
on *preparation-seance*, 6 on *medias*) and a few computed values from centring (`auto` margins). *campagnes* and
*campagne* have none off-token. Page padding is `--space-4 --space-5` (24 / 40 px) at wide widths and `--space-3` (16 px)
at ≤ 960 px; the card, panel and item paddings use `--space-3` / `--space-4`.

### 10.6 States and destructive actions, screen by screen

Source: the code of the 22 Micro-UIs under `apps/` (the states the fixtures do not produce were not rendered). The common
drawing: a message is a plain paragraph; `.muted` for empty and loading, `.is-error` for failure. An error is
red text only in *session-list*, *campaign-list*, *campaign-elements*, *live-soundbox*, *suggestion-pill* and
*live-session-bar*; a box with a red outline and red text in *ai-settings*, *note-import*, *oral-brief*, *to-create-list*,
*entity-editor*, *version-history*, *sound-library*, *media-library*, *grid-settings*, *elements-panel*, *scene-board*,
*battlemap* and *player-stage*; *table-sheet*'s status line (what a send did) is a box with an accent outline and normal text.
Success, where there is one, is a box with a green outline (`.is-ok`). There is no shared component and no icon; each Micro-UI has its own
`role="status"` / `aria-live` region (1–5 each). A command is followed by polling until `applied`, `rejected`,
`cancelled` or `expired`; a non-final status other than these is shown as *… n'a pas abouti (état : <status>)*.

#### Empty

- **campagnes:** *Aucune campagne pour l'instant : créez-en une ou importez une archive.* (muted paragraph).
- **campagne:** *Aucune séance pour l'instant.* (muted, `session-list`); *Aucune campagne ouverte.* (overview);
  *Aucun élément trouvé dans cette note · note brute conservée.* (note-import); *Aucune campagne choisie.* (to-create-list).
  **No text** for an empty tab of `CampaignElements` or for an empty list of entities to create.
- **fiche-entite:** *Aucune fiche choisie.*, *Aucune image* (select option), *Aucune version.* (version history). **No
  text** for an empty rail group (the group is dropped).
- **preparation-seance:** *Aucune séance précédente n'est encore close.*, *Cette séance n'a pas de résumé.*, *Le journal de cette
  séance est vide.*, *Aucun brief analysé pour cette séance.*, *Aucune battlemap pour cette scène*, *La campagne n'a pas de
  battlemap à associer.*, *Aucune séance n'est indiquée : rien à afficher.*
- **medias:** *Aucune campagne n'est ouverte.*, *Aucun lieu* (select option). **No text** for an empty library (no media) and
  none in grid-settings.
- **soundbox:** *Aucun son dans la bibliothèque : déposez le premier.*, *Aucun son ne correspond à la recherche.* (library),
  *Aucun son ne correspond.* (live).
- **parametres-ia:** no empty state; the key field's placeholder is *Aucune clé*.
- **dm-view:** *Aucune séance en cours* (session bar), *Aucune scène dans la séance.* (elements panel), *Aucun élément
  ouvert.* (sheet), *Aucune suggestion pour l'instant.* (pill), *Aucun son ne correspond.* (live soundbox). No text for an
  empty combat or battlemap.
- **player-view:** the empty state is the campaign cover; there is no text for it.

#### Loading

Only five Micro-UIs write a loading text: *Chargement des séances…* (session-list, a muted list line), *Chargement…*
(campaign-elements), *Chargement de la battlemap…* (grid-settings), *Chargement des médias…* (media-library), *Chargement
des sons…* (sound-library). The others render their regions empty until the read returns. Work in progress is a
*… en cours…* line: *Test en cours…* (ai-settings), *Extraction en cours…*, *Création en cours…* (note-import),
*Analyse du brief en cours…* (oral-brief), *Proposition en cours…* (to-create-list, entity-editor), *Enregistrement
en cours…*, *Resynchronisation en cours…*, *Import en cours…* (entity-editor), *Restauration en cours…*
(version-history), *Export en cours…* (campaign-overview), *Clôture en cours…* (live-session-bar), *Ajout en cours…*,
*Suppression…* (sound-library).
- **campagnes:** no loading text (CampaignList). **campagne:** session-list and campaign-elements have one; overview, to-create,
  note-import show *Export / Création / Extraction en cours…* for writes only. **fiche-entite:** none for the read; writes
  as above. **preparation-seance:** none for the reads; *Analyse du brief en cours…*. **medias:** both Micro-UIs have one.
  **soundbox:** sound-library has one; live-soundbox none. **parametres-ia:** none for the read; *Test en cours…*.
  **dm-view:** none. **player-view:** none.

#### Error (a read that fails, or a command that fails)

Each Micro-UI has its own wording: *Impossible de lire …* for a read (21 of the 22; elements-panel says *Une partie du panneau n'a pas pu être lue.*), *<Action> a échoué.* for a command that did
not reach the server, *Le test de connexion a échoué.*, *L'enregistrement a échoué.*, *La création a échoué.*. There is no retry
control and no icon in any of them.
- **campagnes:** *Impossible de lire les campagnes.*, *La création / L'export / L'import a échoué.* (red text in the dialog or
  above the list). **campagne:** *Impossible de lire les séances.*, *… la campagne.*, *… les notes importées.*, *… les éléments à
  créer.*, *… <what>* (elements). **fiche-entite:** *Impossible de lire la fiche.*, *… l'historique des versions.*,
  *L'enregistrement a échoué.*. **preparation-seance:** *… la séance précédente / le journal / le dernier brief / la séance /
  la battlemap*; *Impossible de réordonner les scènes / d'ajouter une scène / d'enregistrer ces notes / d'ajouter ou retirer ce PJ /
  de créer l'instance.* **medias:** *Impossible de lire la bibliothèque de médias.*, *Impossible de lire la battlemap.*,
  *Le téléversement a échoué.*, *L'enregistrement a échoué.*. **soundbox:** *Impossible de lire les sons (de la campagne).*,
  *Le son n'a pas pu être lancé*, *Impossible de tout arrêter*, *La suppression a échoué.*. **parametres-ia:** *Impossible de lire
  les paramètres IA.*, *Connexion impossible.*, *Le test de connexion a échoué.*, *L'enregistrement a échoué.*. **dm-view:**
  *Impossible de lire la séance / les scènes / ce que voient les joueurs / la fiche / les suggestions*, *La commande a échoué.*,
  *La clôture a échoué.*, *L'envoi n'a pas pu être fait.*. **player-view:** *Impossible de lire la diffusion.* (14 px red box, bottom left of the
  projected screen).

#### Queued

The server's `queued` state is shown by name in three places: *Enregistrement en attente (état : queued) : il s'appliquera dès que la
file le permettra.* (grid-settings), *La commande est en file d'attente (état : …) : elle s'appliquera dès que possible.* (media-library),
*L'analyse est toujours en attente (état : …) : elle s'appliquera à son tour.* (oral-brief). The scene-board and the pill
treat a queued or parked command as the DM's own and say so in code comments; elsewhere a command in a non-final state shows *… n'a pas
abouti (état : <status>)*, the same sentence as a failure. The mock-up's own toast (`mock.js`, bottom-left, *en file* in muted grey)
exists in the mock pages' shared code and none of the 22 Micro-UIs calls it.
- **No treatment of a queued command by that name:** campagnes, campagne, fiche-entite (but *Enregistrement en cours…*), soundbox,
  parametres-ia, dm-view (live-session-bar, table-sheet and elements-panel show *… n'a pas abouti*), player-view.

#### Confirmed

`confirmed` (accepted by the server, not yet applied) has no screen in `apps/`: the shell polls through `queued` and `confirmed`
(`apps/shell/src/client/shell.ts`, `ADVANCING`) and draws nothing; the only drawing of *confirmée* is the mock-up toast (accent
border and accent state text, `style.css`), which no Micro-UI uses. The statuses that wait for a person (`awaiting_confirmation`,
`awaiting_review`) appear in the tests of `apps/` only, in no sentence of a Micro-UI: they fall under *… n'a pas abouti (état : <status>)*,
and no Micro-UI offers the author a control to confirm an overwrite. This holds for all nine screens.

#### Applied

A plain success sentence exists in 12 of the 22 Micro-UIs, in a green-outlined box (`.is-ok`, `--color-success`) or a line:
- **campagnes:** *Archive « <file> » enregistrée.* (campaign-list, after an export); *Export effectué, mais …* only for a partly failed export.
- **campagne:** note-import *<N> élément(s) créé(s) · note brute conservée.*
  and *Aucun élément trouvé dans cette note · note brute conservée.* (`.is-ok`, `render.ts:88`). campaign-overview's clean export writes no sentence (the line *Dernier export : <date>* is repainted);
  session-list, campaign-elements and to-create-list write none on success (to-create-list clears its line).
- **fiche-entite:** entity-editor *Fiche enregistrée.*, *Aucun champ de la fiche n'a changé.*, *Personnage importé : retrouvez-le dans
  la liste des éléments.*; version-history *La v<N> est restaurée : la v<M> est en tête de l'historique.* (`.is-ok`).
- **preparation-seance:** oral-brief *Brief analysé · <N> scène(s), <M> à créer.* (`.is-ok`, `render.ts:95`); scene-board and
  previous-summary write none.
- **medias:** grid-settings *Réglage enregistré.*; media-library *« <title> » est ajouté à la campagne.*, *… est rattaché au lieu …*,
  *… est dans la séance : ses médias y sont associés.*
- **soundbox:** sound-library *Son enregistré.*, *« <name> » est dans la soundbox.*, *« <name> » est supprimé.*; live-soundbox clears its line.
- **parametres-ia:** ai-settings *Paramètres enregistrés.* and *Connexion réussie.* / *Dernier test réussi …* (the last-test box).
- **dm-view:** battlemap *Carte envoyée aux joueurs.* (`.bm-status.is-ok`); table-sheet *« <title> » est envoyée aux joueurs, en mode
  scène|fenêtre.*; elements-panel *Battlemap envoyée aux joueurs.* / *« <name> » envoyée aux joueurs en scène|en fenêtre.*
  live-session-bar and suggestion-pill write none (a closed session, a mic or a sound that starts shows by its effect).
- **player-view:** none.

Where there is no sentence, the applied command shows by its effect (a session, a scene, a PJ, a token moved or removed, a sound
playing, the picture on the player-view) and by the end of the *… en cours…* line.

#### Refused

A refused command is rendered as *… refusé(e) : <reasons from the DataGuard>.* in the Micro-UI's own region: *Création refusée*
(campaigns, sessions, notes, entities to create), *Export refusé*, *Import refusé*, *Enregistrement refusé* (ai-settings, grid-settings,
sound-library), *Restauration refusée* (version-history), *Envoi refusé* (table-sheet), *Clôture refusée* / *Refusé* (live-session-bar),
*Action refusée* (scene-board), *Analyse refusée* (oral-brief), *<Action> refusé : …* (battlemap), *Fenêtre non modifiée* (player-stage).
elements-panel (*Pas envoyé : <reasons>.* on `rejected`, `index.ts:191`; *L'envoi n'a pas abouti (état : …)* for any other state),
suggestion-pill (*Le micro n'a pas changé : <reasons or state>.*, *« <name> » n'a pas pu être joué : <reasons or state>.*) and
live-soundbox (*Le son n'a pas pu être lancé : <reasons or state>.*, *Le volume … n'a pas pu être changé*) give the DataGuard's reasons
in a sentence that does not use the word *refusé*. **No refusal sentence** in campaign-elements and previous-summary, which send no
command.

#### Destructive actions

| screen | action | where | confirmation | look |
|---|---|---|---|---|
| campagnes | none (export, create, import) | — | — | — |
| campagne | none | — | — | — |
| fiche-entite | remove a feature (×); *Retirer l'image* (select option); undo the AI's change through the versions dialog (*Annuler les changements cochés*); leave a modified sheet | `EntityEditor`, `VersionHistory` | the versions dialog and a leave-the-sheet prompt; **none** for the × and the image option | × is an `icon-btn`; the dialog button is primary |
| preparation-seance | remove a PJ (×) from the scene; *Réinitialiser la carte* | `SceneBoard`, `Battlemap` | reset has a dialog; **none** for the PJ × | × `icon-btn`; ghost button |
| medias | *Annuler les changements* (resets the form, not data) | `GridSettings` | none, nothing is lost | bare text |
| soundbox | *Supprimer* a sound; *Tout arrêter* | `SoundLibrary` (edit panel), `LiveSoundbox` | **none** for *Supprimer* (the line *Suppression…* then *Suppression refusée* / *La suppression a échoué.*); none for *Tout arrêter* | both filled red (danger) |
| parametres-ia | none | — | — | — |
| dm-view | *Clore la séance*; *Retirer de la carte* / *Retirer le corps*; *Retirer du combat*; *Réinitialiser la carte* | `LiveSessionBar`, `Battlemap` | *Clore la séance* and *Réinitialiser la carte* have a kit Dialog (the close dialog says the microphone is paused and the sounds stopped); **none** for *Retirer de la carte* and *Retirer du combat* | red filled button (*Clore la séance*, *Retirer de la carte*); ghost (reset, combat drop) |
| player-view | none | — | — | — |

No undo for a removal was found in the source of any screen other than the sheet's versions history.

### 10.7 Not measured

- Widths between 961 and 1279 px and above 1280 px; landscape tablet; browsers other than Chromium; a system without
  Impact (the heading font's fallback).
- Keyboard and screen-reader use beyond the counts of live regions, and focus order.
- The dialogs (create / import a campaign, close a session, versions, battlemap reset) and the floating-window and
  illustration states of the player view, read from code only.
- Performance and animation (`prefers-reduced-motion` has one rule in the kit).

## Reproduction

`python3 -m http.server -d mock 8741`, then Playwright's Chromium at 360 × 800, 768 × 1024 and 1280 × 800 on
`http://localhost:8741/pages/<screen>.html`; a script collected, per page and width, the scroll width, the elements right of the
viewport, the computed font sizes and weights, margins, paddings and gaps, the interactive elements' boxes, the elements whose
computed colour is `rgb(245, 184, 61)`, and the contrast of every text node; the ratios of section 10.2 come from
`mock/design/tokens.css`. The player-view's sent-map state was rendered by answering `live.getDiffusion` with a battlemap
(`base: { kind: 'battlemap', id: 'bmi-gue-s4' }`). The scripts are not committed: this change adds one file, and the numbers are
measures of the tree named at the top, not of a later one.
