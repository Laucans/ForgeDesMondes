import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import {
  countText,
  FOLD_KEY,
  level,
  normalize,
  parseCharacters,
  parseElements,
  parseMedia,
  parsePlayers,
  parseSounds,
  placeGlyph,
  railIndex,
  readFolded,
  score,
  search,
  synced,
  writeFolded,
  type Memory,
} from '../src/model.ts';

interface Case {
  name: string;
  input: { term: string; other?: string };
  output: { normalized_term: string; match: boolean | null };
}

/** The Concept's own fixtures: the normal form the search matches on is the one AliasMatch@1 states. */
const aliasMatch = JSON.parse(readFileSync(new URL('../../../../concepts/AliasMatch/v1/fixtures.json', import.meta.url), 'utf8')) as { cases: Case[] };

for (const c of aliasMatch.cases) {
  test(`AliasMatch@1: ${c.name}`, () => {
    assert.equal(normalize(c.input.term), c.output.normalized_term);
    if (c.input.other !== undefined) {
      const same = normalize(c.input.term) !== '' && normalize(c.input.term) === normalize(c.input.other);
      assert.equal(same, c.output.match);
    }
  });
}

const entry = (name: string, ...aliases: string[]) => ({ name, aliases });

test('a name is found by its alias, whatever the accents and the case', () => {
  const elowen = entry('Elowen Ventargent', 'Elowen', 'la rôdeuse');
  assert.equal(score(elowen, 'elowen'), 2);
  assert.equal(score(elowen, 'rodeuse'), 2);
  assert.equal(score(elowen, 'RÔDEUSE'), 2);
  assert.equal(score(elowen, '  la Rôdeuse '), 2);
});

test('a word within one letter (two for a long word) of a word of the name still matches, less well', () => {
  const brannoc = entry('Brannoc Piedferré', 'le prêtre');
  assert.equal(score(brannoc, 'bronnoc'), 1);
  assert.equal(score(brannoc, 'pretrre'), 1);
  assert.equal(score(brannoc, 'piedfer'), 2);
  assert.equal(score(brannoc, 'zzz'), 0);
  // Two letters off a short word is too far; a word of two letters is not compared.
  assert.equal(score(entry('Kael'), 'kxxl'), 0);
  assert.equal(score(entry('Kael'), 'ka zz'), 0);
});

test('a long word may be two letters off, a short one only one', () => {
  assert.equal(score(entry('Valombre'), 'valonbrx'), 1);
  assert.equal(score(entry('Valombre'), 'valxnbrx'), 0);
  assert.equal(score(entry('Orcs'), 'oxxs'), 0);
  assert.equal(score(entry('Orcs'), 'orxs'), 1);
});

test('every word of the search has to be near a word of the entry', () => {
  const tour = entry('La Tour d’Ébène', 'la tour');
  assert.equal(score(tour, 'tour ebene'), 1);
  assert.equal(score(tour, 'tour zzzzzz'), 0);
});

test('a blank search matches nothing and keeps the whole list', () => {
  assert.equal(score(entry('Elowen'), '   '), 0);
  const index = railIndex(parseCharacters([{ id: 'a', name: 'A', kind: 'pj' }]), [], []);
  assert.equal(search(index, ''), index);
  assert.equal(search(index, '  '), index);
});

test('the best matches come first, those that hold the search before those that are near it', () => {
  const index = railIndex(
    parseCharacters([
      { id: 'near', name: 'Elowin', aliases: [], kind: 'pnj' },
      { id: 'exact', name: 'Elowen', aliases: [], kind: 'pj' },
      { id: 'none', name: 'Kael', aliases: [], kind: 'pj' },
    ]),
    [],
    [],
  );
  assert.deepEqual(search(index, 'elowen').map((e) => e.id), ['exact', 'near']);
});

test('the rail holds PJ, PNJ, places and events, each with its sub-line and its initial', () => {
  const index = railIndex(
    parseCharacters([
      { id: 'elowen', name: 'Elowen', aliases: ['la rôdeuse'], kind: 'pj', initials: 'EV', art: 'forest', type: 'Elfe' },
      { id: 'bandit', name: 'Bandit', aliases: [], kind: 'pnj', initials: 'B', type: 'Humain, brigand' },
    ]),
    parseElements([{ id: 'tour', name: 'La Tour d’Ébène', aliases: ['la tour'], art: 'night' }, { id: 'moulin', name: 'Le Moulin', aliases: [] }]),
    parseElements([{ id: 'feu', name: 'L’incendie', aliases: ['le feu'] }]),
  );
  assert.deepEqual(
    index.map((e) => [e.id, e.group, e.sub, e.art, e.glyph]),
    [
      ['elowen', 'pj', 'la rôdeuse', 'forest', 'EV'],
      ['bandit', 'pnj', 'Humain, brigand', 'night', 'B'],
      ['tour', 'place', 'la tour', 'night', 'T'],
      ['moulin', 'place', '', 'night', 'M'],
      ['feu', 'event', 'le feu', 'blood', '!'],
    ],
  );
});

test('a place’s initial skips its article', () => {
  assert.equal(placeGlyph('La Taverne du Sanglier'), 'T');
  assert.equal(placeGlyph('Le Moulin brûlé'), 'M');
  assert.equal(placeGlyph('L’incendie'), 'i');
  assert.equal(placeGlyph('Valombre'), 'V');
});

test('a malformed answer is an empty list, not an error', () => {
  for (const data of [null, undefined, 'x', 3, {}, [null, 4, 'y']]) {
    assert.deepEqual(parsePlayers(data).filter((p) => p.id !== ''), []);
    assert.deepEqual(parseCharacters(data).filter((p) => p.id !== ''), []);
    assert.deepEqual(parseElements(data).filter((p) => p.id !== ''), []);
    assert.deepEqual(parseMedia(data), []);
    assert.deepEqual(parseSounds(data), []);
  }
  assert.deepEqual(parseCharacters([{ id: 'x', name: 'X', aliases: ['a', 3, null], kind: 'weird' }])[0]?.aliases, ['a']);
  assert.equal(parseCharacters([{ id: 'x', name: 'X', kind: 'weird' }])[0]?.kind, 'pnj');
});

test('a character’s player and D&D Beyond link are read as the Capability words them', () => {
  const [pj, pnj] = parseCharacters([
    { id: 'a', name: 'A', kind: 'pj', player: 'jou-1', ddb: { url: 'u', syncedAt: '2026-09-26T18:12:00' } },
    { id: 'b', name: 'B', kind: 'pnj', player: null, ddb: null },
  ]);
  assert.equal(pj?.player, 'jou-1');
  assert.equal(pj?.ddbSyncedAt, '2026-09-26T18:12:00');
  assert.equal(pj?.hasDdb, true);
  assert.equal(pnj?.player, null);
  assert.equal(pnj?.hasDdb, false);
  assert.equal(synced(pnj, (at) => at), '—');
  assert.equal(synced(pj, (at) => at.slice(0, 10)), 'Synchronisé le 2026-09-26');
  const linkedNever = parseCharacters([{ id: 'c', name: 'C', kind: 'pj', ddb: { url: 'u', syncedAt: null } }])[0];
  assert.equal(linkedNever?.ddbSyncedAt, null);
  assert.equal(linkedNever?.hasDdb, true);
});

test('the level is the one the text states', () => {
  assert.equal(level('Elfe des bois, rôdeuse niveau 5'), '5');
  assert.equal(level('Humain, brigand'), '—');
  assert.equal(level(''), '—');
});

test('media: the battlemaps then the illustrations, sized as the store says or by their pixels', () => {
  const media = parseMedia({
    battlemaps: [{ id: 'bm', title: 'Le Gué', width: 960, height: 640 }, { id: 'bm2', title: 'Moulin', size: '1,9 Mo' }],
    illustrations: [{ id: 'ill', title: 'Gaspard', art: 'gold', glyph: 'G' }, { id: 'ill2', title: 'Valombre' }],
  });
  assert.deepEqual(
    media.map((m) => [m.id, m.kind, m.size, m.art, m.glyph]),
    [
      ['bm', 'battlemap', '960×640', 'forest', '⬡'],
      ['bm2', 'battlemap', '1,9 Mo', 'forest', '⬡'],
      ['ill', 'illustration', '', 'gold', 'G'],
      ['ill2', 'illustration', '', 'night', 'V'],
    ],
  );
});

test('sounds: the library’s list, with its tags', () => {
  assert.deepEqual(parseSounds({ masterVolume: 80, sounds: [{ id: 's', name: 'Porte', tags: ['porte', 3] }, 'x'] }).filter((s) => s.id !== ''), [{ id: 's', name: 'Porte', tags: ['porte'] }]);
});

test('the count says how many lines the search kept', () => {
  assert.equal(countText('', 4), '');
  assert.equal(countText('  ', 4), '');
  assert.equal(countText(' elowen ', 1), '1 résultat pour « elowen »');
  assert.equal(countText('elo', 2), '2 résultats pour « elo »');
  assert.equal(countText('zzz', 0), 'Rien pour « zzz »');
});

function storage(initial: Record<string, string> = {}): Memory & { kept: Record<string, string> } {
  const kept = { ...initial };
  return { kept, getItem: (key) => kept[key] ?? null, setItem: (key, value) => void (kept[key] = value) };
}

test('the fold of the rail is kept and read back', () => {
  const memory = storage();
  assert.equal(readFolded(memory), false);
  writeFolded(memory, true);
  assert.equal(memory.kept[FOLD_KEY], '{"collapsed":true}');
  assert.equal(readFolded(memory), true);
  writeFolded(memory, false);
  assert.equal(readFolded(memory), false);
});

test('a browser with no storage, or a broken value, leaves the rail unfolded and loses nothing else', () => {
  assert.equal(readFolded(null), false);
  assert.equal(readFolded(storage({ [FOLD_KEY]: 'not json' })), false);
  assert.equal(readFolded(storage({ [FOLD_KEY]: '"yes"' })), false);
  assert.equal(readFolded(storage({ [FOLD_KEY]: '{"collapsed":"true"}' })), false);
  const refusing: Memory = { getItem: () => { throw new Error('denied'); }, setItem: () => { throw new Error('full'); } };
  assert.equal(readFolded(refusing), false);
  assert.doesNotThrow(() => writeFolded(refusing, true));
  assert.doesNotThrow(() => writeFolded(null, true));
});
