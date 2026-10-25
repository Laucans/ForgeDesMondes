import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderScreen } from './page.js';
import type { Screen } from './screens.js';

const dmView: Screen = {
  screen: 'dm-view',
  generatedFrom: 'a goal',
  version: 1,
  layout: [
    { microUi: 'LiveSessionBar', props: {} },
    { microUi: 'ElementsPanel', props: {} },
    { microUi: 'TableSheet', props: {} },
    { microUi: 'Battlemap', props: { instance: '$ctx.battlemap', view: 'dm' } },
    { microUi: 'LiveSoundbox', props: {} },
    { microUi: 'SuggestionPill', props: {} },
  ],
};

const at = (html: string, needle: string): number => {
  const found = html.indexOf(needle);
  assert.notEqual(found, -1, `${needle} is on the page`);
  return found;
};

// AC2, AC10, AC12: the DM view puts the session bar on top, the panel in the left column, the sheet, battlemap and soundbox behind three tabs, and the pill over the page
test('the DM view is framed: header, left column, centre tabs, overlay', () => {
  const html = renderScreen(dmView);
  const bar = at(html, 'data-component="LiveSessionBar"');
  const left = at(html, 'class="shell-frame__left"');
  const panel = at(html, 'data-component="ElementsPanel"');
  const centre = at(html, 'class="shell-frame__centre"');
  const pill = at(html, 'data-component="SuggestionPill"');
  assert.ok(bar < left && left < panel && panel < centre && centre < pill);
  assert.ok(at(html, '</aside>') > panel, 'the panel is inside the left column');
  assert.ok(at(html, '</main>') < pill, 'the pill is outside the centre, so it takes no room in it');
  assert.ok(html.indexOf('data-component="SuggestionPill"') === html.lastIndexOf('data-component="SuggestionPill"'));
});

// AC2, AC11: the tabs carry the names the rules click, the sheet first, and each opens on its event
test('the centre tabs are Fiche, Battlemap and Soundbox, the sheet shown, the others hidden', () => {
  const html = renderScreen(dmView);
  const tabs = [...html.matchAll(/<button type="button" role="tab"[^>]*>([^<]*)<\/button>/g)].map((m) => m[1]);
  assert.deepEqual(tabs, ['Fiche', 'Battlemap', 'Soundbox']);
  assert.match(html, /aria-selected="true" data-opens="fdm:open">Fiche/);
  assert.match(html, /aria-selected="false" data-opens="fdm:open-map" data-sets="battlemap:id">Battlemap/);
  assert.equal([...html.matchAll(/role="tabpanel"[^>]*hidden/g)].length, 2);
  const panels = [...html.matchAll(/role="tabpanel"[^>]*>(<div data-component="(\w+)")/g)].map((m) => m[2]);
  assert.deepEqual(panels, ['TableSheet', 'Battlemap', 'LiveSoundbox']);
});

// AC1: the frozen props reach the page as written, `$ctx.battlemap` unresolved for the composer to follow
test('the Battlemap keeps its context reference on the page', () => {
  assert.match(renderScreen(dmView), /data-component="Battlemap" data-props="\{&quot;instance&quot;:&quot;\$ctx\.battlemap&quot;,&quot;view&quot;:&quot;dm&quot;\}"/);
});

test('a Micro-UI the frame does not name still closes the page, in the screen order', () => {
  const html = renderScreen({ ...dmView, layout: [...dmView.layout, { microUi: 'Extra', props: {} }] });
  assert.ok(at(html, 'data-component="Extra"') > at(html, 'data-component="SuggestionPill"'));
});

test('a screen with no frame stays a column', () => {
  const html = renderScreen({ screen: 'player-view', generatedFrom: 'a goal', version: 1, layout: [{ microUi: 'PlayerStage', props: {} }] });
  assert.match(html, /class="shell-screen"/);
  assert.doesNotMatch(html, /shell-frame/);
});

test('the same screen gives the same page, byte for byte', () => {
  assert.equal(renderScreen(dmView), renderScreen(dmView));
});
