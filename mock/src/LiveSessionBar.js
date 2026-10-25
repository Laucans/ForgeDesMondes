// LiveSessionBar on the mock-up's page: the Micro-UI's real code, over the page's
// shell client and the mock server (mock/src/server.js), which answers the
// wire protocol from the fixtures. mock/build.sh bundles this file into
// mock/components/LiveSessionBar.js, which the fragment loads.

import { createShell } from '../../apps/shell/src/client/shell.ts';
import { DEFAULT_TIMING, mount } from '../../apps/session/live-session-bar/src/index.ts';
import { compose, mountPoint } from './screen.js';
import { shellOnce } from './server.js';

const { host, props } = mountPoint();
// The shell's client calls its own origin by path; the mock server wants an absolute URL.
const shared = shellOnce((server) => createShell((input, init) => server.fetch(new URL(input, location.href), init)));

// The page's hand-drawn neighbours still listen to the mock-up's own events: the
// current scene reaches them as `fdm:scene`, and a closed session pauses the
// suggestions' microphone, as the real server pauses it.
const shell = Object.assign(Object.create(shared), {
  command: async (body) => {
    const answer = await shared.command(body);
    if (String(body && body.dataCapability).startsWith('session.closeSession')) {
      const mic = document.getElementById('suggestions-mic-button');
      const control = mic && mic.closest('[data-kit]');
      if (control && control.mic && control.mic.state === 'recording') control.mic.set('paused');
    }
    return answer;
  },
});
shared.context.on((name, value) => {
  if (name === 'scene') document.dispatchEvent(new CustomEvent('fdm:scene', { detail: { index: value } }));
});

// The bar closes the session of its fixture at the time that fixture says it ran for: « 3 h 27 min » after 20:02.
fetch(new URL('../fixtures/LiveSessionBar.json', location.href))
  .then((answer) => answer.json())
  .then((bar) => {
    const [hours, minutes] = bar.session.startedAt.split(':').map(Number);
    const [, ranHours = 0, ranMinutes = 0] = (/(\d+) h (\d+) min/.exec(bar.closing.duration) || []).map(Number);
    const timing = { ...DEFAULT_TIMING, now: () => new Date(2026, 0, 1, hours + ranHours, minutes + ranMinutes) };
    compose(props, (resolved) => mount(host, resolved, shell, timing));
  });
