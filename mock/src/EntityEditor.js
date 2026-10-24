// EntityEditor on the mock-up's page: the Micro-UI's real code, over the page's
// shell client and the mock server (mock/src/server.js), which answers the wire
// protocol from the fixtures. mock/build.sh bundles this file into
// mock/components/EntityEditor.js, which the fragment loads.
//
// The sheet speaks the page's bus itself (the rail's `open-entity`, its own
// `entity-opened`, the address's `?id=`). This glue turns its other DOM events
// into the bus's words: `entity-updated` (the history reads again) and
// `open-versions` (« Voir le diff »). What the AI or D&D Beyond changed is
// already a version on the mock server: the history only has to read it again.

import { createShell } from '../../apps/shell/src/client/shell.ts';
import { mount } from '../../apps/campaign/entity-editor/src/index.ts';
import { compose, mountPoint } from './screen.js';
import { mockServer, shellOnce } from './server.js';

const param = (name) => new URLSearchParams(location.search).get(name);

const { host, props } = mountPoint();
const server = mockServer();
// The shell's client calls its own origin by path; the mock server wants an absolute URL.
const shell = shellOnce((s) => createShell((input, init) => s.fetch(new URL(input, location.href), init)));

let opened = null;
host.addEventListener('entity-editor:opened', (e) => { opened = e.detail; });
host.addEventListener('entity-editor:updated', (e) => Mock.emit('entity-updated', { id: e.detail.id, versioned: true }));
host.addEventListener('entity-editor:history', (e) => Mock.emit('open-versions', { id: e.detail.id }));

Promise.all([server.load('EntityEditor'), server.load('campaign.getCampaign')]).then(([editor, campaign]) => {
  // The screen's context: the campaign a new sheet joins.
  shell.context.set('campaign', campaign.id);
  // Where no speech-to-text port listens, the microphone "hears" the dictation of the fixture.
  const dictation = { transcript: () => (opened && editor.dictations[opened.id]) || editor.dictations.default };
  const first = param('id') || (param('new') ? `new:${param('new')}` : null) || props.entity || editor.defaultEntity;
  compose({ ...props, entity: first }, (resolved) => mount(host, resolved, shell, undefined, dictation));
});
