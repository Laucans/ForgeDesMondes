// What the Micro-UI reads and triggers, by identifier only (micro-ui.json):
// the wire protocol takes these strings, never the code behind them.

export const GET_BATTLEMAP = 'media.getBattlemap';
export const SAVE_GRID_SETTINGS = 'media.saveGridSettings';

/** The version of a DataCapability's contract, as a command names it. */
export const DATA_CAPABILITY_VERSION = 1;

/** The aggregate a grid save targets: the battlemap is a Media. */
export const AGGREGATE = 'Media';
