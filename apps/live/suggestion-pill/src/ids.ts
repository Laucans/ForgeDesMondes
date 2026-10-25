// What the Micro-UI reads and triggers, by identifier only (micro-ui.json):
// the wire protocol takes these strings, never the code behind them.

export const LIST_SUGGESTIONS = 'live.listSuggestions';

export const SET_MICROPHONE = 'live.setMicrophone';
export const PLAY_SOUND = 'soundbox.playSound';

/** The version of a DataCapability's contract, as a command names it. */
export const DATA_CAPABILITY_VERSION = 1;

/** This Micro-UI, as a command names its caller. */
export const MICRO_UI = 'SuggestionPill';

/** The page's bus: the DM view's centre follows what is opened there (`{ type, id }`). */
export const BUS_OPEN = 'fdm:open';

/** What a sound's suggestion tells soundbox.playSound it is triggered from. */
export const SOUND_SOURCE = 'suggestion';
