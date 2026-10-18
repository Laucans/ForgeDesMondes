// GridSettings on the mock-up's page: the Micro-UI's real code, over the page's
// shell client and the mock server (mock/src/server.js), which answers the
// wire protocol from the fixtures. mock/build.sh bundles this file into
// mock/components/GridSettings.js, which the fragment loads.

import { createShell } from '../../apps/shell/src/client/shell.ts';
import { mount } from '../../apps/media/grid-settings/src/index.ts';
import { compose, getContext, mountPoint, setContext } from './screen.js';
import { mockServer, shellOnce } from './server.js';

const { host, props } = mountPoint();
// The shell's client calls its own origin by path; the mock server wants an absolute URL.
const shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
// The page names no battlemap: the one the DM chose in the library (the screen's context), else the first.
const raw = props.battlemap ? props : { ...props, battlemap: '$ctx.battlemap' };
compose(raw, (resolved) => mount(host, resolved, shell));
if (!props.battlemap) {
  window.Mock?.on('grid-edit', (d) => { if (d && d.id) setContext('battlemap', d.id); });
  if (getContext('battlemap') === undefined) {
    mockServer().load('media').then((media) => { if (media && media.battlemaps.length) setContext('battlemap', media.battlemaps[0].id); });
  }
}
