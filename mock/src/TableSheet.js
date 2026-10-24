// TableSheet on the mock-up's page: the Micro-UI's real code, over the page's
// shell client and the mock server (mock/src/server.js), which answers the
// wire protocol from the fixtures, mock/src/reads.js and mock/src/commands.js.
// mock/build.sh bundles this file into mock/components/TableSheet.js, which
// the fragment loads.

import { createShell } from '../../apps/shell/src/client/shell.ts';
import { mount } from '../../apps/live/table-sheet/src/index.ts';
import { compose, mountPoint } from './screen.js';
import { shellOnce } from './server.js';

const { host, props } = mountPoint();
// The shell's client calls its own origin by path; the mock server wants an absolute URL.
const shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
// The mock page names no element: it opens the tavern, the fixtures' own, until the DM opens another.
const first = { type: 'place', id: 'taverne-sanglier' };
compose(props, (resolved) => mount(host, resolved.id ? resolved : { ...first, ...resolved }, shell));
