// ElementsPanel on the mock-up's page: the Micro-UI's real code, over the page's
// shell client and the mock server (mock/src/server.js), which answers the
// wire protocol from the fixtures, mock/src/reads.js and mock/src/commands.js.
// mock/build.sh bundles this file into mock/components/ElementsPanel.js, which
// the fragment loads.

import { createShell } from '../../apps/shell/src/client/shell.ts';
import { mount } from '../../apps/live/elements-panel/src/index.ts';
import { getContext, onContext, mountPoint } from './screen.js';
import { shellOnce } from './server.js';

const { host, props } = mountPoint();
// The shell's client calls its own origin by path; the mock server wants an absolute URL.
const base = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
// The panel reads for the session and the campaign the screen's context names. The mock-up's page sets none:
// the session in preparation of the fixtures, s4, and its campaign are the ones the mock server answers for.
const shell = {
  ...base,
  context: {
    get: (name) => getContext(name) ?? { session: 's4', campaign: 'cendres-valombre' }[name],
    on: (listener) => onContext(listener),
  },
};
mount(host, props, shell);
