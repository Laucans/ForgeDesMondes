export const commands = {
  // session.reorderScenes (crates/session/data-capabilities/reorder-scenes):
  // the session's scenes in the order the DM gives, every rank rewritten at
  // once, 1 to n. An order that omits a scene of the fixtures' session,
  // repeats one or names one it does not hold is `scene-order-contiguous`,
  // and no rank changes.
  'session.reorderScenes': async (server, { payload }) => {
    const scenes = await server.load('session.listScenes');
    const order = Array.isArray(payload.order) ? payload.order : [];
    const known = new Set(scenes.map((sc) => sc.id));
    if (order.length !== known.size || new Set(order).size !== order.length || order.some((id) => !known.has(id))) return ['scene-order-contiguous'];
    for (const sc of scenes) sc.order = order.indexOf(sc.id) + 1;
    scenes.sort((a, b) => a.order - b.order);
    return undefined;
  },
};
