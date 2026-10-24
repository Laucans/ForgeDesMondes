// ToCreateList on the mock-up's page: the Micro-UI's real code, over the page's
// shell client and the mock server (mock/src/server.js), which answers the
// wire protocol from the fixtures. mock/build.sh bundles this file into
// mock/components/ToCreateList.js, which the fragment loads. The page names no
// campaign: the mock-up opens the fixtures' own.

import { createShell } from '../../apps/shell/src/client/shell.ts';
import { mount } from '../../apps/ai/to-create-list/src/index.ts';
import { compose, mountPoint } from './screen.js';
import { mockServer, shellOnce } from './server.js';

const { host, props } = mountPoint();
// The shell's client calls its own origin by path; the mock server wants an absolute URL.
const shell = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));
mockServer().load('ToCreateList').then((fixture) => {
  /** What the microphone "hears" on the mock-up, where no speech-to-text port listens: the fixture's dictation of the entry opened. */
  const heard = () => {
    // The transcript is asked as the pause happens: the entry whose microphone still records.
    const id = host.querySelector('.kit-micbutton[data-state="recording"]')?.closest('[data-id]')?.dataset.id;
    return (id && fixture.proposals[id]?.dictation) || '';
  };
  compose({ campaign: 'cendres-valombre', ...props }, (resolved) => mount(host, resolved, shell, undefined, { transcript: heard }));
});
