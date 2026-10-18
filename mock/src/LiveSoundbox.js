// LiveSoundbox on the mock-up's page: the Micro-UI's real code, over the page's
// shell client and the mock server (mock/src/server.js), which answers the
// wire protocol from the fixtures. mock/build.sh bundles this file into
// mock/components/LiveSoundbox.js, which the fragment loads.

import { createShell } from '../../apps/shell/src/client/shell.ts';
import { mount } from '../../apps/soundbox/live-soundbox/src/index.ts';
import { compose, mountPoint } from './screen.js';
import { shellOnce } from './server.js';

const { host, props } = mountPoint();
// The shell's client calls its own origin by path; the mock server wants an absolute URL.
const shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));

// A sound the suggestion pill plays reaches the soundbox on the page's bus, as
// in the pill's own fragment: here it becomes the playSound command the pill
// would send, and the panel reads the library again to hear it.
const mounted = compose(props, (resolved) => mount(host, resolved, shell));
document.addEventListener('fdm:play-sound', (event) => {
  const body = { dataCapability: 'soundbox.playSound@1', payload: { source: 'suggestion' }, target: { id: event.detail.id }, caller: { by: 'dm', from: 'SuggestionPill', roles: [] } };
  void shell.command(body).then(() => mounted?.update?.({}));
});
