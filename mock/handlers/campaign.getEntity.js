export const reads = {
  // campaign.getEntity (crates/campaign/capabilities/get-entity): one element's sheet, as the
  // Capability answers it — the lists `campaign.saveEntity` writes, so a save is read back. A PC or
  // an NPC has its stats with their modifier computed from the score (AbilityModifier@1), its token
  // (`tokenImage`: a dedicated image) and its D&D Beyond link; a place or an event has its kind
  // (`lieu`, `evenement`), aliases, description and notes alone. An unknown element is `null`.
  'campaign.getEntity': async (server, { id }) => {
    const lists = [['campaign.listCharacters', null], ['campaign.listPlaces', 'lieu'], ['campaign.listEvents', 'evenement']];
    for (const [name, kind] of lists) {
      const found = ((await server.load(name)) || []).find((e) => e.id === id);
      if (!found) continue;
      const base = { id: found.id, name: found.name, aliases: found.aliases || [], kind: kind || found.kind, description: found.description || '', notes: found.notes || '' };
      if (kind) return base;
      const modifiers = Object.fromEntries(Object.entries(found.stats).map(([ability, score]) => [ability, Math.floor((score - 10) / 2)]));
      const initials = found.initials || found.name.split(/\s+/).map((w) => w.charAt(0)).join('').slice(0, 2).toUpperCase();
      return { ...base, player: found.kind === 'pj' ? found.player || null : null, prompt: found.prompt || '', background: found.background || '', behavior: found.behavior || '', stats: found.stats, modifiers, ca: found.ca, pvMax: found.pvMax, features: found.features || [], color: found.color || 'accent', initials, portrait: Boolean(found.portrait), tokenImage: Boolean(found.token || found.tokenImage), crop: found.crop || null, ddb: found.ddb && found.ddb.url ? found.ddb : null };
    }
    return null;
  },
};
