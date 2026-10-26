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
  private readonly held = new Set<string>();

  /** Sets one value the address named and keeps it: a later `set` of that name changes nothing. */
  hold(name: string, value: unknown): void {
    this.set(name, value);
    this.held.add(name);
  }

  /** Sets one value and tells everyone who follows the context; nothing happens when it did not change, or when the address holds the name. */
  set(name: string, value: unknown): void {
    if (this.held.has(name) || this.values.get(name) === value) return;
    this.values.set(name, value);
    // One follower failing must not keep the others from the change.
    for (const listener of [...this.listeners]) {
      try {
        listener(name, value);
      } catch (error) {
        console.error(`shell: a follower of the context failed on "${name}"`, error);
      }
    }
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
