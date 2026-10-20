import assert from 'node:assert/strict';
import { test } from 'node:test';
import { hasPrevious, originBadge, parseHistory, reasons, restoreBody, seenUpTo, settled, sourceLabel, topVersion, unseen } from '../src/model.ts';

const version = (v: number, extra: Record<string, unknown> = {}) => ({ v, date: '2026-10-14T19:32:00', origin: 'ia', source: 'ia', restoredFrom: null, changes: [], ...extra });

test('a history is read newest first, however the Capability orders it', () => {
  const history = parseHistory({ name: 'Morlox', versions: [version(1), version(3), version(2)] });
  assert.deepEqual(history.versions.map((v) => v.v), [3, 2, 1]);
  assert.equal(topVersion(history), 3);
});

test('an unknown element (null) and a malformed answer are an empty history, not an error', () => {
  for (const data of [null, undefined, 'x', [], { versions: 'x' }, { versions: [null, { v: 'one' }] }]) {
    const history = parseHistory(data);
    assert.deepEqual(history.versions, []);
    assert.equal(topVersion(history), 0);
  }
});

test('a change shows any value as text: nothing as an empty string, a structure as JSON', () => {
  const history = parseHistory({ name: 'x', versions: [version(2, { changes: [{ field: 'Sorts', old: null, new: ['a', 'b'] }, { field: 'PV', old: 18, new: 'x' }, { nofield: true }] })] });
  assert.deepEqual(history.versions[0]?.changes, [
    { field: 'Sorts', old: '', new: '["a","b"]' },
    { field: 'PV', old: '18', new: 'x' },
  ]);
});

test('what the DM has not seen: after the oldest version the Capability calls unseen, if someone else made it', () => {
  const history = parseHistory({ name: 'x', versions: [version(4, { seen: false }), version(3, { origin: 'manuelle' }), version(2), version(1)] });
  assert.equal(seenUpTo(history), 3);
  assert.deepEqual(unseen(history, 3).map((v) => v.v), [4]);
  // The DM's own edit after that is not news to them.
  const own = parseHistory({ name: 'x', versions: [version(5, { origin: 'manuelle' }), ...history.versions.map((v) => ({ ...v, date: v.date }))] });
  assert.deepEqual(unseen(own, 3).map((v) => v.v), [4]);
  // No version flagged unseen: the DM is up to date.
  assert.equal(seenUpTo(parseHistory({ name: 'x', versions: [version(2), version(1)] })), 2);
});

test('a source is worded; a restoration names the version it put back; any other text stays as it is', () => {
  const read = (extra: Record<string, unknown>) => parseHistory({ name: 'x', versions: [version(2, extra)] }).versions[0]!;
  assert.equal(sourceLabel(read({ source: 'restauration', restoredFrom: 2 })), 'Retour à la v2');
  assert.equal(sourceLabel(read({ source: 'restauration' })), 'Retour à une version antérieure');
  assert.equal(sourceLabel(read({ source: 'dm' })), 'Fiche modifiée à la main');
  assert.equal(sourceLabel(read({ source: 'brief' })), 'Brief oral');
  assert.equal(sourceLabel(read({ source: 'Brief oral — séance 4' })), 'Brief oral — séance 4');
});

test('an origin is badged with the kit Badge’s tone', () => {
  assert.deepEqual(originBadge('ia'), { label: 'IA', tone: 'accent' });
  assert.deepEqual(originBadge('manuelle'), { label: 'Manuelle', tone: 'primary' });
  assert.deepEqual(originBadge('dndbeyond'), { label: 'D&D Beyond', tone: 'success' });
  assert.deepEqual(originBadge('autre'), { label: 'autre', tone: null });
});

test('going back names the element and one version, and no field; undoing changes names the version before them and which', () => {
  assert.deepEqual(restoreBody('campaign.restoreEntityVersion', 'morlox', { version: 2 }, 'k1'), {
    dataCapability: 'campaign.restoreEntityVersion@1',
    target: { aggregate: 'Entity', id: 'morlox' },
    payload: { version: 2 },
    idempotencyKey: 'k1',
    caller: { by: 'dm', from: 'VersionHistory', roles: [] },
  });
  assert.deepEqual((restoreBody('campaign.restoreEntityVersion', 'morlox', { version: 3, changes: [0, 2] }, 'k2') as { payload: unknown }).payload, { version: 3, changes: [0, 2] });
});

test('only a version with another before it can have its changes undone', () => {
  const history = parseHistory({ name: 'x', versions: [version(2), version(1)] });
  assert.equal(hasPrevious(history, history.versions[0]!), true);
  assert.equal(hasPrevious(history, history.versions[1]!), false);
});

test('a command’s answer is read as the server gives it', () => {
  assert.deepEqual(settled({ result: { status: 'rejected', violations: ['alias-unique-in-campaign', 3] } }), { status: 'rejected', violations: ['alias-unique-in-campaign'] });
  assert.deepEqual(settled(null), { status: '', violations: [] });
  assert.match(reasons(['alias-unique-in-campaign', 'other']), /autre élément, other$/);
});
