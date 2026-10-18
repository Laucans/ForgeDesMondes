// What the Micro-UI reads and triggers, by identifier only (micro-ui.json):
// the wire protocol takes these strings, never the code behind them.

export const GET_BATTLEMAP_INSTANCE = 'table.getBattlemapInstance';
// Read through the shell all the same: contract D and the system frontier admit only the Micro-UI's own system in `needs`.
export const GET_BATTLEMAP = 'media.getBattlemap';
export const LIST_CHARACTERS = 'campaign.listCharacters';

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
