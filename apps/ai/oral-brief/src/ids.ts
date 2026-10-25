// What the Micro-UI reads and triggers, by identifier only (micro-ui.json):
// the wire protocol takes these strings, never the code behind them.

export const GET_LAST_BRIEF = 'session.getLastBrief';
export const ANALYZE_BRIEF = 'ai.analyzeBrief';

/** The role the DM holds as the approver of the Session's critical fields: the command applies, it is not left awaiting review. */
export const APPROVER = 'role:approver';

/** The version of a DataCapability's contract, as a command names it. */
export const DATA_CAPABILITY_VERSION = 1;
