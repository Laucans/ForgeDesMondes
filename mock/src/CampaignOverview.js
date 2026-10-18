// CampaignOverview on the mock-up's page: the Micro-UI's real code, over the
// page's shell client and the mock server (mock/src/server.js), which answers
// the wire protocol from the fixtures. mock/build.sh bundles this file into
// mock/components/CampaignOverview.js, which the fragment loads.

import { createShell } from '../../apps/shell/src/client/shell.ts';
import { mount } from '../../apps/campaign/campaign-overview/src/index.ts';
import { compose, mountPoint } from './screen.js';
import { shellOnce } from './server.js';

const { host, props } = mountPoint();
// The shell's client calls its own origin by path; the mock server wants an absolute URL.
const shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
// The mock page names no campaign: it opens the fixtures' own, unless the address names one.
const opened = new URLSearchParams(location.search).get('id') ? {} : { campaign: 'cendres-valombre' };
compose(props, (resolved) => mount(host, { ...opened, ...resolved }, shell));
