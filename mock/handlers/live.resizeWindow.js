import { onWindow } from './_lib/commands.js';
import { ratioOfIllustration } from './_lib/reads.js';

export const commands = {
  // live.resizeWindow (crates/live/data-capabilities/resize-window): only the width
  // is stored, the largest that fits the box asked for at the image's ratio — w
  // alone with no h — from 0.1 to the screen's width, to 4 decimals
  // (window-proportional); the position stays. A w that is no number, which the
  // server refuses, keeps the width.
  'live.resizeWindow': (server, { payload }) => onWindow(server, async (w) => {
    if (typeof payload.w !== 'number' || !Number.isFinite(payload.w)) return undefined;
    const ratio = await ratioOfIllustration(server, w.id || w.media);
    const fit = typeof payload.h === 'number' && Number.isFinite(payload.h) ? Math.min(payload.w, payload.h * ratio) : payload.w;
    w.w = Math.round(Math.max(0.1, Math.min(1, fit)) * 10000) / 10000;
    return undefined;
  }),
};
