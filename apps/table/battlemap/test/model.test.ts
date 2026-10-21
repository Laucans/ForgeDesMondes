import assert from 'node:assert/strict';
import { describe, test } from 'node:test';
import {
  bandLabel,
  commandBody,
  countOf,
  existingToken,
  isFinal,
  numbering,
  nextNumber,
  parseCharacters,
  parseInstance,
  parseMap,
  parseProps,
  reasons,
  settled,
  shown,
  tableCommand,
  withCondition,
} from '../src/model.ts';
import type { Character, Token } from '../src/types.ts';

const token = (over: Partial<Token>): Token => ({
  id: 't', char: 'bandit', kind: 'pnj', name: 'Bandit', num: null, q: 0, r: 0, visible: true, hp: null, hpMax: null, healthBand: null, states: [], dead: false, initials: 'BA', color: '', portrait: false, ...over,
});
const bandit: Character = { id: 'bandit', name: 'Bandit', tokenName: 'Bandit', kind: 'pnj', initials: 'BA', color: 'danger', portrait: false };
const elowen: Character = { id: 'elowen', name: 'Elowen Ventargent', tokenName: 'Elowen', kind: 'pj', initials: 'EV', color: 'success', portrait: true };

describe('players-see-band-only', () => {
  const all = [token({ id: 'a' }), token({ id: 'b', visible: false }), token({ id: 'c' })];

  test('a hidden token does not exist for the players, the DM sees it', () => {
    assert.deepEqual(shown(all, 'player').map((t) => t.id), ['a', 'c']);
    assert.deepEqual(shown(all, 'dm').map((t) => t.id), ['a', 'b', 'c']);
    assert.deepEqual(shown(all, 'prep').map((t) => t.id), ['a', 'b', 'c']);
  });

  test('the band is the only health a player reads', () => {
    assert.equal(bandLabel('en sang'), 'En sang');
    assert.equal(bandLabel('mort'), 'Mort');
    assert.equal(bandLabel(null), '');
  });

  test('the band is read under the name the answer gives it', () => {
    const answer = (extra: Record<string, unknown>) => parseInstance({ id: 'i', state: { tokens: [{ id: 't', kind: 'pnj', ...extra }] } })?.tokens[0]?.healthBand;
    assert.equal(answer({ healthBand: 'blessé' }), 'blessé');
    assert.equal(answer({ health_band: 'indemne' }), 'indemne');
    assert.equal(answer({}), null);
  });
});

describe('multi-drop-numbering', () => {
  test('a drop is numbered after the numbers already taken for the character', () => {
    const tokens = [token({ num: 1 }), token({ id: 'u', num: 2 }), token({ id: 'v', char: 'morlox', num: 7 })];
    assert.equal(nextNumber(tokens, 'bandit'), 3);
    assert.equal(nextNumber(tokens, 'morlox'), 8);
    assert.equal(nextNumber(tokens, 'vesna'), 1);
  });

  test('the dialog says which numbers the drop will take', () => {
    const tokens = [token({ num: 1 }), token({ id: 'u', num: 2 })];
    assert.equal(numbering(tokens, bandit, 3), 'Numérotées de Bandit 3 à Bandit 5, renommables');
    assert.equal(numbering(tokens, bandit, 1), 'Numérotée Bandit 3, renommable');
    assert.equal(numbering([], bandit, 1), 'Un seul token, renommable');
    assert.equal(numbering([], bandit, 2), 'Numérotées de Bandit 1 à Bandit 2, renommables');
  });

  test('how many is a whole number from 1 to 12', () => {
    assert.equal(countOf('3'), 3);
    assert.equal(countOf('0'), 1);
    assert.equal(countOf('-4'), 1);
    assert.equal(countOf('99'), 12);
    assert.equal(countOf('2.9'), 2);
    assert.equal(countOf(''), 1);
    assert.equal(countOf('abc'), 1);
  });

  test('a PC has one token on a map: it is found, and an NPC never is', () => {
    const tokens = [token({ id: 'p', char: 'elowen', kind: 'pj' })];
    assert.equal(existingToken(tokens, elowen)?.id, 'p');
    assert.equal(existingToken([token({ num: 1 })], bandit), undefined);
    assert.equal(existingToken([], elowen), undefined);
  });
});

describe('what the Capabilities answer', () => {
  test('the instance keeps its tokens whether they sit under state or at the top', () => {
    const under = parseInstance({ id: 'i', battlemap: 'm', label: 'L', roster: ['a', 3], state: { tokens: [{ id: 't', name: 'Kael', q: 1, r: 2, visible: false, states: ['À terre', 4] }] } });
    assert.equal(under?.battlemap, 'm');
    assert.deepEqual(under?.roster, ['a']);
    assert.deepEqual(under?.tokens[0], { id: 't', char: '', kind: 'pj', name: 'Kael', num: null, q: 1, r: 2, visible: false, hp: null, hpMax: null, healthBand: null, states: ['À terre'], dead: false, initials: 'KA', color: '', portrait: false });
    assert.equal(parseInstance({ id: 'i', tokens: [{ id: 'x' }] })?.tokens.length, 1);
    assert.equal(parseInstance(null), null);
    assert.equal(parseInstance('nope'), null);
  });

  test('the map keeps a grid per HexGrid@1 and only the shapes a map is drawn with', () => {
    const map = parseMap({
      id: 'bm', title: 'Gué', width: 960, height: 640, grid: { size: 30, ox: 12, oy: 8, orient: 'flat', color: 'accent', opacity: 0.4 },
      terrain: [{ tag: 'rect', width: 10, onload: 'x()' }, { tag: 'script', d: 'x' }, { tag: 'path', d: 'M0 0', class: 'hm-road' }, 'junk'],
    });
    assert.deepEqual(map?.grid, { size: 30, ox: 12, oy: 8, orient: 'flat', color: 'accent', opacity: 0.4 });
    assert.deepEqual(map?.terrain, [{ tag: 'rect', attrs: { width: '10' } }, { tag: 'path', attrs: { d: 'M0 0', class: 'hm-road' } }]);
    assert.equal(parseMap(null), null);
  });

  test('a grid that does not hold falls back to a drawable one', () => {
    const map = parseMap({ id: 'bm', grid: { size: 0, orient: 'square', opacity: 40 } });
    assert.equal(map?.grid.size, 32);
    assert.equal(map?.grid.orient, 'pointy');
    assert.equal(map?.grid.opacity, 1);
    assert.equal(parseMap({ id: 'bm' })?.width, 800);
  });

  test('the characters are listed by id, a token named by the first word of the name', () => {
    const list = parseCharacters([{ id: 'elowen', name: 'Elowen Ventargent', kind: 'pj', initials: 'EV', color: 'success', portrait: true }, { id: 'bandit', name: 'Bandit', tokenName: 'Brigand', kind: 'pnj' }, { name: 'no id' }, 3]);
    assert.deepEqual(Object.keys(list), ['elowen', 'bandit']);
    assert.equal(list.elowen?.tokenName, 'Elowen');
    assert.equal(list.bandit?.tokenName, 'Brigand');
    assert.deepEqual(parseCharacters({}), {});
  });

  test('the props default to the DM view of nothing', () => {
    assert.deepEqual(parseProps({}), { instance: '', view: 'dm', pilot: false, fill: false, campaign: '' });
    assert.deepEqual(parseProps({ instance: 'i', view: 'player', pilot: true, fill: true, campaign: 'c' }), { instance: 'i', view: 'player', pilot: true, fill: true, campaign: 'c' });
    assert.equal(parseProps({ view: 'prep' }).view, 'prep');
    assert.equal(parseProps({ view: 'other' }).view, 'player', 'a view nobody knows is the players’, never the DM’s');
  });
});

describe('commands', () => {
  test('a table command names its DataCapability and version, and targets the instance', () => {
    assert.deepEqual(tableCommand('table.moveToken', 'bmi-1', { token: 't', q: 2, r: 3 }), {
      dataCapability: 'table.moveToken@1',
      target: { aggregate: 'BattlemapInstance', id: 'bmi-1' },
      payload: { token: 't', q: 2, r: 3 },
      caller: { by: 'dm', from: 'Battlemap', roles: [] },
    });
  });

  test('a command without an id targets its aggregate alone', () => {
    assert.deepEqual((commandBody('live.showOnPlayerView', 'Diffusion', undefined, {}) as { target: unknown }).target, { aggregate: 'Diffusion' });
  });

  test('a condition is added, then removed', () => {
    assert.deepEqual(withCondition([], 'À terre'), ['À terre']);
    assert.deepEqual(withCondition(['À terre', 'Effrayé'], 'À terre'), ['Effrayé']);
  });

  test('the answer of the server says where a command stands', () => {
    assert.deepEqual(settled({ commandId: 'c', result: { status: 'rejected', violations: ['token-roster', 4] } }), { status: 'rejected', violations: ['token-roster'] });
    assert.deepEqual(settled(null), { status: '', violations: [] });
    assert.ok(isFinal('applied') && isFinal('rejected') && !isFinal('queued') && !isFinal('awaiting_confirmation'));
    assert.equal(reasons(['base-coherent', 'other']), 'cette carte n’est pas celle de la séance, other');
  });
});
