// What the Micro-UI reads and triggers, by identifier only (micro-ui.json):
// the wire protocol takes these strings, never the code behind them.

export const LIST_SCENES = 'session.listScenes';
// Read through the shell all the same: the system frontier admits a Capability of any system in `needs`.
export const LIST_CHARACTERS = 'campaign.listCharacters';
export const LIST_PLACES = 'campaign.listPlaces';
export const LIST_EVENTS = 'campaign.listEvents';
export const LIST_MEDIA = 'media.listMedia';

export const SHOW_ON_PLAYER_VIEW = 'live.showOnPlayerView';
export const SHOW_WINDOW_ON_PLAYER_VIEW = 'live.showWindowOnPlayerView';

/** The aggregate both diffusion commands target. */
export const AGGREGATE = 'Diffusion';

/** The version of a DataCapability's contract, as a command names it. */
export const DATA_CAPABILITY_VERSION = 1;

/** The screen's context names the panel reads: the live session and its campaign, set by the screen. */
export const SESSION_CONTEXT = 'session';
export const CAMPAIGN_CONTEXT = 'campaign';

/** What the panel tells the screen, as the mock-up's page listens for it: the centre shows an element's sheet, or the battlemap. */
export const OPEN_EVENT = 'fdm:open';
export const OPEN_MAP_EVENT = 'fdm:open-map';
/** What the screen tells the panel: the scene the session is at. */
export const SCENE_EVENT = 'fdm:scene';

/** The drag payload the Battlemap takes a character from (it owns the drop). */
export const CHARACTER_DRAG_TYPE = 'text/fdm-character';
