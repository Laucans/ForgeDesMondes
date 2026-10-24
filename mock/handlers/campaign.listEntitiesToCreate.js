export const reads = {
  // campaign.listEntitiesToCreate (crates/campaign/capabilities/list-entities-to-create):
  // the to-create list, answered as `{ items }`; the fixture is the bare list of entries.
  'campaign.listEntitiesToCreate': async (server) => {
    const fixture = await server.load('campaign.listEntitiesToCreate');
    return { items: Array.isArray(fixture) ? fixture : (fixture && fixture.items) || [] };
  },
};
