// What the Micro-UI reads and triggers, by identifier only (micro-ui.json):
// the wire protocol takes these strings, never the code behind them.

export const LIST_ENTITY_VERSIONS = 'campaign.listEntityVersions';
export const RESTORE_ENTITY_VERSION = 'campaign.restoreEntityVersion';

/** The version of a DataCapability's contract, as a command names it. */
export const DATA_CAPABILITY_VERSION = 1;

/** The aggregate a restore targets: the element the history belongs to. */
export const ENTITY_AGGREGATE = 'Entity';
