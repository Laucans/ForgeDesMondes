import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync } from 'node:fs';

const CRATES = new URL('../../../../crates/', import.meta.url);

/** The `payload` of the DataCapability's manifest, found by its identifier under crates/<system>/data-capabilities/. */
function manifestOf(dataCapability: string): Record<string, string> {
  for (const system of readdirSync(CRATES)) {
    const folder = new URL(`${system}/data-capabilities/`, CRATES);
    if (!existsSync(folder)) continue;
    for (const unit of readdirSync(folder)) {
      const file = new URL(`${unit}/data-capability.json`, folder);
      if (!existsSync(file)) continue;
      const manifest = JSON.parse(readFileSync(file, 'utf8')) as { dataCapability: string; payload: Record<string, string> };
      if (manifest.dataCapability === dataCapability) return manifest.payload;
    }
  }
  throw new Error(`no manifest declares ${dataCapability}`);
}

/** A command a Micro-UI submits carries only fields its DataCapability's manifest declares, and none of those the engine alone sends. */
export function assertDeclared(command: { dataCapability: string; payload: Record<string, unknown> }): void {
  const id = command.dataCapability.replace(/@\d+$/, '');
  const declared = manifestOf(id);
  for (const field of Object.keys(command.payload)) {
    assert.ok(field in declared, `${id}: « ${field} » is not in the manifest's payload (${Object.keys(declared).join(', ')})`);
    assert.doesNotMatch(declared[field] ?? '', /never sent by the DM/, `${id}: « ${field} » is the command's own, never the DM's`);
  }
}
