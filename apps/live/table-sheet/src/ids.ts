// What the Micro-UI reads and triggers, by identifier only (micro-ui.json):
// the wire protocol takes these strings, never the code behind them.

export const GET_ENTITY = 'campaign.getEntity';
export const LIST_MEDIA = 'media.listMedia';

export const SHOW_ON_PLAYER_VIEW = 'live.showOnPlayerView';
export const SHOW_WINDOW_ON_PLAYER_VIEW = 'live.showWindowOnPlayerView';

/** The aggregate both diffusion commands target. */
export const AGGREGATE = 'Diffusion';

/** The version of a DataCapability's contract, as a command names it. */
export const DATA_CAPABILITY_VERSION = 1;

/** The document event the DM view routes to open an element at the centre (`detail`: `{ type, id }`); the sheet listens to it and raises it for the characters of a place. */
export const OPEN_EVENT = 'fdm:open';

/** The document event that asks the centre for the battlemap (`detail`: `{ place }`). */
export const OPEN_MAP_EVENT = 'fdm:open-map';

/** The entity editor an element is modified in; explicit-save-only stays its rule. */
export const EDITOR = 'fiche-entite.html';
