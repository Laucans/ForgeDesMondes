import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, test } from 'node:test';
import {
  buildIndex,
  campaignGroups,
  commandBody,
  commandId,
  isFinal,
  normalize,
  parseCharacters,
  parseEvents,
  parseLibrary,
  parsePlaces,
  parseScenes,
  reasons,
  same,
  sceneGroups,
  score,
  search,
  settled,
} from '../src/model.ts';
import type { World } from '../src/types.ts';

const json = (path: string): unknown => JSON.parse(readFileSync(new URL(`../../../../${path}`, import.meta.url), 'utf8'));
const mock = (name: string): unknown => json(`mock/fixtures/${name}.json`);

interface Case {
  name: string;
  input: { term: string; other?: string };
  output: { normalized_term: string; match: boolean | null };
}
const CASES = (json('concepts/AliasMatch/v1/fixtures.json') as { cases: Case[] }).cases;

const WORLD: World = {
  scenes: parseScenes(mock('session.listScenes')),
  characters: parseCharacters(mock('campaign.listCharacters')),
  places: parsePlaces(mock('campaign.listPlaces')),
  events: parseEvents(mock('campaign.listEvents')),
  library: parseLibrary(mock('media.listMedia')),
};
const INDEX = buildIndex(WORLD);
const names = (items: readonly { name: string }[]): string[] => items.map((i) => i.name);

describe('AliasMatch@1', () => {
  for (const c of CASES) {
    test(c.name, () => {
      assert.equal(normalize(c.input.term), c.output.normalized_term);
      if (c.input.other !== undefined) assert.equal(same(c.input.term, c.input.other), c.output.match);
    });
  }

  test('a blank term matches nothing, not even another blank one', () => {
    assert.equal(same('', ''), false);
    assert.equal(same(' \t', ' '), false);
  });
});

describe('the search', () => {
  const found = (query: string): string[] => names(search(INDEX, query));

  test('elowen, rodeuse, LA RÔDEUSE: Elowen stays listed each time', () => {
    for (const query of ['elowen', 'rodeuse', 'LA RÔDEUSE', '  la rôdeuse  ']) assert.ok(found(query).includes('Elowen Ventargent'), query);
  });

  test('a blank or whitespace-only search finds nothing, so the panel shows everything again', () => {
    for (const query of ['', ' ', ' \t  ']) {
      assert.deepEqual(found(query), []);
      assert.equal(score(INDEX[0] ?? { name: 'x', aliases: [] }, query), 0);
    }
  });

  test('the aliases are searched as well as the names', () => {
    assert.ok(found('le sanglier').includes('La Taverne du Sanglier'));
    assert.deepEqual(names(search(INDEX.filter((i) => i.type === 'place'), 'le sanglier')), ['La Taverne du Sanglier']);
  });

  test('a word a letter or two off still finds (approximate)', () => {
    assert.ok(found('rodeus').includes('Elowen Ventargent'));
    assert.ok(found('elowan').includes('Elowen Ventargent'));
    assert.ok(found('ventagent elowen').includes('Elowen Ventargent'));
  });

  test('a word too far off finds nothing', () => {
    assert.deepEqual(found('zzzzzz'), []);
    assert.deepEqual(found('elowen zzzzzz'), []);
  });

  test('the exact matches come before the approximate ones', () => {
    const items = [
      { type: 'place' as const, id: 'a', name: 'Rodeuse', aliases: [] },
      { type: 'place' as const, id: 'b', name: 'Rodeur', aliases: [] },
    ];
    assert.deepEqual(names(search(items, 'rodeuse')), ['Rodeuse', 'Rodeur']);
  });

  test('a letter that does not decompose is not folded: coeur is not cœur', () => {
    const items = [{ type: 'event' as const, id: 'c', name: 'Cœur de cendre', aliases: [] }];
    assert.equal(score(items[0] as (typeof items)[0], 'cœur'), 2);
    assert.equal(score(items[0] as (typeof items)[0], 'coeur'), 0);
  });
});

describe('the Capabilities’ answers', () => {
  test('scenes keep the order the Capability gives, and a scene with no battlemap has none', () => {
    assert.deepEqual(WORLD.scenes.map((s) => s.id), ['sc1', 'sc2', 'sc3']);
    assert.equal(WORLD.scenes[0]?.battlemap, null);
    assert.equal(WORLD.scenes[2]?.battlemap, 'bmi-gue-s4');
  });

  test('anything but the expected shapes is read as nothing', () => {
    assert.deepEqual(parseScenes({ nope: 1 }), []);
    assert.deepEqual(parseCharacters([{ name: 'no id' }, 3, null]), []);
    assert.deepEqual(parseLibrary(null), { battlemaps: [], illustrations: [] });
    assert.deepEqual(parseScenes([{ id: 'x' }]), [{ id: 'x', title: '', places: [], characters: [], events: [], media: [], battlemap: null }]);
  });

  test('a character keeps its kind, a place has none', () => {
    assert.equal(WORLD.characters.find((c) => c.id === 'elowen')?.kind, 'pj');
    assert.equal(WORLD.places[0]?.kind, undefined);
    assert.deepEqual(parseCharacters([{ id: 'x', name: 'X', kind: 'dragon' }]), [{ type: 'character', id: 'x', name: 'X', aliases: [] }]);
  });
});

describe('the groups', () => {
  test('the campaign by type: PJ, PNJ, lieux, événements, médias', () => {
    const groups = campaignGroups(INDEX);
    assert.deepEqual(groups.map((g) => g.label), ['PJ', 'PNJ', 'Lieux', 'Événements', 'Médias']);
    assert.equal(groups[0]?.items.length, 4);
    assert.ok(groups[1]?.items.every((i) => i.kind === 'pnj'));
    assert.deepEqual(groups[4]?.items.map((i) => i.type), ['battlemap', 'battlemap', 'illustration', 'illustration', 'illustration', 'illustration', 'illustration']);
  });

  test('a scene lists its places, NPCs (not the PCs), events and media', () => {
    const [first] = WORLD.scenes;
    assert.ok(first);
    const groups = sceneGroups(first, WORLD, INDEX);
    assert.deepEqual(groups.map((g) => g.label), ['Lieux', 'PNJ', 'Événements', 'Médias']);
    assert.deepEqual(names(groups[0]?.items ?? []), ['La Taverne du Sanglier']);
    assert.deepEqual(names(groups[1]?.items ?? []), ['Gaspard Grosmollet', 'Morlox']);
    assert.deepEqual(groups[3]?.items.map((i) => i.id), ['ill-gaspard', 'ill-salle-sanglier']);
  });

  test('a group with nothing in it is not listed', () => {
    const [, second] = WORLD.scenes;
    assert.ok(second);
    assert.deepEqual(sceneGroups(second, WORLD, INDEX).map((g) => g.key), ['lieux', 'evenements', 'medias']);
  });

  test('the battlemap of a scene carries the scene’s instance', () => {
    const third = WORLD.scenes[2];
    assert.ok(third);
    const map = sceneGroups(third, WORLD, INDEX).find((g) => g.key === 'medias')?.items.find((i) => i.type === 'battlemap');
    assert.equal(map?.id, 'bm-gue');
    assert.equal(map?.instance, 'bmi-gue-s4');
    assert.equal(INDEX.find((i) => i.id === 'bm-gue')?.instance, 'bmi-gue-s4');
    assert.equal(INDEX.find((i) => i.id === 'bm-moulin')?.instance, undefined);
  });

  test('an instance with no map linked is still the scene’s battlemap', () => {
    const scene = { id: 'sc9', title: 'T', places: [], characters: [], events: [], media: [], battlemap: 'inst-9' };
    const groups = sceneGroups(scene, { ...WORLD, scenes: [scene] }, INDEX);
    assert.deepEqual(groups.map((g) => g.key), ['medias']);
    assert.equal(groups[0]?.items[0]?.instance, 'inst-9');
  });

  test('a scene that links two battlemaps gives its instance to neither: the instance does not say which it is of', () => {
    const scene = { id: 'sc9', title: 'T', places: [], characters: [], events: [], media: ['bm-gue', 'bm-moulin'], battlemap: 'inst-9' };
    const world = { ...WORLD, scenes: [scene] };
    const index = buildIndex(world);
    const items = sceneGroups(scene, world, index).find((g) => g.key === 'medias')?.items ?? [];
    assert.deepEqual(items.map((i) => [i.id, i.instance]), [['inst-9', 'inst-9'], ['bm-gue', undefined], ['bm-moulin', undefined]]);
    assert.equal(index.find((i) => i.id === 'bm-gue')?.instance, undefined);
  });

  test('an id the campaign lacks is left out', () => {
    const scene = { id: 'sc9', title: 'T', places: ['nowhere'], characters: ['ghost'], events: [], media: ['gone'], battlemap: null };
    assert.deepEqual(sceneGroups(scene, WORLD, INDEX), []);
  });
});

describe('the commands', () => {
  test('a command names its DataCapability and version, the diffusion, and who sends it', () => {
    assert.deepEqual(commandBody('live.showOnPlayerView', { kind: 'battlemap', instance: 'i' }, 's4'), {
      dataCapability: 'live.showOnPlayerView@1',
      target: { aggregate: 'Diffusion', id: 's4' },
      payload: { kind: 'battlemap', instance: 'i' },
      caller: { by: 'dm', from: 'ElementsPanel', roles: [] },
    });
    assert.deepEqual((commandBody('live.showWindowOnPlayerView', {}, '') as { target: unknown }).target, { aggregate: 'Diffusion' });
  });

  test('the server’s answers are read for their status, violations and id', () => {
    assert.deepEqual(settled({ commandId: 'c1', result: { status: 'rejected', violations: ['base-coherent', 3] } }), { status: 'rejected', violations: ['base-coherent'] });
    assert.deepEqual(settled('nope'), { status: '', violations: [] });
    assert.equal(commandId({ commandId: 'c1' }), 'c1');
    assert.equal(commandId({ result: { commandId: 'c2' } }), 'c2');
    assert.equal(commandId(null), '');
    assert.equal(isFinal('applied'), true);
    assert.equal(isFinal('queued'), false);
    assert.equal(reasons(['base-coherent', 'other']), 'cette carte ou cette illustration n’est pas celle de la séance, other');
  });
});
