// CampaignElements on the mock-up's pages: the Micro-UI's real code, over the
// page's shell client and the mock server (mock/src/server.js), which answers
// the wire protocol from the fixtures. mock/build.sh bundles this file into
// mock/components/CampaignElements.js, which the fragment loads. The rail's
// open request and the sheet's answer travel as the page's own words
// (`Mock.emit('open-entity')`, `Mock.on('entity-opened')`), which the Micro-UI
// speaks by default on the document.

import { createShell } from '../../apps/shell/src/client/shell.ts';
import { mount } from '../../apps/campaign/campaign-elements/src/index.ts';
import { compose, mountPoint } from './screen.js';
import { shellOnce } from './server.js';

const { host, props } = mountPoint();
// The shell's client calls its own origin by path; the mock server wants an absolute URL.
const shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
compose(props, (resolved) => mount(host, resolved, shell));
