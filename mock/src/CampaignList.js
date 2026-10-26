// CampaignList on the mock-up's page: the Micro-UI's real code, over the
// page's shell client and the mock server (mock/src/server.js), which answers
// the wire protocol from the fixtures and the handlers. mock/build.sh bundles
// this file into mock/components/CampaignList.js, which the fragment loads.

import { createShell } from '../../apps/shell/src/client/shell.ts';
import { mount } from '../../apps/campaign/campaign-list/src/index.ts';
import { compose, mountPoint } from './screen.js';
import { shellOnce } from './server.js';

const { host, props } = mountPoint();
// The shell's client calls its own origin by path; the mock server wants an absolute URL.
const shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
compose(props, (resolved) => mount(host, resolved, shell));
