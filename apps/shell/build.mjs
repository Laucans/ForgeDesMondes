// The bundle step of `npm run build`, after `tsc` has checked the sources:
// the composer and the shell's stylesheet into dist/client/, and every
// Micro-UI of apps/<system>/<micro-ui>/ (its entry src/index.ts or
// src/index.js) into dist/micro-ui/<MicroUi>.js, named by its manifest.

import { build } from 'esbuild';
import { existsSync } from 'node:fs';
import { cp, mkdir, readdir, readFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';

const shell = resolve(import.meta.dirname);
const apps = resolve(shell, '..');
const shared = { bundle: true, format: 'esm', target: 'es2022', logLevel: 'warning' };

await build({ ...shared, entryPoints: [join(shell, 'src/client/composer.ts')], outfile: join(shell, 'dist/client/composer.js') });
await mkdir(join(shell, 'dist/client'), { recursive: true });
await cp(join(shell, 'src/client/shell.css'), join(shell, 'dist/client/shell.css'));

let microUis = 0;
for (const system of await readdir(apps, { withFileTypes: true })) {
  if (!system.isDirectory() || system.name === 'shell' || system.name === 'screens') continue;
  for (const folder of await readdir(join(apps, system.name), { withFileTypes: true })) {
    const manifest = join(apps, system.name, folder.name, 'micro-ui.json');
    if (!folder.isDirectory() || !existsSync(manifest)) continue;
    const { microUi } = JSON.parse(await readFile(manifest, 'utf8'));
    const entry = ['index.ts', 'index.js'].map((name) => join(apps, system.name, folder.name, 'src', name)).find((path) => existsSync(path));
    if (!entry) throw new Error(`${manifest}: no src/index.ts or src/index.js to bundle`);
    await build({ ...shared, entryPoints: [entry], outfile: join(shell, 'dist/micro-ui', `${microUi}.js`) });
    microUis += 1;
  }
}
console.log(`shell: composer bundled, ${microUis} Micro-UI(s) bundled`);
