// What the Micro-UI reads and triggers, by identifier only (micro-ui.json):
// the wire protocol takes these strings, never the code behind them.

export const LIST_ENTITIES_TO_CREATE = 'campaign.listEntitiesToCreate';
export const GET_ENTITY = 'campaign.getEntity';
export const CREATE_FROM_BRIEF = 'ai.createFromBrief';

/** The version of a DataCapability's contract, as a command names it. */
export const DATA_CAPABILITY_VERSION = 1;

/** The aggregate `ai.createFromBrief` targets: the session the entry came from. */
export const SESSION_AGGREGATE = 'Session';

/** The role every command of the list carries: the DM approving their own act (creating an Entity writes critical fields, which the DataGuard parks without it). */
export const APPROVER = 'role:approver';
