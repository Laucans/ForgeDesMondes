// Battlemap on the mock-up's pages: the Micro-UI's real code, over the page's
// shell client and the mock server (mock/src/server.js), which answers the
// wire protocol from the fixtures, mock/src/reads.js and mock/src/commands.js.
// mock/build.sh bundles this file into mock/components/Battlemap.js, which the
// fragment loads.

import { createShell } from '../../apps/shell/src/client/shell.ts';
import { mount } from '../../apps/table/battlemap/src/index.ts';
import { compose, mountPoint } from './screen.js';
import { mockServer, shellOnce } from './server.js';

const { host, props } = mountPoint();
// The shell's client calls its own origin by path; the mock server wants an absolute URL.
const base = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
// The map follows the data as it moves — another tab's drop included. The real server answers `GET /dataVersion`;
// the mock server tells the same through its event stream.
const shell = {
  ...base,
  onDataVersion(listener) {
    const server = mockServer();
    const source = server.eventSource();
    let watching = true;
    source.addEventListener('dataVersion', (message) => { if (watching) listener(JSON.parse(message.data).dataVersion); });
    listener(server.dataVersion);
    return () => { watching = false; source.close(); };
  },
};
compose(props, (resolved) => mount(host, resolved, shell));
