// What the shell reads from its environment: `PORT` to listen on and
// `SERVER_URL`, where the product's server answers the wire protocol. The
// repository root locates the frozen screens (`apps/screens/`), the contracts
// and the design system's files, which the shell serves as they are.

import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export interface Config {
  port: number;
  serverUrl: URL;
  root: string;
  /** The build output: the bundled composer, the stylesheet, the Micro-UIs. */
  dist: string;
}

/** The repository root, from where the build output sits: apps/shell/dist/tsc/server/. */
export function repositoryRoot(): string {
  return resolve(dirname(fileURLToPath(import.meta.url)), '../../../../..');
}

export function readConfig(env: Record<string, string | undefined>, root: string = repositoryRoot()): Config {
  const port = Number(env.PORT ?? '8791');
  if (!Number.isInteger(port) || port < 0 || port > 65535) throw new Error(`PORT is not a port: ${env.PORT}`);
  const serverUrl = env.SERVER_URL;
  if (!serverUrl) throw new Error('SERVER_URL is not set: the shell proxies the wire protocol to it');
  return { port, serverUrl: new URL(serverUrl), root, dist: resolve(root, 'apps/shell/dist') };
}
