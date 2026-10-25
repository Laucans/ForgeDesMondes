export const commands = {
  // campaign.exportCampaign (crates/dataguard/data-capabilities/export-campaign):
  // stamps the campaign's last export with today's date and changes nothing
  // else (`export-stamps-date`). An unknown campaign is `not-found`.
  'campaign.exportCampaign': async (server, { payload }) => {
    const campaign = await server.load('campaign.getCampaign');
    if (!campaign || campaign.id !== payload.id) return ['not-found'];
    const now = new Date();
    const pad = (n) => String(n).padStart(2, '0');
    campaign.lastExport = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
    return undefined;
  },
};
