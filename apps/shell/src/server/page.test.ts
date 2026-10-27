import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
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

// AC2, AC10: the DM view puts the session bar on top, the panel in the left column, the sheet, battlemap and soundbox behind three tabs, and the pill over the page
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

const DM_SCREENS = ['campagnes', 'campagne', 'fiche-entite', 'preparation-seance', 'medias', 'soundbox', 'parametres-ia', 'dm-view'];
const asDm = (name: string): Screen => ({ screen: name, generatedFrom: 'a goal', version: 1, surface: 'dm', layout: [{ microUi: 'Anything', props: {} }] });
const menuOf = (html: string): string => /<header class="shell-site-header">.*?<\/header><\/header>/s.exec(html)?.[0] ?? '';
const linksOf = (html: string): Array<{ label: string; href: string; current: boolean }> =>
  [...menuOf(html).matchAll(/<a href="([^"]*)"( aria-current="page")?>([^<]*)<\/a>/g)].map((m) => ({ label: m[3] ?? '', href: m[1] ?? '', current: m[2] !== undefined }));

// AC1: a DM screen opens with the site header holding the kit's menu, before its own content and, on the DM view, before the frame's header
test('every DM screen opens with the kit menu before its own content', () => {
  for (const name of DM_SCREENS) {
    const html = renderScreen(name === 'dm-view' ? { ...dmView, surface: 'dm' } : asDm(name));
    const menu = at(html, '<header class="shell-site-header"><header class="kit-menu">');
    assert.ok(at(html, '<body>') < menu, name);
    assert.ok(menu < at(html, 'data-component='), name);
    if (name === 'dm-view') assert.ok(menu < at(html, 'class="shell-frame__header"'));
  }
});

// AC2, AC3: the brand leads to the campaign list; the nav "Sections" holds the four links in order
test('the menu has its brand and the four sections, in order', () => {
  const html = renderScreen(asDm('campagnes'));
  assert.match(menuOf(html), /<a class="brand" href="\/screens\/campagnes">ForgeDesMondes<\/a>/);
  assert.match(menuOf(html), /<nav aria-label="Sections">/);
  assert.deepEqual(
    linksOf(html).map(({ label, href }) => [label, href]),
    [
      ['Campagnes', '/screens/campagnes'],
      ['Médias', '/screens/medias'],
      ['Soundbox', '/screens/soundbox'],
      ['Paramètres IA', '/screens/parametres-ia'],
    ],
  );
});

// AC4, AC5: the campaign's screens mark Campagnes; Médias, Soundbox and Paramètres IA mark their own; never two at once
test('the current section is marked, alone', () => {
  const expected: Record<string, string> = {
    campagnes: 'Campagnes',
    campagne: 'Campagnes',
    'fiche-entite': 'Campagnes',
    'preparation-seance': 'Campagnes',
    'dm-view': 'Campagnes',
    medias: 'Médias',
    soundbox: 'Soundbox',
    'parametres-ia': 'Paramètres IA',
  };
  for (const name of DM_SCREENS) {
    const marked = linksOf(renderScreen(asDm(name))).filter((link) => link.current).map((link) => link.label);
    assert.deepEqual(marked, [expected[name]], name);
  }
});

// AC6: the players' screen carries no menu and no site header, nor does a screen with no surface
test('player-view and a screen with no surface draw no menu', () => {
  for (const surface of ['player', undefined]) {
    const html = renderScreen({ screen: 'player-view', generatedFrom: 'a goal', version: 1, ...(surface ? { surface } : {}), layout: [{ microUi: 'PlayerStage', props: {} }] });
    assert.doesNotMatch(html, /kit-menu|shell-site-header|aria-current/);
  }
});

// AC7: the page is the same twice, byte for byte
test('the menu is deterministic', () => {
  assert.equal(renderScreen(asDm('medias')), renderScreen(asDm('medias')));
});

// The alert EntityEditor sticks to the top of the page sticks beneath the menu, not under it
test('the sticky unsaved-changes alert is offset by the menu height', async () => {
  const css = await readFile(new URL('../../../src/client/shell.css', import.meta.url), 'utf8');
  const rule = css.match(/\.shell-site-header ~ \* \.mf-entity__unsaved \{([^}]*)\}/)?.[1] ?? '';
  assert.match(rule, /top:\s*calc\(var\(--shell-menu-height\) \+ var\(--space-2\)\)/);
});
