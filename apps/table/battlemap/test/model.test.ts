import assert from 'node:assert/strict';
import { describe, test } from 'node:test';
import {
  bandLabel,
  bandOf,
  commandBody,
  countOf,
  existingToken,
  hitPointsAfter,
  initiativeOf,
  inserted,
  isFinal,
  numbering,
  nextNumber,
  parseCharacters,
  parseCombat,
  parseInstance,
  parseMap,
  parseProps,
  reasons,
  reordered,
  settled,
  shown,
  tableCommand,
  turnAfter,
  visibleOrder,
  withCondition,
  without,
} from '../src/model.ts';
import type { Character, Combat, Token } from '../src/types.ts';

const token = (over: Partial<Token>): Token => ({
  id: 't', char: 'bandit', kind: 'pnj', name: 'Bandit', num: null, q: 0, r: 0, visible: true, hp: null, hpMax: null, healthBand: null, states: [], dead: false, initials: 'BA', color: '', portrait: false, ...over,
});
const bandit: Character = { id: 'bandit', name: 'Bandit', tokenName: 'Bandit', kind: 'pnj', initials: 'BA', color: 'danger', portrait: false, dexMod: 1 };
const elowen: Character = { id: 'elowen', name: 'Elowen Ventargent', tokenName: 'Elowen', kind: 'pj', initials: 'EV', color: 'success', portrait: true, dexMod: 4 };

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
      caller: { by: 'dm', from: 'Battlemap', roles: ['role:approver'] },
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

const line = (...ids: string[]): Combat => ({ round: 1, current: ids[0] ?? '', order: ids.map((id, i) => ({ id, init: 20 - i, dex: 0 })) });

describe('dead-derived', () => {
  const raw = (over: Record<string, unknown>) => parseInstance({ id: 'i', state: { tokens: [{ id: 't', kind: 'pnj', name: 'Bandit', hp: 11, hpMax: 11, ...over }] } })?.tokens[0];

  test('an NPC is dead iff its current HP is 0, whatever the answer says', () => {
    assert.equal(raw({ hp: 0, dead: false })?.dead, true);
    assert.equal(raw({ hp: 1, dead: true })?.dead, false);
    assert.equal(raw({})?.dead, false);
  });

  test('a PC has no HP to die of: its flag is the answer’s', () => {
    assert.equal(raw({ kind: 'pj', hp: null, hpMax: null })?.dead, false);
  });

  test('the hit points stay within 0 and the maximum', () => {
    assert.equal(hitPointsAfter(11, 11, -4), 7);
    assert.equal(hitPointsAfter(3, 11, -4), 0);
    assert.equal(hitPointsAfter(9, 11, 7), 11);
  });

  test('HealthBand@1 is told from the HP alone', () => {
    assert.deepEqual([bandOf(11, 11), bandOf(8, 11), bandOf(5, 11), bandOf(0, 11)], ['indemne', 'blessé', 'en sang', 'mort']);
  });
});

describe('next-turn', () => {
  const alive = (...names: string[]) => names.map((id) => token({ id }));

  test('the turn goes to the next participant by rank', () => {
    assert.deepEqual(turnAfter(line('a', 'b', 'c'), alive('a', 'b', 'c')), { current: 'b', round: 1 });
  });

  test('a dead participant is skipped', () => {
    const all = [token({ id: 'a' }), token({ id: 'b', dead: true }), token({ id: 'c' })];
    assert.deepEqual(turnAfter(line('a', 'b', 'c'), all), { current: 'c', round: 1 });
  });

  test('the round counts up when the order comes back to its first', () => {
    const combat = { ...line('a', 'b', 'c'), current: 'c' };
    assert.deepEqual(turnAfter(combat, alive('a', 'b', 'c')), { current: 'a', round: 2 });
    const skipping = [token({ id: 'a', dead: true }), token({ id: 'b' }), token({ id: 'c' })];
    assert.deepEqual(turnAfter(combat, skipping), { current: 'b', round: 2 }, 'the first being dead, the lap ends on the next living one');
  });

  test('a lone survivor keeps the turn and the round counts up each lap', () => {
    const all = [token({ id: 'a' }), token({ id: 'b', dead: true })];
    assert.deepEqual(turnAfter(line('a', 'b'), all), { current: 'a', round: 2 });
  });
});

describe('hidden-keeps-rank', () => {
  const all = [token({ id: 'a' }), token({ id: 'boss', visible: false }), token({ id: 'c' })];
  const combat = line('a', 'boss', 'c');

  test('the DM carousel holds the hidden token, the players’ does not', () => {
    assert.deepEqual(visibleOrder(combat, all, 'dm').map((x) => x.token.id), ['a', 'boss', 'c']);
    assert.deepEqual(visibleOrder(combat, all, 'player').map((x) => x.token.id), ['a', 'c']);
  });

  test('a token revealed again shows at its former rank, and the others do not move', () => {
    const revealed = all.map((t) => (t.id === 'boss' ? { ...t, visible: true } : t));
    assert.deepEqual(visibleOrder(combat, revealed, 'player').map((x) => x.token.id), ['a', 'boss', 'c']);
  });

  test('a participant whose token left the map is not drawn', () => {
    assert.deepEqual(visibleOrder(line('a', 'gone'), all, 'dm').map((x) => x.token.id), ['a']);
  });
});

describe('the order of a combat', () => {
  test('a participant moved takes the rank asked, the others keeping their relative order', () => {
    assert.deepEqual(reordered(line('a', 'b', 'c').order, 'a', 2).map((o) => o.id), ['b', 'c', 'a']);
    assert.deepEqual(reordered(line('a', 'b', 'c').order, 'c', 0).map((o) => o.id), ['c', 'a', 'b']);
    assert.deepEqual(reordered(line('a', 'b', 'c').order, 'b', 9).map((o) => o.id), ['a', 'c', 'b'], 'past the end, it goes last');
    assert.deepEqual(reordered(line('a', 'b').order, 'x', 0).map((o) => o.id), ['a', 'b'], 'an unknown participant changes nothing');
  });

  test('a newcomer goes in ahead of those whose initiative is lower, behind a tie', () => {
    const order = [{ id: 'a', init: 18, dex: 0 }, { id: 'b', init: 12, dex: 0 }, { id: 'c', init: 5, dex: 0 }];
    assert.deepEqual(inserted(order, { id: 'n', init: 12, dex: 0 }).map((o) => o.id), ['a', 'b', 'n', 'c']);
    assert.deepEqual(inserted(order, { id: 'n', init: 1, dex: 0 }).map((o) => o.id), ['a', 'b', 'c', 'n']);
    assert.deepEqual(inserted(order, { id: 'n', init: 30, dex: 0 }).map((o) => o.id), ['n', 'a', 'b', 'c']);
  });

  test('an NPC’s initiative is its d20 plus its DEX modifier', () => {
    assert.equal(initiativeOf(13, 1), 14);
    assert.equal(initiativeOf(2, -1), 1);
  });

  test('the combat is read from the instance, or is none', () => {
    const combat = parseCombat({ round: 2, current: 'b', order: [{ id: 'a', init: 18, dex: 2 }, { id: 'b', init: 12 }, { nope: true }] });
    assert.deepEqual(combat, { round: 2, current: 'b', order: [{ id: 'a', init: 18, dex: 2 }, { id: 'b', init: 12, dex: 0 }] });
    assert.equal(parseCombat(null), null);
    assert.equal(parseCombat({ round: 1, order: [] }), null);
    assert.equal(parseInstance({ id: 'i', state: { tokens: [], combat: { round: 1, current: 'a', order: [{ id: 'a', init: 3 }] } } })?.combat?.current, 'a');
  });

  test('the DEX modifier of a sheet is floor((score - 10) / 2)', () => {
    const read = parseCharacters([{ id: 'a', stats: { DEX: 12 } }, { id: 'b', stats: { DEX: 8 } }, { id: 'c', stats: { DEX: 9 } }, { id: 'd' }]);
    assert.deepEqual(['a', 'b', 'c', 'd'].map((id) => read[id]?.dexMod), [1, -1, -1, 0]);
  });
});

describe('a participant leaving the order', () => {
  const alive = (...names: string[]) => names.map((id) => token({ id }));

  test('the turn passes on to the next living participant of those left, and the leaver is never current', () => {
    const left = without({ ...line('a', 'b', 'c'), current: 'b' }, alive('a', 'b', 'c'), 'b');
    assert.deepEqual([left?.current, left?.round, left?.order.map((o) => o.id)], ['c', 1, ['a', 'c']]);
  });

  test('someone else leaving changes neither the turn nor the round', () => {
    const left = without(line('a', 'b', 'c'), alive('a', 'b', 'c'), 'c');
    assert.deepEqual([left?.current, left?.round, left?.order.map((o) => o.id)], ['a', 1, ['a', 'b']]);
  });

  test('the last one to hold the turn, the others being dead, hands it to the first of those left without a lap', () => {
    const all = [token({ id: 'a' }), token({ id: 'b', dead: true })];
    const left = without(line('a', 'b'), all, 'a');
    assert.deepEqual([left?.current, left?.round, left?.order.map((o) => o.id)], ['b', 1, ['b']]);
  });

  test('the round counts up when the turn laps over the leaver’s place', () => {
    const left = without({ ...line('a', 'b', 'c'), current: 'c' }, alive('a', 'b', 'c'), 'c');
    assert.deepEqual([left?.current, left?.round], ['a', 2]);
  });

  test('the combat ends with its last participant', () => {
    assert.equal(without(line('a'), alive('a'), 'a'), null);
  });

  test('a newcomer ties on initiative behind a higher DEX and ahead of a lower one', () => {
    const order = [{ id: 'a', init: 12, dex: 3 }, { id: 'b', init: 12, dex: 0 }];
    assert.deepEqual(inserted(order, { id: 'n', init: 12, dex: 1 }).map((o) => o.id), ['a', 'n', 'b']);
  });

  test('a view that cannot name the turn says none, and the first is not made up', () => {
    assert.equal(parseCombat({ round: 1, current: '', order: [{ id: 'a', init: 3 }] })?.current, '');
  });
});
