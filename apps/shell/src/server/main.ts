// `npm run serve`: the shell on PORT, reaching the server at SERVER_URL.

import { createServer } from 'node:http';
import { createApp } from './app.js';
import { readConfig } from './config.js';

const config = readConfig(process.env);
const server = createServer(await createApp(config));
server.listen(config.port, config.host, () => {
  console.log(`shell: http://${config.host}:${config.port} → ${config.serverUrl.href}`);
});
for (const signal of ['SIGINT', 'SIGTERM'] as const) {
  process.on(signal, () => {
    // The `/dataVersion` stream never ends on its own: close it so the shell can.
    server.close(() => process.exit(0));
    server.closeAllConnections();
  });
}
