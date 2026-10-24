// What the Micro-UI reads and triggers, by identifier only (micro-ui.json):
// the wire protocol takes these strings, never the code behind them.

export const GET_ENTITY = 'campaign.getEntity';
export const LIST_PLAYERS = 'campaign.listPlayers';
export const LIST_MEDIA = 'media.listMedia';
export const SAVE_ENTITY = 'campaign.saveEntity';
export const PROPOSE_CHARACTER = 'ai.proposeCharacter';
export const IMPORT_CHARACTER = 'dndbeyond.importCharacter';
export const RESYNC_CHARACTER = 'dndbeyond.resyncCharacter';

/** The role every command of the sheet carries: the DM approving their own act (Entity.characterEntityId, versionNumber… are critical). */
export const APPROVER = 'role:approver';

/** The version of a DataCapability's contract, as a command names it. */
export const DATA_CAPABILITY_VERSION = 1;

/** The aggregate every command of the sheet targets. */
export const ENTITY_AGGREGATE = 'Entity';

/** The screen context's names: the sheet on show (the rail follows it) and the campaign a new sheet belongs to. */
export const ENTITY_CONTEXT = 'entity';
export const CAMPAIGN_CONTEXT = 'campaign';

/** The prop value that opens a sheet to create: `new:<kind>`. */
export const NEW_PREFIX = 'new:';

/** The DOM events the sheet dispatches from its host, for whoever composes it. */
export const EVENT_OPENED = 'entity-editor:opened';
export const EVENT_UPDATED = 'entity-editor:updated';
export const EVENT_HISTORY = 'entity-editor:history';

/** The page's bus, as the rail words it: DOM events on the document. The rail asks (`open-entity`, `entity-query`), the sheet answers (`entity-opened`). */
export const BUS_OPEN = 'fdm:open-entity';
export const BUS_QUERY = 'fdm:entity-query';
export const BUS_OPENED = 'fdm:entity-opened';
