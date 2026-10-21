// What the Micro-UI reads and triggers, by identifier only (micro-ui.json):
// the wire protocol takes these strings, never the code behind them.

export const GET_DIFFUSION = 'live.getDiffusion';
// Read through the shell all the same: the system frontier admits a Capability of any system in `needs`.
export const GET_CAMPAIGN = 'campaign.getCampaign';
export const LIST_MEDIA = 'media.listMedia';

export const MOVE_WINDOW = 'live.moveWindow';
export const RESIZE_WINDOW = 'live.resizeWindow';

/** The aggregate both window commands target. */
export const AGGREGATE = 'Diffusion';

/** The micro-frontend a battlemap base is handed to: composed by the screen, never drawn here. */
export const BATTLEMAP = 'Battlemap';

/** The screen's context name under which the battlemap shown is told to the Micro-UIs that follow it. */
export const BATTLEMAP_CONTEXT = 'battlemap';

/** The version of a DataCapability's contract, as a command names it. */
export const DATA_CAPABILITY_VERSION = 1;
