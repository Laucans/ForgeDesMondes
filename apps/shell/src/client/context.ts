// The screen's context: what a frozen screen composes its Micro-UIs with
// (contract E). A prop written `$ctx.<name>` reads the context, which a
// Micro-UI sets and the others follow. The same shape as the mock-up's screen
// host (mock/src/screen.js), so a micro-frontend behaves on the shell as it
// does on its page.

export type Props = Record<string, unknown>;
export type ContextListener = (name: string, value: unknown) => void;

const REFERENCE = '$ctx.';

export class Context {
  private readonly values = new Map<string, unknown>();
  private readonly listeners = new Set<ContextListener>();

  /** Sets one value and tells everyone who follows the context; nothing happens when it did not change. */
  set(name: string, value: unknown): void {
    if (this.values.get(name) === value) return;
    this.values.set(name, value);
    for (const listener of [...this.listeners]) listener(name, value);
  }

  /** One value; `undefined` while nothing set it. */
  get(name: string): unknown {
    return this.values.get(name);
  }

  /** Listens to the context; answers how to stop. */
  on(listener: ContextListener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  /** The raw props with every `$ctx.<name>` resolved; `undefined` while the context has no value. */
  resolve(raw: Props): Props {
    const out: Props = {};
    for (const [key, value] of Object.entries(raw)) {
      out[key] = typeof value === 'string' && value.startsWith(REFERENCE) ? this.get(value.slice(REFERENCE.length)) : value;
    }
    return out;
  }

  /** The context names the raw props follow. */
  follows(raw: Props): string[] {
    return Object.values(raw)
      .filter((value): value is string => typeof value === 'string' && value.startsWith(REFERENCE))
      .map((value) => value.slice(REFERENCE.length));
  }
}
