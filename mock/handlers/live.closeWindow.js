import { diffusionRow } from './_lib/reads.js';

export const commands = {
  // live.closeWindow (crates/live/data-capabilities/close-window): the window's
  // media, position and width are emptied together (window-fields-together);
  // the base and the framing stay. A window already closed is closed again.
  'live.closeWindow': async (server) => {
    const row = await diffusionRow(server);
    row.window = null;
    if (window.Mock) window.Mock.store.set('diffusion', row);
    return undefined;
  },
};
