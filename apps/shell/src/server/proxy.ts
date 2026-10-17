// The shell's wire protocol, forwarded to the product's server: the status,
// the headers and the body come back as the server sent them, a stream
// included — nothing is buffered, nothing is answered
// here. The shell never serves the mock server's answers.

import { request as httpRequest, type IncomingMessage, type ServerResponse } from 'node:http';
import { request as httpsRequest } from 'node:https';

/** The paths of the wire protocol: each one, and everything under it. */
export const PROTOCOL = ['/capabilities', '/commands', '/holds', '/reviews', '/dataVersion', '/health'] as const;

export function isProtocol(pathname: string): boolean {
  return PROTOCOL.some((prefix) => pathname === prefix || pathname.startsWith(`${prefix}/`));
}

// Headers that belong to one connection, not to the message (RFC 9110 §7.6.1).
const HOP_BY_HOP = new Set(['connection', 'keep-alive', 'proxy-authenticate', 'proxy-authorization', 'te', 'trailer', 'transfer-encoding', 'upgrade']);

function forwarded(headers: IncomingMessage['headers'], drop: string[] = []): Record<string, string | string[]> {
  const out: Record<string, string | string[]> = {};
  for (const [name, value] of Object.entries(headers)) {
    if (value === undefined || HOP_BY_HOP.has(name) || drop.includes(name)) continue;
    out[name] = value;
  }
  return out;
}

/** Sends `req` to `serverUrl` and pipes the answer into `res`. The request target is only ever a path on that server. */
export function proxy(req: IncomingMessage, res: ServerResponse, serverUrl: URL): void {
  const target = new URL(`${serverUrl.origin}${req.url ?? '/'}`);
  const send = target.protocol === 'https:' ? httpsRequest : httpRequest;
  const upstream = send(
    target,
    { method: req.method, headers: { ...forwarded(req.headers, ['host']), host: target.host } },
    (answer) => {
      res.writeHead(answer.statusCode ?? 502, answer.statusMessage, forwarded(answer.headers));
      answer.pipe(res);
      answer.on('error', () => res.destroy());
    },
  );
  upstream.on('error', () => {
    if (res.headersSent) return res.destroy();
    res.writeHead(502, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ error: 'server-unreachable' }));
  });
  // A client that goes away takes its stream with it.
  res.on('close', () => upstream.destroy());
  req.pipe(upstream);
}
