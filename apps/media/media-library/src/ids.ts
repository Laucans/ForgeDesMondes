// What the Micro-UI reads and triggers, by identifier only (micro-ui.json):
// the wire protocol takes these strings, never the code behind them.

export const LIST_MEDIA = 'media.listMedia';
export const LIST_SCENES = 'session.listScenes';
export const LIST_PLACES = 'campaign.listPlaces';
export const UPLOAD_MEDIA = 'media.uploadMedia';
export const SET_MEDIA_PLACE = 'media.setMediaPlace';
export const ADD_PLACE_TO_SESSION = 'session.addPlaceToSession';

/** Everything a card shows comes from these three reads. */
export const READS = `${LIST_MEDIA} ${LIST_SCENES} ${LIST_PLACES}`;

/** The version of a DataCapability's contract, as a command names it. */
export const DATA_CAPABILITY_VERSION = 1;

/** The screen context's name for the battlemap whose grid is being set: GridSettings follows it. */
export const BATTLEMAP_CONTEXT = 'battlemap';
