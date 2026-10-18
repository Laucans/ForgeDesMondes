# Contest — R4, « Joueurs et personnages distincts »

Task #60 (Capability `campaign.getEntity`), milestone #18.

## The rule and its test

- Rule: **R4** — given a PC linked to a player and an NPC, when the DM opens
  the campaign's tabs, then each appears in the right tab with its sheet
  (stats, AC, HP, spells, actions), its token and its notes.
- Test: `tests/rules/business/tests/r4_players_and_characters_apart.rs`: it
  reads `campaign.listPlayers`, `campaign.listCharacters`, `campaign.listPlaces`,
  `campaign.listEvents` and `campaign.getEntity` from the running server and
  requires `elowen` (a pj with a listed player) and `morlox` (a pnj with none)
  among the characters, each with its full sheet from `campaign.getEntity`.

## Why it cannot hold from this unit

`campaign.getEntity` was the last unit the test waits for. It is built
(`crates/campaign/capabilities/get-entity/`, its query registered, 22 hermetic
tests green, the DEX modifier of `elowen` is +4), so the rule is no longer
pending; against the rules' runtime it is red, for reasons none inside the unit:

1. **The server does not dispatch the Capabilities.** `Reads::run`
   (`crates/dataguard/server/src/reads.rs`) answers only `ai.getSettings`; any
   other identifier answers `data: null`, and it takes no variables, so a read
   bound to `$1` (`$id` here, `$campaign` for the lists) cannot be run.
   Observed: `POST /capabilities/campaign.listPlayers` and
   `POST /capabilities/campaign.getEntity` → `{"asOf":0,"data":null}`; the test
   stops at « the PJ is not among the characters », before `getEntity` is read.
   Wiring the five campaign Capabilities there is an edit of
   `dataguard-server`'s `Cargo.toml` and `reads.rs`, plumbing that is not this
   task's unit (rule 1: one unit per task).
2. **No fixture reaches the database.** `fixtures()` in
   `crates/server/src/lib.rs` skips every file ("no aggregate holds it"), so the
   `entity` and `character` tables of a scratch database are empty: nobody is
   listed, whatever the Capabilities answer.

What this unit does satisfy: with the rows the store holds for `elowen` and
`morlox`, `answer` gives each its `stats` (FOR to CHA), `ca`, `pvMax`,
`features` (actions, then spells), `initials` and `notes` as in
`mock/fixtures/entities.json` (unit tests), the DEX modifier of a score of 18
is +4, and a token with neither portrait nor token image is the initials on
`color`.

## What is not mine to change

`tests/rules/`, `rules.json`, `scripts/rules-runtime.sh`, and the server's
plumbing outside this unit. R4 can pass once the server wires
`campaign.listPlayers`, `listCharacters`, `listPlaces`, `listEvents` and
`getEntity` with their variables, and seeds the fixtures' campaign, players and
elements into the store.
