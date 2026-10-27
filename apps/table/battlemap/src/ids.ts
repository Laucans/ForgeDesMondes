// What the Micro-UI reads and triggers, by identifier only (micro-ui.json):
// the wire protocol takes these strings, never the code behind them.

export const GET_BATTLEMAP_INSTANCE = 'table.getBattlemapInstance';
// Capabilities of other systems (media, campaign): read through the shell by identifier, and listed in `needs` all the same.
export const GET_BATTLEMAP = 'media.getBattlemap';
export const LIST_CHARACTERS = 'campaign.listCharacters';
export const GET_DIFFUSION = 'live.getDiffusion';

export const MOVE_TOKEN = 'table.moveToken';
export const PLACE_TOKENS = 'table.placeTokens';
export const REMOVE_TOKEN = 'table.removeToken';
export const RENAME_TOKEN = 'table.renameToken';
export const SET_TOKEN_VISIBILITY = 'table.setTokenVisibility';
export const SET_TOKEN_CONDITION = 'table.setTokenCondition';
export const CHANGE_HIT_POINTS = 'table.changeHitPoints';
export const START_COMBAT = 'table.startCombat';
export const NEXT_TURN = 'table.nextTurn';
export const REORDER_INITIATIVE = 'table.reorderInitiative';
export const ADD_PARTICIPANT = 'table.addParticipant';
export const REMOVE_PARTICIPANT = 'table.removeParticipant';
export const END_COMBAT = 'table.endCombat';
export const RESET_BATTLEMAP = 'table.resetBattlemap';
export const SHOW_ON_PLAYER_VIEW = 'live.showOnPlayerView';

/** The aggregate every table command targets, by the instance's id. */
export const AGGREGATE = 'BattlemapInstance';

/** The version of a DataCapability's contract, as a command names it. */
export const DATA_CAPABILITY_VERSION = 1;

/** The role every command of the map carries: the DM approving their own act (BattlemapInstance.q, r, visible, hpCurrent… are critical, and a command without it waits for a review instead of being stored). */
export const APPROVER = 'role:approver';
