// What the Micro-UI reads and triggers, by identifier only (micro-ui.json):
// the wire protocol takes these strings, never the code behind them.

export const LIST_SOUNDS = 'soundbox.listSounds';
export const GET_LIVE_SESSION = 'session.getLiveSession';
export const ADD_SOUND = 'soundbox.addSound';
export const UPDATE_SOUND = 'soundbox.updateSound';
export const REMOVE_SOUND = 'soundbox.removeSound';

/** The version of a DataCapability's contract, as a command names it. */
export const DATA_CAPABILITY_VERSION = 1;

/** The screen's context name that holds the campaign the soundbox belongs to: the library publishes it for LiveSoundbox. */
export const CAMPAIGN_CONTEXT = 'campaign';
