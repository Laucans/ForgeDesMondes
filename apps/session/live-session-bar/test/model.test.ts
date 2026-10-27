import assert from 'node:assert/strict';
import { test } from 'node:test';
import { battlemapOf, commandBody, durationSince, parseDiffusion, parseLive, parseRecap, parseScenes, reasons, settled, showing, withScenes } from '../src/model.ts';

test('the session is read flat or under `session`, with its campaign and microphone; its scenes are another read’s', () => {
  const flat = parseLive({ id: 's4', campaign: 'cendres-valombre', order: 4, title: 'L’embuscade', startedAt: '20:02', currentScene: 2 });
  assert.deepEqual(flat, { id: 's4', campaign: 'cendres-valombre', order: 4, title: 'L’embuscade', startedAt: '20:02', micOn: null, scenes: [], currentScene: 2 });
  const nested = parseLive({ session: { id: 's4', order: 4, title: 'L’embuscade', startedAt: '20:02', present: [] }, microphone: { on: true } });
  assert.equal(nested?.id, 's4');
  assert.equal(nested?.campaign, '');
  assert.equal(nested?.micOn, true);
});

test('an answer with no session is not an exception, and a current scene out of range falls back to the first', () => {
  assert.equal(parseLive(undefined), null);
  assert.equal(parseLive({ session: {} }), null);
  const live = parseLive({ id: 's1', currentScene: 7 });
  assert.ok(live);
  assert.equal(withScenes(live, [{ id: 'a', order: 1, title: 'A', battlemap: null }]).currentScene, 0);
});

test('the current scene is a whole number, the first otherwise', () => {
  assert.equal(parseLive({ id: 's1', currentScene: 1.5 })?.currentScene, 0);
  assert.equal(parseLive({ id: 's1', currentScene: -1 })?.currentScene, 0);
  assert.equal(parseLive({ id: 's1', currentScene: 2 })?.currentScene, 2);
});

test('the scenes keep their battlemap, null when they have none, and the Battlemap tab opens the live scene’s else the first', () => {
  const scenes = parseScenes([{ id: 'a', order: 1, title: 'A', battlemap: null }, { id: 'b', order: 2, title: 'B', battlemap: 'bmi-b' }, { id: 'c', order: 3, title: 'C' }, 'junk']);
  assert.deepEqual(scenes.map((s) => s.battlemap), [null, 'bmi-b', null]);
  const live = withScenes(parseLive({ id: 's1', currentScene: 1 }) as NonNullable<ReturnType<typeof parseLive>>, scenes);
  assert.equal(battlemapOf(live), 'bmi-b');
  assert.equal(battlemapOf({ ...live, currentScene: 0 }), 'bmi-b');
  assert.equal(battlemapOf({ ...live, scenes: [] }), undefined);
  assert.deepEqual(parseScenes(undefined), []);
});

test('the players see the campaign cover unless the answer says otherwise', () => {
  assert.deepEqual(parseDiffusion(undefined), { base: { kind: 'cover', title: '' }, window: null });
  assert.deepEqual(parseDiffusion({ base: { kind: 'nonsense' } }).base.kind, 'cover');
  const both = parseDiffusion({ base: { kind: 'battlemap', title: 'Le Gué' }, window: { id: 'i', title: 'Sceau' } });
  assert.deepEqual(showing(both), { base: 'la battlemap « Le Gué »', window: 'fenêtre « Sceau »' });
  assert.deepEqual(showing(parseDiffusion({})), { base: 'la couverture de la campagne', window: null });
});

test('the duration runs from the start to the close, over midnight when need be', () => {
  const at = (h: number, m: number): Date => new Date(2026, 9, 17, h, m);
  assert.equal(durationSince('20:02', at(23, 29)), '3 h 27 min');
  assert.equal(durationSince('20:02', at(20, 29)), '27 min');
  assert.equal(durationSince('23:30', at(1, 5)), '1 h 35 min');
  assert.equal(durationSince('', at(1, 5)), '');
});

test('the recap counts the journal and reads the model whichever form `summaryMeta` takes', () => {
  const journal = { journal: [{ t: '20:04', text: 'a' }, { t: '20:06', text: 'b' }] };
  const recap = parseRecap(journal, { summary: 'Résumé', summaryMeta: 'par qwen', mentions: [{ id: 'm', name: 'Morlox' }], threads: ['fil'] }, '1 h 02 min');
  assert.deepEqual(recap, { journalLines: 2, duration: '1 h 02 min', summaryModel: 'qwen', summary: 'Résumé', mentions: ['Morlox'], threads: ['fil'] });
  assert.equal(parseRecap(journal, { summaryMeta: { model: 'qwen' } }, '').summaryModel, 'qwen');
  // The sentence the fixtures use names the model; the date it was edited is not part of it.
  const sentence = 'Généré à la clôture par qwen2.5-14b-instruct (opencode maison), retouché le 4 oct. 2026';
  assert.equal(parseRecap(journal, { summaryMeta: sentence }, '').summaryModel, 'qwen2.5-14b-instruct (opencode maison)');
  assert.equal(parseRecap(journal, { summaryMeta: 'inconnu' }, '').summaryModel, 'inconnu');
  assert.equal(parseRecap(undefined, undefined, '').journalLines, 0);
});

test('a command names its DataCapability by identifier and version; the close targets its session', () => {
  assert.deepEqual(commandBody('live.showCover', 'k1'), { dataCapability: 'live.showCover@1', payload: {}, idempotencyKey: 'k1', caller: { by: 'dm', from: 'LiveSessionBar', roles: [] } });
  const close = commandBody('session.closeSession', 'k2', { aggregate: 'Session', id: 's4' }) as Record<string, unknown>;
  assert.deepEqual(close.target, { aggregate: 'Session', id: 's4' });
  assert.equal(close.idempotencyKey, 'k2');
  assert.deepEqual(settled({ result: { status: 'rejected', violations: ['status-lifecycle'] } }), { status: 'rejected', violations: ['status-lifecycle'] });
  assert.equal(reasons(['status-lifecycle', 'x']), 'cette séance est déjà close, x');
});
