import assert from 'node:assert/strict';
import { test } from 'node:test';
import { tagChips, tagId } from '../src/render.ts';

test('two tags that differ only by an accent or a space have two ids', () => {
  const ids = ['intérieur', 'interieur', 'int rieur', 'int-rieur', 'combat'].map(tagId);
  assert.equal(new Set(ids).size, ids.length);
  assert.equal(tagId('combat'), 'combat');
});

test('a chip is on when its tag is chosen, and says so to a screen reader', () => {
  const html = tagChips(['bois', 'combat'], new Set(['combat']));
  assert.match(html, /class="chip" aria-pressed="false" data-tag="bois"/);
  assert.match(html, /class="chip is-on" aria-pressed="true" data-tag="combat" id="soundbox-tag-combat">#combat</);
});
