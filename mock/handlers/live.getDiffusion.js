import { diffusionRow, ratioOfIllustration } from './_lib/reads.js';

export const reads = {
  // live.getDiffusion (crates/live/capabilities/get-diffusion): the one row both
  // player views read, so view 'dm' and view 'player' answer the same values —
  // position, size, zoom and pan. The window's x and w are fractions of the stage's
  // width, y of its height; h, derived from the image's ratio, is in the width's unit
  // and never stored. Nothing else leaves the row: no token, no HP figure, no note.
  'live.getDiffusion': async (server) => {
    const row = await diffusionRow(server);
    const win = row.window;
    const ratio = win ? await ratioOfIllustration(server, win.id || win.media) : 1;
    return {
      base: { kind: row.base.kind, id: row.base.id || '' },
      window: win ? { media: win.id || win.media, x: win.x, y: win.y, w: win.w, h: win.w / ratio } : null,
      view: row.view ? { zoom: row.view.z ?? row.view.zoom, panX: row.view.px ?? row.view.panX, panY: row.view.py ?? row.view.panY } : null,
    };
  },
};
