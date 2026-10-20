// NoteImport on the mock-up's page: the Micro-UI's real code, over the page's
// shell client and the mock server (mock/src/server.js), which answers the
// wire protocol from the fixtures. mock/build.sh bundles this file into
// mock/components/NoteImport.js, which the fragment loads. The page gives no
// campaign: the mock-up's is the fixture's, and its draft fills the text area.

import { createShell } from '../../apps/shell/src/client/shell.ts';
import { mount } from '../../apps/ai/note-import/src/index.ts';
import { compose, mountPoint } from './screen.js';
import { mockServer, shellOnce } from './server.js';

/** What the microphone "hears" on the mock-up, where no speech-to-text port listens. */
const HEARD = 'Dicté : Mère Ombeline garde un herbier de plantes qui ne poussent qu’au pied du mont Pourpre.';

const { host, props } = mountPoint();
// The shell's client calls its own origin by path; the mock server wants an absolute URL.
const shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
mockServer().load('NoteImport').then((fixture) => {
  compose({ campaign: fixture.campaign, ...props }, (resolved) => {
    const mounted = mount(host, resolved, shell, undefined, { transcript: () => HEARD });
    host.querySelector('#notes-text').value = fixture.draft;
    return mounted;
  });
});
