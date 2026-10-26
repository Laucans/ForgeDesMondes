// What the Micro-UI reads and triggers, by identifier only (micro-ui.json):
// the wire protocol takes these strings, never the code behind them.

export const LIST_CAMPAIGNS = 'campaign.listCampaigns';
export const CREATE_CAMPAIGN = 'campaign.createCampaign';
export const EXPORT_CAMPAIGN = 'campaign.exportCampaign';
export const IMPORT_CAMPAIGN = 'campaign.importCampaign';

/** The version of a DataCapability's contract, as a command names it. */
export const DATA_CAPABILITY_VERSION = 1;
