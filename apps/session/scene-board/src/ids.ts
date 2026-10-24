// What the Micro-UI reads and triggers, by identifier only (micro-ui.json):
// the wire protocol takes these strings, never the code behind them.

export const GET_SESSION = 'session.getSession';
export const LIST_SCENES = 'session.listScenes';
export const LIST_CHARACTERS = 'campaign.listCharacters';
export const LIST_MEDIA = 'media.listMedia';
export const GET_BATTLEMAP_INSTANCE = 'table.getBattlemapInstance';

export const ADD_PRESENT_CHARACTER = 'session.addPresentCharacter';
export const REMOVE_PRESENT_CHARACTER = 'session.removePresentCharacter';
export const ADD_SCENE = 'session.addScene';
export const REORDER_SCENES = 'session.reorderScenes';
export const CREATE_BATTLEMAP_INSTANCE = 'table.createBattlemapInstance';

/** The version of a DataCapability's contract, as a command names it. */
export const DATA_CAPABILITY_VERSION = 1;
