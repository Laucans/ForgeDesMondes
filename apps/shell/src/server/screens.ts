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

export interface Screen {
  screen: string;
  generatedFrom: string;
  version: number;
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
  const frozen = document as { screen: string; generatedFrom: string; version: number; layout: Array<Record<string, Record<string, unknown>>> };
  if (frozen.screen !== name) throw new InvalidScreen(`${name}.json freezes the screen "${frozen.screen}"`);
  return {
    screen: frozen.screen,
    generatedFrom: frozen.generatedFrom,
    version: frozen.version,
    layout: frozen.layout.map((entry) => {
      const [microUi, props] = Object.entries(entry)[0] as [string, Record<string, unknown>];
      return { microUi, props };
    }),
  };
}
