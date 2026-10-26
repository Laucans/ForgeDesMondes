// What the Micro-UI reads and triggers, by identifier only (micro-ui.json):
// the wire protocol takes these strings, never the code behind them.

export const GET_LIVE_SESSION = 'session.getLiveSession';
/** The live session's scenes, each with the battlemap instance it holds. */
export const LIST_SCENES = 'session.listScenes';
export const GET_DIFFUSION = 'live.getDiffusion';
/** Read once the session is closed, for its recap. */
export const GET_SESSION_JOURNAL = 'session.getSessionJournal';
export const GET_PREVIOUS_SESSION_SUMMARY = 'session.getPreviousSessionSummary';

export const SHOW_COVER = 'live.showCover';
export const CLOSE_WINDOW = 'live.closeWindow';
export const CLOSE_SESSION = 'session.closeSession';

/** The version of a DataCapability's contract, as a command names it. */
export const DATA_CAPABILITY_VERSION = 1;

/** The aggregate `session.closeSession` targets. */
export const SESSION_AGGREGATE = 'Session';

/**
 * The screen's context names the bar publishes, for the Micro-UIs that follow it: the current scene (its
 * position in the session), the live session, its campaign and the battlemap instance of the live scene.
 */
export const SCENE_CONTEXT = 'scene';
export const SESSION_CONTEXT = 'session';
export const CAMPAIGN_CONTEXT = 'campaign';
export const BATTLEMAP_CONTEXT = 'battlemap';
