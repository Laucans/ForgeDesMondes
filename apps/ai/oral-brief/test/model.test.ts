import assert from 'node:assert/strict';
import { test } from 'node:test';
import { commandBody, commandId, isFinal, lastRunLabel, parseBrief, reasons, settled } from '../src/model.ts';

test('a Capability answer with scenes out of order is read by order, an element with no id dropped', () => {
  const brief = parseBrief({
    lastRun: '2026-10-14T19:31:00',
    duration: '04:12',
    transcript: ['une', '', 7, 'deux'],
    result: {
      scenes: [
        { order: 2, title: 'B', links: ['x', 3, ''] },
        { order: 1, title: 'A', links: [] },
        { order: 3, title: '' },
      ],
      recognized: [{ id: 'm', type: 'pnj', name: 'Morlox', heard: null, updated: true, change: 'Notes' }, { name: 'sans id' }],
      missing: [{ id: 'tc', type: 'lieu', name: 'Le Gué' }, null],
    },
  });
  assert.deepEqual(brief.transcript, ['une', 'deux']);
  assert.deepEqual(brief.scenes.map((s) => s.title), ['A', 'B']);
  assert.deepEqual(brief.scenes[1]?.links, ['x']);
  assert.deepEqual(brief.recognized, [{ id: 'm', type: 'pnj', name: 'Morlox', heard: null, updated: true, change: 'Notes' }]);
  assert.deepEqual(brief.missing, [{ id: 'tc', type: 'lieu', name: 'Le Gué' }]);
});

test('an answer that is not a brief reads as no brief', () => {
  for (const data of [null, undefined, 'x', [], { result: 'x' }]) {
    assert.deepEqual(parseBrief(data), { lastRun: null, duration: null, transcript: [], scenes: [], recognized: [], missing: [] });
  }
});

test('updated is true only when the answer says true', () => {
  const brief = parseBrief({ result: { recognized: [{ id: 'a', updated: 'yes' }, { id: 'b', updated: true }] } });
  assert.deepEqual(brief.recognized.map((r) => r.updated), [false, true]);
});

test('the last run is dated in UTC when it carries no zone, and says nothing before a first brief', () => {
  assert.equal(lastRunLabel(null, '04:12'), '');
  assert.match(lastRunLabel('2026-10-14T19:31:00', '04:12'), /^Dernier brief : .*octobre.* · 04:12$/);
  assert.equal(lastRunLabel('2026-10-14T19:31:00', null).includes('·'), false);
  const utc = new Date('2026-10-14T19:31:00Z').toLocaleString('fr-FR', { day: 'numeric', month: 'long', hour: '2-digit', minute: '2-digit' });
  assert.equal(lastRunLabel('2026-10-14T19:31:00', null), `Dernier brief : ${utc}`);
  assert.equal(lastRunLabel('2026-10-14T19:31:00Z', null), lastRunLabel('2026-10-14T19:31:00', null));
  assert.equal(lastRunLabel('pas une date', null), 'Dernier brief : pas une date');
});

test('the command names the DataCapability and its version, the DM as the approver, and the Micro-UI', () => {
  assert.deepEqual(commandBody('ai.analyzeBrief', { session: 's4', text: 'hop' }), {
    dataCapability: 'ai.analyzeBrief@1',
    payload: { session: 's4', text: 'hop' },
    caller: { by: 'dm', from: 'OralBrief', roles: ['role:approver'] },
  });
});

test('a command is followed by its answer: status, violations in words, id', () => {
  assert.deepEqual(settled({ result: { status: 'rejected', violations: ['text-only-no-audio', 3] } }), { status: 'rejected', violations: ['text-only-no-audio'] });
  assert.deepEqual(settled(null), { status: '', violations: [] });
  assert.equal(reasons(['text-only-no-audio', 'autre']), 'seul du texte est accepté, autre');
  assert.equal(commandId({ commandId: 'c1' }), 'c1');
  assert.equal(commandId({ result: { commandId: 'c2' } }), 'c2');
  assert.equal(commandId('x'), '');
  assert.deepEqual(['applied', 'rejected', 'cancelled', 'expired', 'queued', ''].map(isFinal), [true, true, true, true, false, false]);
});
