// OralBrief on the mock-up's page: the Micro-UI's real code, over the page's
// shell client and the mock server (mock/src/server.js), which answers the
// wire protocol from the fixtures. mock/build.sh bundles this file into
// mock/components/OralBrief.js, which the fragment loads. The page gives no
// session: the mock-up's is the fixture's.

import { createShell } from '../../apps/shell/src/client/shell.ts';
import { mount } from '../../apps/ai/oral-brief/src/index.ts';
import { compose, mountPoint } from './screen.js';
import { mockServer, shellOnce } from './server.js';

const { host, props } = mountPoint();
// The shell's client calls its own origin by path; the mock server wants an absolute URL.
const shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
mockServer().load('OralBrief').then((fixture) => {
  // What the microphone "hears" on the mock-up, where no speech-to-text port listens: the brief told in the fixture.
  const dictation = { transcript: () => fixture.transcript.join('\n') };
  // « Voir le diff » opens the changed sheet's history, which the page's other micro-frontend follows.
  host.addEventListener('open-versions', (e) => window.Mock?.emit('open-versions', e.detail));
  compose({ session: fixture.session, ...props }, (resolved) => mount(host, resolved, shell, undefined, dictation));
});
