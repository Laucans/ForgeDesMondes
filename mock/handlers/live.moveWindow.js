import { onScreen, onWindow } from './_lib/commands.js';

export const commands = {
  // live.moveWindow (crates/live/data-capabilities/move-window): x and y alone,
  // brought back onto the screen (0 to 1); the size and the content stay.
  'live.moveWindow': (server, { payload }) => onWindow(server, (w) => { w.x = onScreen(payload.x, w.x); w.y = onScreen(payload.y, w.y); }),
};
