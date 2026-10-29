# Design direction

One written rule for every later UI task, in place of each screen inventing its own. It is argued from
[`UI-AUDIT.md`](UI-AUDIT.md) (the nine screens at 360, 768 and 1280 px, measured on the tree it names) and it gives the
direction the audit refused to give.

- **Status.** Proposal. Nothing here changes a token, a kit component, a mock page or a file under `apps/`. Every value is
  written as the token it would become (name and value); the design system has one writer (`design-system`,
  `docs/ARCHITECTURE.md`, contract O), and a later task carries these values there.
- **Open values.** Three choices are the owner's taste, not this document's: the accent colour, the display face and the
  DM view's density. Each section states the rule and leaves the value open ([§9](#9-the-owners-choices)).
- **Language.** The interface is in French and the examples quote it as the audit does. The rules are written in English.
- **Words.** *Must* is a rule a gate or a review can refuse a change for; *should* is the default a change departs from
  only with a reason written in its pull request.
- **Contrast numbers** are WCAG 2.x ratios computed from the hex values of `mock/design/tokens.css` and from the values
  proposed below; AA is 4.5:1 for body text, 3:1 for large text (≥ 24 px, or ≥ 18.66 px bold) and for UI components.

## Correction to the brief

The brief says "the muted grey of today fails". The audit measured it (`UI-AUDIT.md` §10.2): `--color-text-muted #b9a6c9`
is **8.62:1** on `--color-surface` and **7.83:1** on `--color-surface-2`: it passes AA at every size. What fails, and
what this direction replaces, is the *muted grey as it renders*:

- the input placeholder (`--color-text-muted` at opacity .7): 4.70 and **4.48** on the inputs' `--color-surface-2`;
- the disabled button and input (opacity .55 and .6): **3.3–3.8**;
- `--color-secondary #8b3fd9` used as text (statblock abbreviations, privacy bullets): **3.51** and **3.19**;
- `--color-border #3d2b57` as the outline of inputs, chips and cards: **1.55** and **1.41**, against 3:1.

So the secondary-text colour is kept (it is not the failure) and the failing renderings get solid tokens of their own
([§5](#5-contrast)).

---

## 1. Principles

1. **Size and weight carry the hierarchy.** Not capitals, not colour, not the typeface.
2. **One accent, one meaning:** "this is the primary action of the view".
3. **One rhythm:** every gap and padding is `--space-1` to `--space-5`.
4. **Text is readable, borders are visible:** AA on both dark surfaces, 3:1 on every control's outline.
5. **A state is never told by colour alone:** each has a shape and a sentence.
6. **The same thing looks the same on every screen:** a pending change, an error, a destructive action, a focused control.

---

## 2. Type

### 2.1 The scale

Hierarchy comes from the **size** and the **weight** of a step, in that order. Two steps next to each other in a screen
differ by size or by weight; they never differ only by capitals, letter-spacing or colour. Headings are in
`--color-text`.

| step | size token (value) | weight | line height | use |
|---|---|---|---|---|
| `display` | `--font-size-display` = `clamp(2.5rem, 1.75rem + 3vw, 4.5rem)` (new) | 700 | `--line-height-tight` | The player view's cover title, and nothing else. It follows the viewport (today a fixed 64 px) |
| `stage` | `--font-size-stage` = `clamp(1.25rem, 1rem + 1.2vw, 2rem)` (new) | 400 | `--line-height-snug` | Everything else a player reads from a distance: the cover's subtitle, token names and status lines. Nothing on the player view is smaller than 1.25 rem |
| `title` | `--font-size-heading` = `2rem` (exists) | 700 | `--line-height-tight` | The page's `h1`, one per page |
| `section` | `--font-size-large` = `1.25rem` (exists) | 700 | `--line-height-snug` | `h2`: the title of a panel, of a dialog |
| `group` | `--font-size-base` = `1rem` (exists) | 700 | `--line-height-snug` | `h3`, a card's title, a fieldset's legend |
| `reading` | `--font-size-large` = `1.25rem` | 400 | `--line-height-body` | What the DM reads at the table: the opened sheet's body. The DM view's brief may move it one step ([§9](#9-the-owners-choices)); it never adds a size |
| `body` | `--font-size-base` = `1rem` | 400 | `--line-height-body` | Running text |
| `action` | `--font-size-base` = `1rem` | 600 | `--line-height-tight` | The text of a button, a tab, a menu link |
| `label` | `--font-size-small` = `0.875rem` (exists) | 600 | `--line-height-snug` | A form label, a badge, a chip, a table header, a statblock abbreviation |
| `small` | `--font-size-small` = `0.875rem` | 400 | `--line-height-snug` | A hint, a meta line, a counter, a date, the footer |

New tokens, besides the two sizes above:

| token | value | replaces |
|---|---|---|
| `--font-weight-regular` | `400` | the literal 400 |
| `--font-weight-semibold` | `600` | the literal 600 of the kit's buttons and labels (today not a token) |
| `--line-height-tight` | `1.15` | `h1`'s 1.15 |
| `--line-height-snug` | `1.35` | the browser's default `normal` on labels and small text |
| `--line-height-body` | `1.5` | the running text's 1.5 |

`--font-weight-heading` (700) and the four existing sizes stay. Reading the scale back: `title` and `section` differ by size
(32 / 20 px); `section` and `group` by size (20 / 16 px); `group` and `body` by weight (700 / 400) at one size; `section` and
`reading` by weight (700 / 400) at one size; `body`, `action` and `label` by size and weight (16/400, 16/600, 14/600);
`label` and `small` by weight (600 / 400). No pair relies on capitals or colour.

### 2.2 What this removes (audit §10.4)

- **Three off-token sizes go.** The audit counted 11.2 px (9 elements), 11.67 px (4) and 13.33 px (78) beside the four
  token sizes (14, 16, 20, 32 px). The off-token ones come from the browser's default sheet (`small`, an unstyled `button`) and from kit
  rules; **no element takes a size from the browser**: buttons and fields use `font: inherit`, and the `small` *element*
  is styled by the `small` step. The smallest text of the product is 14 px (`--font-size-small`), on the DM's screens and
  the player view alike (1.25 rem there).
- **The weights are three:** 400, 600, 700, each a token. Weight 600 is now named.
- **Impact appears on two steps only.** Today it also sets 95 elements at 14 px (badges, table headers, tabs, legends).
  The display face is used for `display` and `title`; every other step is `--font-family`. Which face that is stays the
  owner's choice ([§9](#9-the-owners-choices)); the rule holds for any face.
- **No `text-transform`, no `letter-spacing` for style.** The `h1`'s uppercase and its `.03em` go; so do the uppercase
  badges, tabs and table headers (*JOUÉE*, *PERSONNAGES (9)*, *EXISTE*, *À CRÉER*).

### 2.3 Sentence case

Every title, label, button, tab, badge, menu entry, table header and message is **sentence case**: a capital on the first
word and on proper nouns, nowhere else, and written that way in the source, never produced by CSS.

| today (rendered) | written |
|---|---|
| `JOUÉE`, `EN PRÉPARATION` | Jouée, En préparation |
| `PERSONNAGES (9)` | Personnages (9) |
| `EXISTE` · `À CRÉER` · `MIS À JOUR` | Existe · À créer · Mis à jour |
| the `h1` *Vos campagnes*, rendered uppercase | Vos campagnes |

Names the DM wrote (a campaign, a place, a sound) are shown as written. Abbreviations that are written in capitals
(*FOR*, *DEX*, *PJ*, *PNJ*) stay as they are.

---

## 3. Accent

### 3.1 The rule

There is **one accent**. It is kept for **the primary action**: the filled button that does the thing the view exists for
(*Nouvelle campagne*, *Lancer la séance en direct*, *Enregistrer la fiche*). **At most one element of a view carries it.**
A *view* is a screen, or an open dialog while it is open (the dialog's own confirm is then the one, and the screen's is
inert behind it).

- **Role token.** The accent role is carried by one token. Its working value is today's `--color-primary` (`#e8702a`), the
  fill every primary button already uses: 6.25:1 on `--color-surface`, 5.67:1 on `--color-surface-2`, and the text on it,
  `--color-primary-contrast #14081f`, 6.26:1. The value is the owner's to confirm or change ([§9](#9-the-owners-choices)).
  Any value must be ≥ 3:1 as a fill against both surfaces and carry a text colour at ≥ 4.5:1 on it.
- **`--color-accent` (amber `#f5b83d`) leaves the interface.** It is renamed `--color-art-gold` in the token task and is
  read only by artwork and maps (cover gradients, `.hm-hill`, `.hm-road`, `.bm-ford`). Until then no new code reads it.
  Two tokens that both mean "the colour that stands out" is what makes the product read as orange everywhere.
- **Everything else on a screen is not the accent.** A view's other actions are secondary (outlined) or ghost.
- **Lists of items.** An action repeated on each item (*Ouvrir* on three campaign cards, *Lancer en direct* on a session)
  is secondary. The view's primary is the one action above the list (*Nouvelle campagne*). Which action a screen's primary
  is, is written in its mock page; Micro-UIs draw their actions as secondary unless the screen says otherwise (how it says
  so is left to the token and kit task).

### 3.2 What the accent must no longer be used for

Each point cites the audit; "accent" here is either token (the orange `--color-primary` or the amber `--color-accent`).

| used today for | audit | becomes |
|---|---|---|
| Several filled orange elements at once: 6 on *campagnes*, 8 on *campagne*, 4 on *preparation-seance*, 3 on *fiche-entite* and *medias* | §10.1 | one filled element per view; the others outlined |
| A repeated per-item button (*Ouvrir* ×3; session 4's *Lancer en direct* / *Préparer* beside the same two actions in the hero) | §1, §2 | secondary; the duplicate in the hero is a screen decision, not a style one |
| The active tab (*Personnages (9)*, *Fiche / Battlemap / Soundbox*) and the current menu link, filled orange | §2, §8, §10.1 | selected = `--color-text`, weight 600 and a 2 px underline in `--color-text` (a shape), `aria-selected` / `aria-current` |
| A state drawn as a button (*Mis à jour*, filled orange) | §3 | a state treatment of [§6](#6-states), never a fill |
| The table header of *soundbox* (filled orange), the first column in amber, durations in green | §6 | header in `--color-text` on `--color-surface-2` with a bottom border; one text colour in the body |
| The filled `.kit-badge .primary` (*Campagne* badge) | §10.1 | an outlined badge in `--color-text-muted` text on `--color-border-strong` |
| `h2` text in amber, in 17 Micro-UIs; `.kit-card h3`, `kit-dialog__title`, legends | §10.3 | `--color-text` (the hierarchy is the size and weight of [§2](#2-type)) |
| Links and every hover colour (`a`, `.chip`, `.icon-btn`, ghost hover) in amber | §10.3 | links in `--color-text`, underlined; hover is the state layer of [§8](#8-focus-and-hover) |
| A focus ring in amber (menu, tabs, breadcrumb) | §10.3 | the one focus colour of [§8](#8-focus-and-hover) |
| State-like uses: the *confirmed* toast, the *to create* chips and cards (dashed), the unread version (`is-unseen`), the first suggestion's outline | §10.3 | the state treatments of [§6](#6-states); an unread version is a `small` text "Nouvelle" with a dot, not a border colour |
| Numbers and values: session `__num` disc, slider values, `__round b`, `.bm-init`, token numbers | §10.3 | `--color-text`, weight 600 |
| Border tells the kind of a chip (accent, secondary, primary, dashed accent in *scene-board*) | §4 | one chip look; the kind is told by its word (*lieu*, *PNJ*, *à créer*) and an icon, not a border colour |
| The primary border of the selected battlemap card, and the same *Régler la grille* button filled or not by selection | §5 | selected = 2 px `--color-text` border and a "Sélectionnée" label; the button keeps one look |
| Decoration: cover gradients, hills, roads | §10.3 | `--color-art-gold`, outside the interface |

### 3.3 Other colours that are not the accent

- `--color-secondary` (violet) is never text ([§5](#5-contrast)) and never a hover bar. It stays for map water and rivers.
- Green (`--color-success`) means *applied* and nothing else: not a duration, not a volume, not a hint (audit §6, §5).
- Red (`--color-danger`) means *refused*, *error* or *destructive* and nothing else.
- Cyan is the focus ring and nothing else ([§8](#8-focus-and-hover)). The checked radio's hint is `--color-text`.

---

## 4. Spacing

The spacing tokens are `--space-1` 4 px, `--space-2` 8 px, `--space-3` 16 px, `--space-4` 24 px, `--space-5` 40 px (they stay as
they are). **A margin, padding or gap is one of these five or zero**; a literal in a rule is a violation. The audit found
the product already on them except for 1 px (borders), 2 px (48 on *soundbox*, 8 on *preparation-seance*) and 6 px
(6 on *preparation-seance*, 6 on *medias*): a border is `--border-width`, a focus ring is `--focus-ring-*` ([§8](#8-focus-and-hover)),
a 2 px gap becomes `--space-1`, a 6 px gap becomes `--space-2`.

| use | token | notes |
|---|---|---|
| **Inside a control** (padding of a button, a field, a select) | `--space-2` block, `--space-3` inline | the block padding, a 1.15 line and the border give a control about 36 px high; a control that is touched is not under 24 × 24 px (WCAG 2.2, 2.5.8) |
| Inside a *compact* control (chip, badge, icon button) | `--space-1` block, `--space-2` inline | |
| Between the icon and the text of a control | `--space-2` | |
| A label and its field | `--space-1` | |
| **Between controls** in a row (buttons, chips, a field and its button) | `--space-2` | also the wrap gap |
| Between two fields stacked in a form | `--space-3` | |
| Inside a card, a list item, a table cell | `--space-3` | |
| Inside a panel, a dialog | `--space-4` | |
| A heading and the first block under it | `--space-2` | |
| **Between groups** (a fieldset and the next, the items of a card grid, the items of a list) | `--space-4` | |
| **Between sections** (one panel and the next, the blocks of a page) | `--space-5` | |
| **Page margins** | `--space-3` at ≤ 960 px; `--space-4` above and `--space-5` inline from 961 px | as today (`--space-4 --space-5` wide, `--space-3` narrow) |

Reading it back: the nearer two things are in meaning, the smaller the token between them (`--space-1` label to field,
`--space-2` control to control, `--space-3` field to field, `--space-4` group to group, `--space-5` section to section).
A gap that separates two groups is never smaller than the one inside a group. `--space-5` is not used inside a control.

---

## 5. Contrast

Two dark surfaces exist: `--color-surface #120a1c` and `--color-surface-2 #1f1331` (they differ by 1.10:1; a container is
told from its surround by its border and its spacing, not by its fill). Every text and control colour is measured against
both. Ratios are WCAG 2.x, to two decimals.

### 5.1 Text

| role | token | value | on `--color-surface` | on `--color-surface-2` | needed | |
|---|---|---|---|---|---|---|
| **Body text** | `--color-text` | `#f5ecf7` (kept) | 16.78 | 15.24 | 4.5 | pass |
| **Secondary text** (hints, meta, placeholders' neighbours) | `--color-text-muted` | `#b9a6c9` (kept, solid) | 8.62 | 7.83 | 4.5 | pass |
| **Disabled text** and the **placeholder** | `--color-text-disabled` (new) | `#9a87ad` | 5.93 | 5.39 | 4.5 | pass |
| **Pending text** (violet, the queued and confirmed states; replaces `--color-secondary` as text) | `--color-secondary-text` (new) | `#b98af0` | 7.33 | 6.65 | 4.5 | pass |
| Success text and glyph | `--color-success` | `#3ecf8e` (kept) | 9.69 | 8.80 | 4.5 | pass |
| Danger text and glyph | `--color-danger` | `#ff5a5f` (kept) | 6.34 | 5.75 | 4.5 | pass |
| Violet as text (not allowed) | `--color-secondary` | `#8b3fd9` | 3.51 | 3.19 | 4.5 | **fail** (not text) |

Rules:

- **No opacity on text.** Dimming is a solid token. The placeholder's `opacity: .7` (4.48 on the inputs) and the disabled
  buttons' and inputs' `.55` / `.6` (3.3–3.8) are replaced by `--color-text-disabled`, which clears 4.5:1 on both surfaces
  and is still visibly under `--color-text-muted` (5.39 against 7.83 on `--color-surface-2`).
- **Disabled is also a shape**: no fill, a dashed `--color-border-strong` outline, `cursor: not-allowed`, no hover. A
  disabled control is not told by its dimness alone, and nothing below 4.5:1 is left on a screen.
- **`--color-secondary` is a fill or a stroke, never text.** The statblock abbreviations (*FOR … CHA*, 3.19:1) and the
  privacy panel's bullets use `--color-text-muted`.
- **Text on a fill.** Use the pair that measures: `--color-primary-contrast #14081f` on primary 6.26, on success 9.70, on
  danger 6.35 and on `--color-secondary-text` 7.34; `--color-text` on `--color-secondary` 4.78. Never `--color-primary-contrast`
  on `--color-secondary` (3.51) and never `--color-text` on primary, accent, success or danger (1.54–2.69).
- **Token discs** (`kit-token__initials`, white on the bright discs, 1.54–2.69:1 in the audit) use
  `--color-primary-contrast`: the `.is-dark` variant `style.css` already has becomes the only one.
- **Large text** (≥ 24 px, or ≥ 18.66 px bold) may fall to 3:1 under WCAG; this direction does not use the allowance: the
  whole scale, `display` included, is held to 4.5:1.

### 5.2 Borders and graphical objects

| role | token | value | on `--color-surface` | on `--color-surface-2` | needed | |
|---|---|---|---|---|---|---|
| **The outline of a control** (button, input, select, checkbox, radio, toggle, chip) | `--color-border-strong` (new) | `#7d6a96` | 4.03 | 3.66 | 3 | pass |
| A divider or a container's outline (informational, not needed to find a control) | `--color-border` | `#3d2b57` (kept) | 1.55 | 1.41 | none | not for controls |
| The focus ring | `--color-focus` (new) | `#27d7f0` | 11.11 | 10.09 | 3 | pass |

`--color-border` (1.55 / 1.41) fails as the outline of a control, which is how the audit found it used (inputs, cards,
chips). The rule: **a control's outline is `--color-border-strong`**; `--color-border` is for dividers between rows and for
a panel's edge, where the spacing already tells the container.

### 5.3 States on the hover layer

A hovered row or control sits on the surface plus the `--state-hover` layer ([§8](#8-focus-and-hover)). Text on it,
measured on the composite colour (`#241c2e` over `--color-surface`, `#302441` over `--color-surface-2`):

| text | on hovered `--color-surface` | on hovered `--color-surface-2` |
|---|---|---|
| `--color-text` | 14.23 | 12.53 |
| `--color-text-muted` | 7.31 | 6.44 |
| `--color-secondary-text` | 6.21 | 5.47 |
| `--color-success` | 8.22 | 7.23 |
| `--color-danger` | 5.37 | 4.73 |
| `--color-border-strong` (outline, needs 3) | 3.42 | 3.01 |

All hold. (The disabled text is never hovered.)

---

## 6. States

Seven states, each with **one** treatment on every screen. Three principles hold for all of them:

1. **A glyph, a sentence and a place; colour is the third cue, never the only one.** Every state shows a shape (a glyph,
   `aria-hidden`) *and* a sentence that says it. Where two states share a colour family (neutral: empty and loading;
   pending: queued and confirmed; danger: refused and error), the glyph and the wording differ, and so does the place.
2. **One component, the status line**, replaces the per-Micro-UI paragraphs of the audit (`.muted`, `.is-error`, `.is-ok`;
   no shared component and no icon today). Anatomy: a 4 px left border in the state's colour, the glyph in that colour
   (`--space-2` before the text), then the sentence in `--color-text` at `body` (the text is never coloured red or violet,
   so its contrast stays 15:1). Padding `--space-2` block, `--space-3` inline.
3. **The server's statuses are not shown to the DM.** `queued`/parked, `confirmed`, `applied`, `rejected` and `expired` map
   to the states below; the sentence *… n'a pas abouti (état : <status>)* and the English words (*queued*) disappear.

### 6.1 The treatments

| state | colour token | glyph (shape) | wording pattern | where it appears |
|---|---|---|---|---|
| **Empty** | `--color-text-muted` text; a dashed `--color-border-strong` outline | `○` (an empty circle) | *Aucun(e) <objet> pour l'instant.* + the next step when there is one, as an action: *Aucune campagne pour l'instant. Créez-en une ou importez une archive.* | In the region the content would fill, in its place. **Every list, tab, rail group and library says it is empty** (audit §10.6: today none for an empty tab, the library, a rail group, a combat). A search with no hit says *Aucun <objet> ne correspond à « <terme> ».* |
| **Loading** | `--color-text-muted` | `◌` rotating (a still `…` under `prefers-reduced-motion`) | A read: *Chargement <de/des/du> <objet>…* A write: *<Nom de l'action> en cours…* (as the audit's *Enregistrement en cours…*) | In the region that is waiting, replacing its content (`aria-busy="true"`); a write's line sits at the control that sent it, and that control is disabled. **Every read has one** (17 of 22 Micro-UIs show nothing today) |
| **Error** (the read or the call did not complete) | `--color-danger` | `⚠` (a triangle) | *Impossible de lire <objet>.* / *<Action> a échoué.* then a **Réessayer** control, then, in `small`, the cause when it helps. | A read: in the region that failed, replacing its content, with *Réessayer*. A command: in the status line at its control. `role="alert"`. It stays until the retry succeeds |
| **Queued** (the server holds it in its line, or parked) | `--color-secondary-text` | `◷` (a clock) | *En file d'attente : <action> passera à son tour.* and, in `small`, *Vous pouvez continuer.* (the command keeps its place) | The status line at the control that sent it; the control stays usable for another action. `role="status"`. Replaced by *confirmed* or *applied* |
| **Confirmed** (accepted by the server, not yet applied) | `--color-secondary-text` | `◑` (half-filled) | *Accepté : <action> va être appliquée.* | The same line, in place of *queued*. Today drawn nowhere in `apps/` (audit §10.6): the shell polls through it and draws nothing |
| **Applied** | `--color-success` | `✓` (a check) | The result in the past participle: *Fiche enregistrée.*, *« Bruit de foule » est dans la soundbox.* A command that undoes (deletion) adds **Annuler** ([§7](#7-destructive-actions)). | The same line, in place of *confirmed*. It stays until the DM acts again in that region or leaves, then clears. `role="status"`. **Exception:** an action repeated at the table (play a sound, move a token, change a volume) is acknowledged by its effect; every action that changes saved content (a sheet, a setting, a scene, a session) says *applied* |
| **Refused** (the DataGuard answered no) | `--color-danger` | `⊘` (a circle slashed) | *<Action> refusée : <la raison, dans les mots du DM>.* and, when known, the way out: *Renseignez le nom, puis réessayez.* | The same line at the control; it stays until the next attempt or until the DM dismisses it. `role="alert"`. **Always the word *refusé(e)***: the audit's *Pas envoyé*, *n'a pas pu être joué*, *Fenêtre non modifiée*, *Le micro n'a pas changé* say it in four other ways |

### 6.2 Telling them apart without colour

| pair that shares a colour | what differs |
|---|---|
| Empty / loading (neutral) | `○` static with a dashed outline and a sentence *Aucun…* / `◌` rotating, no outline, a sentence *Chargement…* |
| Queued / confirmed (pending violet) | `◷` / `◑`; *En file d'attente* / *Accepté* |
| Refused / error (danger) | `⊘` / `⚠`; *refusée* / *a échoué* or *Impossible de…*; a refusal has its reason and no retry (it would be refused again), an error has *Réessayer* |
| Applied, alone in green | `✓` and a past participle; a pending line never has a check |

### 6.3 Mapping of the server's statuses

| status | state |
|---|---|
| `queued`, parked | queued |
| `confirmed` | confirmed |
| `applied` | applied |
| `rejected` | refused |
| `expired` (parked, past 24 h) | refused: *<Action> expirée : elle n'a pas été appliquée dans les 24 heures.* |
| `cancelled` (by the DM) | the line clears |
| transport failure, timeout | error |

The glyphs are the product's own shapes, drawn in the kit as icons; the characters above stand for them until the kit task
decides the artwork. A state also has its words in the accessible name: the glyph is `aria-hidden`.

---

## 7. Destructive actions

### 7.1 What counts

An action is **destructive** when it removes or overwrites saved content, or ends something the DM cannot resume. An
action that only interrupts and can be repeated is not: it is an ordinary secondary action and is not red. By that rule
*Tout arrêter* (stops the sounds, which restart with one click) and *Annuler les changements* (resets the form to what is saved)
are **not destructive** today and lose their red; *Supprimer*, *Clore la séance*, *Retirer de la carte* are.

### 7.2 The one treatment

| | rule |
|---|---|
| **Look** | An outlined danger button: 1 px `--color-danger` outline (5.75–6.34:1), `--color-danger` text and a trash glyph before the label, no fill. In a dense list it may collapse to the glyph in a bordered 24 × 24 px button, with the verb as its accessible name and a tooltip, never to a colour. Red is a fill only on the confirm button of the dialog below. |
| **Place** | Never beside a harmless action: at least `--space-4` from it, at the end of its row or in its own group. (*Clore la séance* sits next to *Player view pilotée* today, audit §8.) |
| **Confirmation** | A destructive action is either **undoable** (tier A) or **confirmed** (tier B), never neither. Tier A: the action runs at once and its *applied* line carries **Annuler**, which restores it. Tier B: the kit Dialog, below. The tier follows the product, not the taste of a screen: an action is tier A when the DataGuard keeps a tombstone to restore it from (soft delete, `docs/ARCHITECTURE.md` rule 7) and the screen offers *Annuler*; until it does, it is tier B. |
| **Dialog** | Title: the question, with the object: *Supprimer le son « Bruit de foule » ?* Body (`body`): what is lost, what stays, whether it can be undone (*Les séances qui l'utilisent ne le joueront plus. Cette action est définitive.*). Two buttons, **safe first**: *Garder* (secondary, receives the initial focus) and *Supprimer le son* (the one filled button of the dialog, **`--color-danger` fill with `--color-primary-contrast` text, 6.35:1**; the dialog then has no accent button). Escape and the backdrop mean *Garder*. |
| **Wording** | A verb and its object, sentence case, on the button and in the title: *Supprimer le son*, *Retirer de la carte*, *Clore la séance*. Never *OK*, *Oui*, *Confirmer* alone. The applied line says it in the past: *« Bruit de foule » est supprimé.* |
| **After** | The *applied* line of [§6](#6-states); on refusal the *refused* line with its reason; the object is never left half-removed on screen. |
| **Keyboard** | The dialog traps focus, returns it to the button that opened it, and the danger button is reached after *Garder*. |

### 7.3 The audit's destructive actions, filed under the rule

| screen | action | tier | change |
|---|---|---|---|
| fiche-entite | × on a feature of the sheet; *Retirer l'image* | A | Undo restores the field; the sheet is already guarded by the leave-the-sheet prompt |
| fiche-entite | *Annuler les changements cochés* (versions dialog) | B, kept | The dialog button is the danger kind (it overwrites the sheet), not primary |
| preparation-seance | × on a PJ of the scene | A | Undo re-adds the PJ |
| preparation-seance, dm-view | *Réinitialiser la carte* | B, kept | The dialog gains the wording of 7.2 |
| medias | *Annuler les changements* | not destructive | An ordinary secondary button (today bare text) |
| soundbox | *Supprimer* a sound | B, then A | A dialog today (none), **Annuler** once the restore exists |
| soundbox | *Tout arrêter* | not destructive | Secondary, not red, no confirmation: at the table it must be one click |
| dm-view | *Clore la séance* | B, kept | The dialog already tells that the microphone is paused and the sounds stopped; it gets the danger button and *Garder* first |
| dm-view | *Retirer de la carte*, *Retirer le corps*, *Retirer du combat* | A | Undo re-adds the token as it was; no dialog at the table |

(*− Dégâts* is an adjustment of a number, not a removal; the audit lists it among the red buttons and it becomes an ordinary button.)

---

## 8. Focus and hover

### 8.1 Focus

Every interactive element has a visible focus ring, the same on every screen, and none ever has `outline: none` without a
ring in its place. The audit found six rings (cyan for forms, amber for the menu, tabs and breadcrumb, text colour for the
menu button, violet for the slider thumb, a border change only on the file drop and the hex map).

| token | value | note |
|---|---|---|
| `--color-focus` | `#27d7f0` | 11.11:1 on `--color-surface`, 10.09:1 on `--color-surface-2`, 9.42 and 8.30 on a hovered row. This is the value the kit already renders through the undefined `--color-accent-cyan` fallback: no pixel changes, the token becomes real |
| `--focus-ring-width` | `2px` | |
| `--focus-ring-offset` | `2px` | |

```css
:where(a, button, input, select, textarea, summary, [role="tab"], [tabindex]):focus-visible {
  outline: var(--focus-ring-width) solid var(--color-focus);
  outline-offset: var(--focus-ring-offset);
}
```

- The ring sits **outside** the control, on the surface behind it, so its contrast is `--color-focus` against that surface
  (≥ 3:1 required, 10.09–11.11 measured) whatever the control's own fill. A control drawn over a picture or a map
  (the battlemap's buttons, token handles) sits on a `--color-surface` plate so that holds there too.
- Text fields show the ring on `:focus`; other controls on `:focus-visible` (keyboard).
- A slider's thumb, a file drop, the hex map's view and a radio's label use the same ring (the slider's violet halo, the
  `outline: none` of the file drop and a border-only change on the map are replaced).
- **Focus is never hidden.** `scroll-padding-top` equals the sticky header's height plus `--space-3`: the audit found the
  *fiche-entite* rail button behind the 98 px header after a scroll.
- The ring is not the accent and not a state colour; if the owner picks cyan for the accent ([§9](#9-the-owners-choices)), the
  focus colour changes first.

### 8.2 Hover

Every interactive element has a hover state, and it is the same layer everywhere: nothing slides and nothing changes the
layout.

| token | value | note |
|---|---|---|
| `--state-hover` | `rgb(245 236 247 / 0.08)` | the text colour at 8% over the element or row; text on it holds ([§5.3](#53-states-on-the-hover-layer)) |
| `--state-hover-brightness` | `1.12` | for a filled button (primary, danger), as the kit's button already does |

| element | hover | pressed |
|---|---|---|
| Primary button (accent fill) | `filter: brightness(var(--state-hover-brightness))`; text 6.74:1 on the hovered fill | the same, and a 2 px inset ring in `--color-primary-contrast` |
| Danger button in a dialog (red fill) | the same | the same |
| Secondary (outlined), ghost, icon button, chip, menu link, breadcrumb, tab | `--state-hover` layer; the outline goes `--color-border-strong` → `--color-text-muted` | the layer, and the outline goes `--color-text` |
| Danger outlined button | `--state-hover` layer; the outline stays `--color-danger` | the layer, and the outline goes `--color-text` |
| Link | the underline thickens (`text-decoration-thickness: 2px`) | the same |
| Table row, list row, select option | `--state-hover` layer, no brightness filter, no violet bar | the same |
| Text field, select, textarea | the outline goes `--color-border-strong` → `--color-text-muted` | — |
| Checkbox, radio, toggle (the row) | `--state-hover` layer on the label | — |
| Disabled (any) | none; `cursor: not-allowed` | — |

The hover never uses the accent, amber or violet (the audit finds amber on hover in 11 Micro-UIs and the violet bar on the
table row). The pressed state is a border change rather than a second layer so every text colour keeps 4.5:1 on it.
`prefers-reduced-motion` removes the transition and the loading glyph's rotation; nothing is lost, since the states are
shapes and words.

---

## 9. The owner's choices

Three values are taste. Each has its rule here; the value is **left open** and pending the owner's answer to #753 (the
`harness:human` task), who chooses among the options drawn by #752. A later task writes the answers into this section and
the tokens.

| choice | the rule this document sets | working value, until #753 | open |
|---|---|---|---|
| **The accent colour** | Role and uses as [§3](#3-accent). Any value: ≥ 3:1 as a fill against `--color-surface` and `--color-surface-2`; a text colour on it at ≥ 4.5:1; not in the families of the states (green, red, violet) and not the focus cyan | `#e8702a` (`--color-primary` today) | **yes — the owner's** |
| **The display face** | Used for `display` and `title` only ([§2](#2-type)); every other step is `--font-family`. It must read in sentence case, have a 700 weight, carry a fallback stack that lands on a face of similar width, and stay legible at `--font-size-display` from the back of a table | the Impact stack (`--font-family-heading`) | **yes — the owner's** |
| **The DM view's density** | Density changes the spacing inside the DM view's panels by one step of `--space-1..5` and may move the `reading` step one step along the scale. It never goes under `small` (14 px), never changes a colour, a state or the destructive treatment, and never makes a control smaller than 24 × 24 px | not set | **yes — the owner's** |

---

## 10. Proposed tokens, in one place

All proposals; none is in `mock/design/tokens.css` today. "New" is a token to add, "changed" one whose meaning or value moves.

| token | value | status | section |
|---|---|---|---|
| `--font-size-display` | `clamp(2.5rem, 1.75rem + 3vw, 4.5rem)` | new | §2 |
| `--font-size-stage` | `clamp(1.25rem, 1rem + 1.2vw, 2rem)` | new | §2 |
| `--font-weight-regular` | `400` | new | §2 |
| `--font-weight-semibold` | `600` | new | §2 |
| `--line-height-tight` | `1.15` | new | §2 |
| `--line-height-snug` | `1.35` | new | §2 |
| `--line-height-body` | `1.5` | new | §2 |
| `--color-text-disabled` | `#9a87ad` | new | §5 |
| `--color-secondary-text` | `#b98af0` | new | §5 |
| `--color-border-strong` | `#7d6a96` | new | §5 |
| `--color-focus` | `#27d7f0` | new (replaces the undefined `--color-accent-cyan`) | §8 |
| `--focus-ring-width`, `--focus-ring-offset` | `2px`, `2px` | new | §8 |
| `--state-hover` | `rgb(245 236 247 / 0.08)` | new | §8 |
| `--state-hover-brightness` | `1.12` | new | §8 |
| `--color-art-gold` | `#f5b83d` | `--color-accent` renamed; read only by artwork | §3 |
| `--color-primary` (the accent role) | `#e8702a` | unchanged; value open ([§9](#9-the-owners-choices)) | §3 |
| `--color-text`, `--color-text-muted`, `--color-success`, `--color-danger`, `--color-surface`, `--color-surface-2`, `--color-border`, `--space-1..5` | as today | unchanged | §4, §5 |

## 11. Left open, not decided here

- **How a screen names its one primary action** to the Micro-UIs it composes (§3): a screen-level decision for the token and
  kit task.
- **The status line and the icon set** are kit components to write on `design-system` (contract O): this document fixes
  their behaviour and anatomy, not their artwork.
- **A command that waits for its author** (`awaiting_confirmation`, `awaiting_review`; the author confirms an overwrite,
  `docs/ARCHITECTURE.md` rule 8) has no state among the seven. It takes the *queued* line plus an accent-free secondary
  *Confirmer l'écrasement* control; its design belongs to the task that builds that control (the audit found none).
- **Touch target size.** WCAG 2.2's 24 × 24 px is the floor held here; whether the primary actions rise to 44 px on narrow
  screens is a layout question for the screens' tasks (the audit counts, per screen, the controls under 44 px).
- **Layout failures the audit measured** (the 637 px and 724 px overflow at 360 px on *campagne* and *soundbox*, the order
  of the panels, the sticky header's height) are not typography, colour or states and are not argued here.

---

## DM view

The screen the DM uses live, at the table, with a sheet to read, a map to move tokens on and a microphone listening. This
brief keeps their attention on what they read: they always know where to look for the live state, what they do not need
at the moment folds away, and the suggestion pill and the microphone never cover or push what they read.

- **Status.** Proposal, like the rest of this document: it changes no token, kit component, mock page or file under `apps/`.
  The build tasks that follow carry it into the mock-up and the Micro-UIs.
- **Read from.** `mock/pages/dm-view.html`, `apps/screens/dm-view.json` (the six Micro-UIs `LiveSessionBar`,
  `ElementsPanel`, `TableSheet`, `Battlemap`, `LiveSoundbox`, `SuggestionPill`), the audit's §8 (`UI-AUDIT.md`) and the
  rules of `rules.json` that name the DM view (R17, R18, R22, R98, R109, R110).
- **Widths.** As the audit measured them: **360 px** (phone), **tablet** (768 px; every width from 561 to 960 px, where the shared CSS is
  already one column) and **1280 px** (every width from 961 px up; 961–1279 px was not rendered by the audit and takes this
  layout, which is why the left column below has a minimum width).
- **Not decided here.** The DM view's *density* ([§9](#9-the-owners-choices)) and the options for it belong to the next slices. Nothing below depends on a
  density: a density changes the spacing inside a region by one step, never which region is visible or folded.
- **The pill is held to R98, as written.** *In the DM view, the suggestion pill and the mic on/off button form a layer over
  the page, fixed at the bottom right of the window, keeping that position whatever the centre shows and while the page or
  the left panel scrolls, passing over the content without shifting it, and staying clickable.* Nothing in this brief moves
  the pill, changes how it is anchored or makes it less reachable; see [the pill and the microphone](#the-pill-and-the-microphone-r98) for the
  check, and [when R98 would have to change](#when-r98-would-have-to-change).

### What the audit found that this brief answers

At 360 px the sticky header (site menu + live bar) is 517 px and the pill 524 px of an 800 px window: together **784 px**, so
the page shows in 16 px. The centre's tab bar comes after the whole elements panel (2256 px down the page). At 1280 px the
header is 231 px (29 % of the window) and the pill covers the *Soundbox* tab, the right of the sheet's title and the end of
its action row. The session bar puts *Clore la séance* beside *Player view pilotée* (audit §8; [§7.2](#72-the-one-treatment) forbids it). The microphone's
state is written in the live bar (*Micro actif*) and again in the pill (*Écoute en local*, *J'écoute…*, *Micro en pause*), and
the current scene in the bar's chips, the panel's `is-current` mark and the battlemap's own title.

### Regions: always visible, and what folds away

"Always visible" means in the window without a scroll, at any scroll position of the page. "Folds" means closed by default,
opened by one action named in the last column, and closed again by the same action, by *Échap*, or by what the opening was for.
Opening or closing a region that is **not** the page's flow (a drawer, a menu, the pill's card) never moves the centre.

| region (today's id) | 360 px | tablet | 1280 px | folds / unfolds |
|---|---|---|---|---|
| **Live strip** (`#session-bar`, [below](#the-live-strip-the-one-place-for-the-live-state)) | always, sticky, 2 rows | always, sticky, 1 row | always, sticky, 1 row | does not fold |
| **Site menu** (`#site-header` menu) | not sticky: scrolls away, brand and menu button only | not sticky | not sticky, one row | the menu button (the kit Menu as it renders at each width) |
| **Centre tab bar** (*Fiche · Battlemap · Soundbox*, `#dm-center-tabs`) | always, sticky under the strip | always, sticky under the strip | always, sticky under the strip | does not fold; it is what switches the centre |
| **Centre content** (sheet, battlemap, Soundbox) | always | always | always | one of three, by the tab |
| **Elements panel** (`#dm-elements`) | folded, a drawer | folded, a drawer | open, left column, sticky, scrolls inside | 360 and tablet: the *Éléments* button at the start of the tab bar; 1280: the *Éléments* button on the column's edge folds it to a `--space-5` rail |
| **Elements panel, scenes other than the current one** | closed | closed | closed | the scene's own `summary` (native disclosure); the current scene is open |
| **Session actions** (*Ouvrir la player view*, *Player view pilotée*, *Clore la séance*) | folded | folded | folded | the *Séance* menu button in the strip |
| **Broadcast undo** (*Revenir à la couverture*, *Fermer la fenêtre*), only while something is sent | in the *Séance* menu | in the strip, beside the sentence they undo | same | the menu at 360 px; inline otherwise |
| **Sheet actions** (*Modifier*, *Envoyer l'illustration* in *Scène* / *Fenêtre*) | *Modifier* visible; the sends fold | *Modifier* visible; the sends fold | *Modifier* visible; the sends fold | *Envoyer aux joueurs* menu button in the sheet's title row |
| **Battlemap tools** (*Lancer le combat*, *Terminer le combat*, *Réinitialiser la carte*, *Envoyer aux joueurs*) | the map and the token inspector; the tools fold | same | same | the *Carte* menu button in `#battlemap-toolbar` |
| **Initiative carousel** (`#initiative-carousel`) | shown only while a combat runs (as today) | same | same | appears with *Lancer le combat*, leaves with *Terminer le combat* |
| **Pill and microphone layer** (`<aside>` of `SuggestionPill`) | always, fixed, bottom right, folded to one row | always, fixed, bottom right, folded to one row | always, fixed, bottom right, open | its chevron, [below](#the-pill-and-the-microphone-r98) |

Rules behind the table:

1. **Order in the page is the order of attention.** In the markup and on screen: strip, tab bar, centre, then the drawer.
   Today the elements panel comes first, so at 360 px and tablet the DM scrolls past every element to reach the tab bar. The
   drawer is not in the flow: it opens over the centre from the left, no wider than the window minus `--space-5`, moves the focus
   into itself and gives it back to its button on close (it is not modal: the pill stays usable above it), and closes by
   itself when the DM picks an element, since picking an element opens it in the centre (R17).
2. **The elements panel keeps its content and R17's order**: the session's elements first, the scenes one by one. What
   changes is that only the current scene is open. The search field stays at the top of the panel, always shown.
3. **A destructive action folds and keeps [§7.2](#72-the-one-treatment)'s treatment inside**: *Clore la séance* and *Réinitialiser la carte* are the
   last item of their menu, in a group of their own at least `--space-4` from the others, outlined danger with the trash
   glyph, with the dialog of §7.2 (*Garder* first). A menu is never the way a destructive action skips its confirmation.
4. **Nothing the DM does during a fight is behind a fold.** *Suivant ▸*, the round, the order, a token's *PV* and its
   visible/hidden switch stay where they are, with the map: a fold is for what is done once or rarely per session.
5. **Menus are the kit's.** *Séance*, *Envoyer aux joueurs* and *Carte* are one component (a button with `aria-expanded`
   and a list of buttons, `Échap` closes, the focus returns to the button), not three inventions. It is a kit component to write
   on `design-system` (contract O); until it exists a screen task recodes it as [the kit instruction](../mock/kit/kit.json) says.
6. **Sticky budget.** The strip and the tab bar together are the only sticky header: at most **25 %** of the window's height at 360 px
   (200 px of 800) and at most **13 %** at tablet and 1280 px (about 100 px of 800), against 517, 335 and 231 px today. The
   page's `scroll-padding-top` is that height plus `--space-3` ([§8.1](#81-focus)).

### The live strip: the one place for the live state

The live state is told in **one place**, the strip (`#session-bar`, `LiveSessionBar`), and nowhere else. It is the first
thing in the page, sticky, the same position at every width, and the DM looks there for any of these five facts:

| fact | wording | where it is told |
|---|---|---|
| **The session runs** | *Séance 4 — La Taverne · En direct depuis 19:02* (*Close* / *Clôturée* when it is closed) | the strip's title |
| **The microphone** | *Micro actif* or *Micro en pause* (or *Micro éteint* before the first start) | the strip, as a sentence with the state's shape ([§6.2](#62-telling-them-apart-without-colour)), `aria-live="polite"` |
| **The current scene** | *Scène 2 / 5 · La taverne*, with *‹* and *›* to step to the previous or next scene | the strip; the chips of every scene are removed from it (that was the wrap onto many rows at 360 px) |
| **The combat turn** | *Combat · round 3 · tour de Bandit 2*, only while a combat runs | the strip, `aria-live="polite"` |
| **What the players see** | *Les joueurs voient la battlemap + une illustration*, with its undo | the strip (today's `#session-bar-showing`) |

**No other region repeats it.** The rule is: *a fact of the table above is stated as a sentence once*. The other regions
may carry a **mark** on the thing the fact is about, never a second sentence:

- The elements panel marks the current scene's `summary` (`is-current`, a shape and `aria-current="true"`) but writes no
  *Scène en cours* line. The scene list lives only in the panel; the strip steps through it.
- The initiative carousel marks the active participant, as the thing it lists, and keeps the round counter that R22 asks of it
  (« un compteur de rounds »): they are the instrument's own values, not a sentence, and the brief does not remove them. It writes
  no *Tour de …*; the strip's *Combat · round 3 · tour de Bandit 2* is the statement, and the one the DM reads when the
  centre shows the sheet or the Soundbox.
- The microphone button of the pill is the control (its icon and `aria-pressed`, [below](#the-pill-and-the-microphone-r98)). The words
  *Écoute en local*, *J'écoute…* and *Micro en pause* leave the pill; what the microphone heard stays in the pill as the
  suggestion's own `via` (« la taverne »).
- The battlemap's title and the sheet's title name their content, not the session or the scene.

The strip is a state, not a toolbar: apart from the scene steppers its only control is the *Séance* menu button, which holds
the player-view links, the broadcast undo at 360 px, and *Clore la séance* as its last, separated item.

### The pill and the microphone (R98)

The `SuggestionPill` is a layer over the page. It is already built so (`position: fixed; right: var(--space-3); bottom:
var(--space-3); z-index: 1000`, the last child of `<body>`/the shell, a `complementary` landmark); the brief keeps that and
adds only what is inside the layer.

| R98 says | the brief |
|---|---|
| **a layer over the page, fixed at the bottom right of the window** | `position: fixed`, `right` and `bottom` of `--space-3`, never `static`, `sticky` or `absolute`; the same box at every width and in every state of the centre. Nothing is added to the pill that anchors it to another region (not the strip, not the centre, not the left panel) |
| **whatever the centre shows** | the same anchor for the long sheet scrolled to its end, the battlemap and the *Soundbox* tab; no tab of the centre moves it, resizes it or hides it |
| **while the page or the left panel scrolls** | it does not scroll with either; the drawer and the menus open **under** it (the pill's `z-index` is above theirs) so that it stays clickable |
| **passing over the content without shifting it** | the layer is out of the grid: opening, folding or filling it changes the centre's size by no pixel. The room the DM needs to read behind it is [a reserve](#what-the-dm-reads-keeps-room) fixed in advance, not a size that follows the pill |
| **staying clickable** | no other element has a `z-index` or a `pointer-events` that catches a click meant for it; a click on a suggested entity opens it in the centre (the `fdm:open` of today's page), on the *Fiche* tab, whichever tab was open |

What is inside the layer, and the two states of its card:

- **The microphone button** is always shown in the layer's bottom row, the same size in both states (compact
  `kit-micbutton`, 48 px), the one control of the page that starts the listening. It carries the *action* (a microphone
  icon, a pause icon, `aria-pressed`, `aria-label` *Micro de séance*), not the *state in words* (the strip's).
- **The card of suggestions** has two states, and R98's box is the same in both (anchor, right edge, bottom edge):
  - **Open**: the stack of the last five (R18), the history icon at the top right of the card (R109), a card height capped at
    **40 %** of the window's height, scrolling inside. Default at 1280 px, since the layer there covers the right of the centre
    least and the rules' journeys read the stack without a click.
  - **Folded**: one row, the newest suggestion (its name and kind) beside the microphone button, so the entity stays one click
    from the centre. Default at 360 px and tablet, where the open card would hide the page (524 of 800 px today).
  - The chevron at the card's top edge, `aria-expanded`, switches them; the choice is kept for the session per width class.
    Picking an entity or an armed sound **folds** the card, so that what the DM asked to read is not behind it.
    A new suggestion does not unfold a folded card: it replaces the row, with a short, `prefers-reduced-motion`-safe, pulse.
- **The pill does not carry** a status paragraph of its own for the microphone ([above](#the-live-strip-the-one-place-for-the-live-state)); its
  refusal line (*Le micro n'a pas changé : …*) stays, since it is the answer to the pill's own control (the status line of §6).
- **The pill does not hold** any control of the sheet, the map or the Soundbox; *Tout arrêter* belongs to the Soundbox tab.

The check against R98's wording: every line above keeps the layer, its anchor, its independence from the centre and from
scrolling, the absence of any shift, and the click on the suggested entity. Folding changes what is *drawn in* the layer,
not where it is or whether it can be clicked, so it is not a change of R98. The journey of R98 (`tests/rules/journey/R98-…spec.ts`)
runs at the default 1280 px window, where the card is open; it reads `#suggestions-stack button[data-kind="entity"]`,
`#suggestions-mic` and the layer's `position: fixed`, which the brief leaves as they are. The build task keeps those ids.

#### When R98 would have to change

The brief needs **no** change to R98, so no `harness:rules` issue stands behind it: [the list of facts](#the-live-strip-the-one-place-for-the-live-state)
and the layer's states hold inside the rule's wording. The section keeps R98's wording as it is. If a later slice, the owner's
choice of density or a build task finds that the pill must be somewhere else (another corner, docked in the strip, not over
the content) or must behave otherwise (shift the content, scroll with it, hide while the map is shown), it does not write that layout.
It opens a `harness:rules` issue stating the change and why, links it here, and this section keeps R98's wording until that issue is
settled. A layout that works around R98 is a violation of the rules' process: a rule is changed through its own issue, never beside it.

### What the DM reads keeps room

The three things the DM reads (the sheet, the battlemap, the Soundbox) each end with a **reserve**: empty space inside the
centre, after the last line and the last control, at least as tall as the layer in its **tallest resting state**, so that
scrolled to its end nothing is under the pill. The pill never moves for it; the reserve is the centre's.

- **The size.** The layer's tallest resting state, plus `--space-3`, rounded up to a multiple of `--space-5`:
  - at 360 px and tablet (folded: one row of about 56 px): `calc(var(--space-5) * 3)`, 120 px;
  - at 1280 px (open: the card at its 40 % cap plus the microphone row): `calc(40vh + var(--space-5) * 3)`, 440 px of a 800 px window.
  The values are the starting ones; the build task measures the rendered layer and takes the next multiple up. The reserve
  is fixed per width class: folding or unfolding the card changes none of it, since a reserve that follows the pill would shift
  the content, and R98 forbids it.
- **Where it goes.**
  - *Sheet*: `padding-bottom` of the sheet's body (today the centre has a flat `calc(var(--space-5) * 7)`, 280 px, which is below
    the open card's 524 px: replace it by the two values above). The last statblock line and the last action are reachable.
  - *Battlemap*: the map is not scrolled, so the reserve is a band under the map and the carousel in the panel's flow, and the
    battlemap's own controls (`#battlemap-toolbar`, `#carousel-controls`, the token inspector) are placed **top and left of the
    map**, never in the bottom-right corner of the centre where the layer stands.
  - *Soundbox*: `padding-bottom` of the grid, and the controls of the whole tab (search, tags, the global volume, *Tout
    arrêter*) sit in its top row, so that the one control a DM must reach in a second is never under the layer.
- **Focus is never hidden under the layer either**: `scroll-padding-bottom` of the page equals the reserve, as
  [§8.1](#81-focus)'s `scroll-padding-top` equals the header. A control focused by `Tab` at the bottom of a sheet scrolls into view above
  the layer, not under it. The layer is last in the tab order and reachable by shortcut ([below](#keyboard)).
- **What the reserve does not do.** It covers the *end* of a content. While a sheet scrolls, its lines pass under the layer;
  that is R98's "passing over the content", and the lines come out from under it as they scroll. This is why the map's and
  the Soundbox's controls, which do not scroll, are never placed in the bottom-right corner.

### Keyboard

Every action a DM repeats at the table has a key, so they do not hunt for a button. The keys are chords of
**Alt+Shift** (Option+Shift on a Mac) and a letter or a digit:

- No shortcut is a bare character key: a bare letter is the screen readers' quick navigation (NVDA and JAWS in browse mode take
  `M`, `N`, `H`, `B`, `1`–`6`…) and the browser's typing, and WCAG 2.1.4 would then ask for a way to turn it off. A chord
  with a modifier needs none.
- The shortcuts work wherever the focus is **except** in a text field, a `select` or a `contenteditable` (the search of the
  panel, the notes), and while a dialog is open (where only *Échap* and the dialog's own keys act). They ignore a key held
  down (`event.repeat`), so *Tour suivant* advances once.
- The chord is read from `event.key` when it is a letter and, when it is not (Option+Shift on a Mac types another character),
  from `event.code`. A French DM on an AZERTY keyboard presses the key *labelled* with the letter; digits are the top row.
- No page defines an `accesskey`, so Firefox's Alt+Shift accesskeys have nothing to fire on.
- Each shortcut is written in the control's `aria-keyshortcuts` and in its tooltip, and a *Raccourcis* item of the *Séance*
  menu lists them.
- **No shortcut closes the session, resets the map, removes a token or a participant, or plays a sound** ([§7](#7-destructive-actions): a
  destructive action is reached by its button and confirmed).

| keys | action | scope | checked against |
|---|---|---|---|
| **Alt+Shift+M** | microphone on / off (`live.setMicrophone`); the strip's *Micro actif / en pause* announces the result | the whole DM view, whatever the centre shows | Chrome and Edge: Alt+Shift+T and B are theirs (toolbar and bookmarks focus), not M; Firefox: Alt+Shift accesskeys, none defined. NVDA, JAWS and VoiceOver: no Alt+Shift+M. Mac: Option+Shift+M types « Â » in a field, which is why the shortcut is off in fields |
| **Alt+Shift+N** | next turn (`table.nextTurn`, the same as *Suivant ▸*); the strip announces *tour de …* | only while a combat runs; otherwise nothing happens and nothing is announced | in no published list of those browsers or screen readers. It has no inverse, as the button has none: the key repeat is ignored and the result is read aloud |
| **Alt+Shift+O** | open the top suggestion (the pill's newest entity) in the centre, on the *Fiche* tab; for a sound it only arms it (the second click, not a key, plays) | the whole DM view; when there is no suggestion, nothing happens | in no published list of those browsers or screen readers |
| **Alt+Shift+1 / 2 / 3** | switch the centre to *Fiche* / *Battlemap* / *Soundbox* | the whole DM view | the digits are the top row on any layout (on AZERTY with the Shift the chord already holds). Chrome and Firefox switch browser tabs with Ctrl+1–8, Firefox on Linux with Alt+1–8, never with Alt+Shift; NVDA's `1`–`6` are bare keys |
| **← → Home End** | move between the three tabs once a tab has the focus (WAI-ARIA tablist) | the tab bar, focused | the tablist's own keys; Alt+Left / Right (browser back / forward) are not used |
| **Alt+Shift+L** | fold or unfold the elements panel (the drawer at 360 px and tablet, the column at 1280 px) | the whole DM view | in no published list of those browsers or screen readers |
| **Alt+Shift+P** | unfold or fold the pill's card | the whole DM view | in no published list of those browsers or screen readers |
| **Échap** | close the open menu, drawer or pill card, in that order of nesting; focus returns to the button that opened it | when one is open | already the dialogs' key in the Micro-UIs |

**What "checked" means here.** The table was checked against the published shortcut lists of Chrome, Edge and Firefox and the
quick-navigation and modifier keys of NVDA (Insert or Caps Lock), JAWS (Insert) and VoiceOver (Control+Option); this is a reading
of their documentation. No screen reader was run for this document. The build task runs each row on Chrome, Firefox
and Safari and with NVDA and VoiceOver, on a QWERTY and an AZERTY layout, and a row that clashes changes its key (the
table is the contract, the letters are not). The shortcuts are the only keyboard additions: the Micro-UIs add the key handler,
and it is a Micro-UI that calls the DataCapability, never the shell's page script.

### Reading it back: the acceptance of this brief

- At each width, the DM sees without scrolling: the strip, the tab bar, the start of the centre content and the layer
  (folded at 360 px and tablet). The elements, the session actions, the sends and the map's tools are one action away.
- The live state is one sentence per fact, in the strip. The pill has the microphone's button and the suggestions, no state sentence.
- The layer is the R98's layer. The three contents end with a reserve, and nothing the DM needs a second after another is under
  the layer's corner.
- Every shortcut is a chord, outside fields and dialogs, none destructive, and the table names its checks.
