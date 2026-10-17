import assert from 'node:assert/strict';
import { test } from 'node:test';
import { Context } from './context.js';

test('a $ctx prop reads the context; the others are handed as they are', () => {
  const context = new Context();
  context.set('campaign', 'c1');
  assert.deepEqual(context.resolve({ campaign: '$ctx.campaign', scene: '$ctx.scene', fixed: 'a', n: 2 }), { campaign: 'c1', scene: undefined, fixed: 'a', n: 2 });
});

test('the ones that follow the context are told of a change, once, and stop when asked', () => {
  const context = new Context();
  const told: Array<[string, unknown]> = [];
  const stop = context.on((name, value) => told.push([name, value]));
  context.set('scene', 's1');
  context.set('scene', 's1');
  stop();
  context.set('scene', 's2');
  assert.deepEqual(told, [['scene', 's1']]);
});

test('a follower that fails does not keep the others from a change', () => {
  const context = new Context();
  const told: string[] = [];
  context.on(() => {
    throw new Error('boom');
  });
  context.on((name) => told.push(name));
  const error = console.error;
  console.error = () => undefined;
  try {
    context.set('scene', 's1');
  } finally {
    console.error = error;
  }
  assert.deepEqual(told, ['scene']);
  assert.equal(context.get('scene'), 's1');
});

test('names the context a set of props follows', () => {
  assert.deepEqual(new Context().follows({ a: '$ctx.x', b: 'y', c: '$ctx.z', d: 1 }), ['x', 'z']);
});
