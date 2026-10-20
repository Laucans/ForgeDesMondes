// VersionHistory on the mock-up's pages: the Micro-UI's real code, over the page's
// shell client and the mock server (mock/src/server.js), which answers the wire
// protocol from the fixtures. mock/build.sh bundles this file into
// mock/components/VersionHistory.js, which the fragment loads.
//
// The other sheets' fragments are still drawings: they tell the page by its bus
// (`Mock.emit`) what the real saves will do through the server. This glue turns
// that into what the server would hold — a version in the fixture, the data moved
// — and passes the page's other words on: the sheet opened in place, « voir le diff ».

import { createShell } from '../../apps/shell/src/client/shell.ts';
import { mount } from '../../apps/campaign/version-history/src/index.ts';
import { compose, mountPoint } from './screen.js';
import { mockServer, shellOnce } from './server.js';

const { host, props } = mountPoint();
const server = mockServer();
// The shell's client calls its own origin by path; the mock server wants an absolute URL.
const shell = shellOnce((s) => createShell((input, init) => s.fetch(new URL(input, location.href), init)));
const param = (name) => new URLSearchParams(location.search).get(name);

// The sheet on show: the address's ?id=, else the page's prop, else Morlox; ?new= is a sheet not saved yet.
const first = param('id') || props.entity || (param('new') ? 'nouvelle-fiche' : 'morlox');
let current = { ...props, entity: first };
const mounted = compose(current, (resolved) => mount(host, { ...resolved, entity: first }, shell));

// A sheet saved elsewhere on the page: its new version, as the server would list it.
Mock.on('entity-updated', async (u) => {
  const all = await server.load('campaign.listEntityVersions');
  const entities = await server.load('entities');
  const known = [...entities.characters, ...entities.places, ...entities.events].find((e) => e.id === u.id);
  const history = (all[u.id] = all[u.id] || { name: known ? known.name : u.id, versions: [] });
  history.versions.unshift({ v: (history.versions[0] ? history.versions[0].v : 0) + 1, date: new Date().toISOString(), origin: u.origin, source: u.source, restoredFrom: null, seen: false, changes: u.changes });
  server.bump();
  mounted.update(current);
});
// The editor opened another sheet in place: follow it, with its own history.
Mock.on('entity-opened', (u) => {
  current = { ...current, entity: u.id || 'nouvelle-fiche' };
  mounted.update(current);
});
// « Voir le diff » of the brief: the sheet it changed, its history open.
Mock.on('open-versions', (u) => {
  current = { ...current, entity: u.id };
  mounted.update(current);
  mounted.open();
});
