// What the mock server answers a Capability with, when a fixture as it is
// is not enough: one function per identifier, `(server, variables) =>
// data | undefined` (`undefined` is `not-found`). `server.load(name)` is a
// mutable fixture (`fixtures/<name>.json`); `server.dataVersion` the
// version. A Capability absent here answers `fixtures/<identifier>.json`.
//
//   'campagne.listerPjs': async (server, { campagneId }) =>
//     (await server.load('pjs')).filter((p) => p.campagneId === campagneId),

export const READS = {
  // table.getBattlemapInstance: the fixture's instance, or one a command made.
  'table.getBattlemapInstance': async (server, { instance }) => {
    const fixture = await server.load('table.getBattlemapInstance');
    if (!instance || instance === fixture.id) return fixture;
    return server.instances && server.instances.get(instance);
  },
};
