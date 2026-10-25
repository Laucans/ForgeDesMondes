// What the Micro-UI reads and triggers, by identifier only (micro-ui.json):
// the wire protocol takes these strings, never the code behind them.

export const GET_SESSION = 'session.getSession';
export const LIST_SCENES = 'session.listScenes';
export const LIST_CHARACTERS = 'campaign.listCharacters';
export const LIST_PLACES = 'campaign.listPlaces';
export const LIST_EVENTS = 'campaign.listEvents';
export const LIST_MEDIA = 'media.listMedia';
export const GET_BATTLEMAP_INSTANCE = 'table.getBattlemapInstance';

export const ADD_PRESENT_CHARACTER = 'session.addPresentCharacter';
export const REMOVE_PRESENT_CHARACTER = 'session.removePresentCharacter';
export const ADD_SCENE = 'session.addScene';
export const REORDER_SCENES = 'session.reorderScenes';
export const CREATE_BATTLEMAP_INSTANCE = 'table.createBattlemapInstance';
export const UPDATE_SCENE_NOTES = 'session.updateSceneNotes';

/** The role the DM holds as the approver of the Session's critical fields: the command applies, it is not left awaiting review. */
export const APPROVER = 'role:approver';

/** The version of a DataCapability's contract, as a command names it. */
export const DATA_CAPABILITY_VERSION = 1;
