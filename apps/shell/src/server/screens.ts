// A frozen screen (contract E): `apps/screens/<screen>.json`, validated
// against its contract and loaded as it is. Nothing here composes, edits or
// completes it — a screen is replayed, never regenerated (rule 10).

import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import Ajv2020 from 'ajv/dist/2020.js';

/** One entry of a layout: the Micro-UI's identifier and the props handed to it. */
export interface Placement {
  microUi: string;
  props: Record<string, unknown>;
}

/** Another surface a screen acts as while its page's URL carries `<query>=1` (contract E `pilot`). */
export interface Pilot {
  surface: string;
  query: string;
}

export interface Screen {
  screen: string;
  generatedFrom: string;
  version: number;
  /** Who acts on the screen: a surface of `surfaces.json` (contract E); `undefined` acts with no role. */
  surface?: string;
  pilot?: Pilot;
  layout: Placement[];
}

const NAME = /^[a-z][a-z0-9-]*$/;

/** Whether `name` can be a screen's name (contract E): the only names that reach the disk. */
export const isScreenName = (name: string): boolean => NAME.test(name);

export type Validate = (document: unknown) => string | undefined;

/** Builds the validator of contract E from its schema; answers why a document breaks it, or nothing. */
export async function contractValidator(root: string): Promise<Validate> {
  const schema: unknown = JSON.parse(await readFile(join(root, 'contracts/e-screen-composition.schema.json'), 'utf8'));
  const validate = new Ajv2020.default({ allErrors: true, strict: false }).compile(schema as object);
  return (document) => (validate(document) ? undefined : (validate.errors ?? []).map((e) => `${e.instancePath || '/'} ${e.message}`).join('; '));
}

export class InvalidScreen extends Error {}

/** `undefined` when no such screen is frozen; throws `InvalidScreen` when its file breaks the contract. */
export async function loadScreen(root: string, name: string, validate: Validate): Promise<Screen | undefined> {
  // The name is a path segment: only what the contract allows reaches the disk.
  if (!isScreenName(name)) return undefined;
  let text: string;
  try {
    text = await readFile(join(root, 'apps/screens', `${name}.json`), 'utf8');
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return undefined;
    throw error;
  }
  let document: unknown;
  try {
    document = JSON.parse(text);
  } catch (error) {
    throw new InvalidScreen(`${name}.json is not JSON: ${(error as Error).message}`);
  }
  const broken = validate(document);
  if (broken) throw new InvalidScreen(`${name}.json breaks contract E: ${broken}`);
  const frozen = document as { screen: string; generatedFrom: string; version: number; surface?: string; pilot?: Pilot; layout: Array<Record<string, Record<string, unknown>>> };
  if (frozen.screen !== name) throw new InvalidScreen(`${name}.json freezes the screen "${frozen.screen}"`);
  return {
    screen: frozen.screen,
    generatedFrom: frozen.generatedFrom,
    version: frozen.version,
    ...(frozen.surface === undefined ? {} : { surface: frozen.surface }),
    ...(frozen.pilot === undefined ? {} : { pilot: frozen.pilot }),
    layout: frozen.layout.map((entry) => {
      const [microUi, props] = Object.entries(entry)[0] as [string, Record<string, unknown>];
      return { microUi, props };
    }),
  };
}

/** The caller a command carries (contract S): who it is by, and the roles and permissions its gestures hold. */
export interface Caller {
  by: string;
  roles: string[];
  permissions: string[];
}

/** Who acts when nothing says otherwise: the players', with no role and no permission (fail closed). */
export const FAIL_CLOSED: Caller = { by: 'player', roles: [], permissions: [] };

const strings = (value: unknown): string[] | undefined =>
  Array.isArray(value) && value.every((item): item is string => typeof item === 'string') ? value : undefined;

/** The surfaces of `surfaces.json` (contract S) by name; empty when the file is missing, unreadable or not a declaration. */
export async function loadSurfaces(root: string): Promise<Record<string, Caller>> {
  const out: Record<string, Caller> = {};
  try {
    const declared = JSON.parse(await readFile(join(root, 'surfaces.json'), 'utf8')) as { surfaces?: Record<string, { by?: unknown; roles?: unknown; permissions?: unknown }> };
    for (const [name, surface] of Object.entries(declared.surfaces ?? {})) {
      const roles = strings(surface?.roles);
      const permissions = surface?.permissions === undefined ? [] : strings(surface.permissions);
      // A surface that is not whole is no surface: whoever acts as it acts with no role.
      if (typeof surface?.by === 'string' && surface.by && roles && permissions) out[name] = { by: surface.by, roles, permissions };
    }
  } catch {
    // Missing or not JSON: no surface is declared.
  }
  return out;
}

/** The caller of a screen's commands: its pilot's surface while the address carries the pilot's query as `1`, else its own; the players' for any doubt. */
export function callerOf(screen: Screen, surfaces: Record<string, Caller>, query: URLSearchParams): Caller {
  const name = screen.pilot && query.get(screen.pilot.query) === '1' ? screen.pilot.surface : screen.surface;
  const found = name !== undefined && Object.hasOwn(surfaces, name) ? surfaces[name] : undefined;
  return found ?? FAIL_CLOSED;
}
