// What the Micro-UI reads and triggers, by identifier only (micro-ui.json):
// the wire protocol takes these strings, never the code behind them.

export const LIST_SOUNDS = 'soundbox.listSounds';
export const PLAY_SOUND = 'soundbox.playSound';
export const STOP_ALL = 'soundbox.stopAll';
export const SET_SOUND_VOLUME = 'soundbox.setSoundVolume';
export const SET_MASTER_VOLUME = 'soundbox.setMasterVolume';

/** The version of a DataCapability's contract, as a command names it. */
export const DATA_CAPABILITY_VERSION = 1;

/** The screen's context name that holds the campaign the soundbox belongs to. */
export const CAMPAIGN_CONTEXT = 'campaign';
