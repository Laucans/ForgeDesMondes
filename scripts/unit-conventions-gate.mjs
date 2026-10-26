// The gates `data-capability-conventions`, `capability-conventions` and
// `screen-surfaces` of .github/workflows/gates.yml: the conventions a unit
// drifts from when it copies a neighbour, judged statically on the units a
// pull request adds or changes — never on the units it leaves alone, which
// predate the gate.
//
//   node scripts/unit-conventions-gate.mjs data-capabilities <base sha>
//   node scripts/unit-conventions-gate.mjs capabilities <base sha>
//   node scripts/unit-conventions-gate.mjs screens <base sha> [--warn] [--all]
//
// A unit is added when the change adds its manifest, changed when it
// touches any file of its folder (a Capability: or its persisted query).
// Each rule says on which it is judged: the rules every existing unit
// already keeps judge both; the rules some older units break judge the
// added ones only, so editing an old unit never turns red for its past.
//
// DataCapability (crates/<system>/data-capabilities/<name>/):
//   clock (changed)        no SystemTime, Instant::now, chrono, Utc::now,
//                          Local::now in src/ outside tests: time comes
//                          from the engine (context.now_millis, FixedClock)
//   id-constant (changed)  `pub const DATA_CAPABILITY: &str = "<id>";` in
//                          src/lib.rs, the manifest's id
//   ports (changed)        the manifest's `ports` are exactly the ports-*
//                          crates of [dependencies]
//   manifest-planted (added) a test plants the real manifest,
//                          include_str!("../data-capability.json")
//   sql-tested (added)     src/ that reads SQL (`.rows(`) runs it on
//                          PostgreSQL in a test, through the test kit's
//                          Scratch or the guard, named by path
//                          (platform_test_kit::…Scratch,
//                          platform_scratch_db::, ScratchDb::create) in
//                          code, never a comment: over memory
//                          `context.rows` is None, a broken query passes
//   legacy (added)         no `pub fn names(` nor `pub const MANIFEST`
// Capability (crates/<system>/capabilities/<name>/), hostless ones aside:
//   query-file-named (added) reads is ["queries/<folder>.graphql"]
//   no-rust-refilter (added) answer() reads no variable the SQL binds from
//                          its variables argument (`<param>.get("x")`,
//                          `<param>["x"]`; a `_`-named one reads none): the
//                          filter is the SQL's, once. An output key that
//                          shares a variable's name is no read
//   variables-bound (added) VARIABLES is empty only when input is {}
//   one-select (changed)   SQL starts with SELECT or WITH, holds no `;`
//   binds (changed)        SQL binds exactly $1..$n for the n VARIABLES,
//                          and the query file's `$var:` are VARIABLES, in
//                          order — the registry binds them by position
//   parity (changed)       every field the query file selects is a word
//                          of the SQL, camelCase as snake_case
//   text-compare (changed) each $n is compared as text,
//                          `<column>::text = $n` (or `= lower($n)`)
//
// Both kinds:
//   holes (changed)        no `SCAFFOLD:` left in a file of the unit's
//                          folder (a Capability: or its query file) — the
//                          skeleton's holes, every one filled before merge
//
// Screens (apps/screens/<screen>.json), caller model A — the shell sets the
// caller of every command from its screen's `surface` (contract E), a
// surface of `surfaces.json` (contract S). Nothing to judge while the
// product declares no surfaces.json. An error only on a file the change
// touches and can fix; a warning otherwise:
//   surface-declared (added or changed screen) the screen declares `surface`
//   surface-known (changed screen, surfaces.json changed) its `surface` and
//                          `pilot.surface` are surfaces of surfaces.json
//   player-review-path (a warning, always) every action of every Micro-UI
//                          the screen lays out writes only critical fields
//                          whose review role (contract I `review`, by
//                          default role:approver) its surface or its pilot's
//                          holds — else the command parks with no screen to
//                          decide it; judged when the screen, a Micro-UI it
//                          lays out, one of their DataCapabilities or
//                          aggregates, or surfaces.json changed. An action
//                          not built yet is said, not judged. A warning: a
//                          read-only placement of a Micro-UI renders none of
//                          its actions, which the manifests cannot say — the
//                          journeys prove it at run time
//   permissions-held (changed screen; a warning when only its units
//                          changed) the permissions of every DataCapability
//                          its Micro-UIs act on are its surface's (or its
//                          pilot's)
//   reviews-held (surfaces.json changed; a warning on a changed
//                          aggregate.json) every review role an aggregate's
//                          critical fields ask is held by some surface
//   page-surface (mock/pages.json changed) a page's `surface`, when set, is
//                          a surface of surfaces.json and its screen's
// --warn says every finding as a warning (a milestone's pull request: its
// tasks were judged on their own); --all judges every screen, aggregate and
// page as changed, whatever the base (an owner's audit).
//
// Reads the files as text, runs none of them. Every finding is reported
// (`::error file=…::<rule>: …`), the verdict comes last. Installed by
// `harness init-repo`; the repository owns it from here.

import { execFileSync } from 'node:child_process';
import * as fs from 'node:fs';
import { posix } from 'node:path';

const [kind, base] = process.argv.slice(2);
const KINDS = {
  'data-capabilities': 'data-capability.json',
  capabilities: 'capability.json',
};
if (!KINDS[kind] && kind !== 'screens') {
  process.stdout.write(`usage: node scripts/unit-conventions-gate.mjs data-capabilities|capabilities <base sha> | screens <base sha> [--warn] [--all]\n`);
  process.exit(2);
}

let failed = 0;
const error = (file, rule, message) => {
  failed += 1;
  process.stdout.write(`::error file=${file}::${rule}: ${message}\n`);
};

const git = (...args) => execFileSync('git', args, { encoding: 'utf8' });
const reachable = (sha) => {
  try {
    git('cat-file', '-e', sha);
    return true;
  } catch {
    return false;
  }
};
const read = (path) => (fs.existsSync(path) ? fs.readFileSync(path, 'utf8') : null);
const json = (path) => {
  try {
    return JSON.parse(read(path));
  } catch {
    return null;
  }
};
if (kind === 'screens') process.exit(screens(process.argv.slice(3)));
if (!base || !reachable(base)) {
  process.stdout.write('no base to compare with: nothing to judge\n');
  process.exit(0);
}
const diff = (filter) => git('diff', '--name-only', `--diff-filter=${filter}`, `${base}...HEAD`)
  .split('\n')
  .filter(Boolean);


/** The `.rs` files under `dir`, recursively, as paths from the root. */
function rustFiles(dir) {
  if (!fs.existsSync(dir)) return [];
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const path = posix.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...rustFiles(path));
    else if (entry.name.endsWith('.rs')) out.push(path);
  }
  return out.sort();
}

/** A Rust file's text before its first `#[cfg(test)]`: the shipped code. */
const shipped = (text) => {
  const at = text.indexOf('#[cfg(test)]');
  return at < 0 ? text : text.slice(0, at);
};

/** Rust text with its `//` comments blanked, line numbers kept. */
const uncommented = (text) => (text || '').replace(/(^|[ \t])\/\/.*$/gm, '$1');

/** A test reaching PostgreSQL through the kit's Scratch or the guard, named by path. */
const SCRATCH = /platform_test_kit::(?:\{[^}]*\b)?Scratch\b|platform_scratch_db::|ScratchDb::create/;

/** `camelCase` as `camel_case`. */
const snake = (name) => name.replace(/([a-z0-9])([A-Z])/g, '$1_$2').toLowerCase();

/** The unit folders of `kind` the paths fall in, as `crates/<system>/<kind>/<name>`. */
function unitsOf(paths) {
  const pattern = new RegExp(`^(crates/[^/]+/${kind}/[^/]+)/`);
  const dirs = new Set();
  for (const path of paths) {
    const found = pattern.exec(path);
    if (found) dirs.add(found[1]);
  }
  return dirs;
}

const added = new Set(diff('A'));
const changed = diff('AMR');
const addedUnits = new Set([...unitsOf(added)].filter((dir) => added.has(`${dir}/${KINDS[kind]}`)));
const touched = unitsOf(changed);
if (kind === 'capabilities') {
  // A persisted query changed is its Capability changed.
  const queries = changed.filter((path) => /^crates\/[^/]+\/queries\/[^/]+$/.test(path));
  for (const query of queries) {
    const system = query.split('/')[1];
    const root = `crates/${system}/capabilities`;
    if (!fs.existsSync(root)) continue;
    for (const name of fs.readdirSync(root)) {
      const manifest = json(`${root}/${name}/capability.json`);
      const reads = Array.isArray(manifest?.reads) ? manifest.reads : [];
      if (reads.some((r) => typeof r === 'string' && query.endsWith(r.replace(/^\.?\//, '')))) {
        touched.add(`${root}/${name}`);
      }
    }
  }
}
const judged = [...new Set([...addedUnits, ...touched])]
  .filter((dir) => fs.existsSync(`${dir}/${KINDS[kind]}`))
  .sort();
if (judged.length === 0) {
  process.stdout.write(`no ${kind} added or changed: nothing to judge\n`);
  process.exit(0);
}

for (const dir of judged) {
  const isAdded = addedUnits.has(dir);
  holes(dir);
  if (kind === 'data-capabilities') dataCapability(dir, isAdded);
  else capability(dir, isAdded);
}

process.stdout.write(`${judged.length} unit(s) judged (${judged.join(', ')}): ${failed === 0 ? 'every convention holds' : `${failed} finding(s)`}\n`);
process.exit(failed === 0 ? 0 : 1);

/** Every file under `dir`, recursively, build output aside. */
function filesUnder(dir) {
  if (!fs.existsSync(dir)) return [];
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const path = posix.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== 'target' && entry.name !== 'node_modules') out.push(...filesUnder(path));
    } else if (entry.isFile()) out.push(path);
  }
  return out.sort();
}

/** A skeleton's hole left in the unit's folder, or in a Capability's query file. */
function holes(dir) {
  const files = filesUnder(dir);
  if (kind === 'capabilities') {
    const system = dir.split('/')[1];
    const manifest = json(`${dir}/capability.json`);
    const reads = Array.isArray(manifest?.reads) ? manifest.reads : [];
    const queries = [`queries/${dir.split('/').pop()}.graphql`, ...reads.filter((r) => typeof r === 'string')]
      .map((r) => `crates/${system}/${r.replace(/^\.?\//, '')}`)
      .filter((path) => fs.existsSync(path) && !files.includes(path));
    files.push(...new Set(queries));
  }
  for (const path of files) {
    const lines = (read(path) || '').split('\n');
    const at = lines.flatMap((line, n) => (line.includes('SCAFFOLD:') ? [n + 1] : []));
    if (at.length > 0) {
      error(path, 'holes', `line(s) ${at.join(', ')} leave a \`SCAFFOLD:\` hole of the skeleton: fill each one, never merge it`);
    }
  }
}

function dataCapability(dir, isAdded) {
  const manifestPath = `${dir}/data-capability.json`;
  const manifest = json(manifestPath);
  if (!manifest) {
    error(manifestPath, 'manifest', 'is not JSON');
    return;
  }
  const src = rustFiles(`${dir}/src`).map((path) => ({ path, code: shipped(read(path)) }));
  const tests = rustFiles(`${dir}/tests`).map((path) => uncommented(read(path))).join('\n');
  for (const { path, code } of src) {
    code.split('\n').forEach((line, n) => {
      if (/SystemTime|Instant::now|chrono::|Utc::now|Local::now/.test(line)) {
        error(path, 'clock', `line ${n + 1} reads a clock — time comes from the engine, context.now_millis (FixedClock in tests)`);
      }
    });
  }
  const lib = read(`${dir}/src/lib.rs`) || '';
  const id = manifest.dataCapability;
  if (!lib.includes(`pub const DATA_CAPABILITY: &str = "${id}";`)) {
    error(`${dir}/src/lib.rs`, 'id-constant', `declares no \`pub const DATA_CAPABILITY: &str = "${id}";\`, the manifest's id its Prepare is registered under`);
  }
  const declared = (Array.isArray(manifest.ports) ? manifest.ports : []).map(String).sort();
  const cargo = read(`${dir}/Cargo.toml`) || '';
  const dependencies = (/^\[dependencies\]\s*$([\s\S]*?)(?=^\[|(?![\s\S]))/m.exec(cargo) || [, ''])[1];
  const crates = [...dependencies.matchAll(/^(ports-[a-z0-9-]+)\s*=/gm)].map((m) => m[1].replace(/^ports-/, '')).sort();
  if (JSON.stringify(declared) !== JSON.stringify(crates)) {
    error(`${dir}/Cargo.toml`, 'ports', `the manifest's ports [${declared.join(', ')}] are not the ports-* crates of [dependencies] [${crates.join(', ')}]`);
  }
  if (!isAdded) return;
  if (!/include_str!\(\s*"(?:\.\.\/)+data-capability\.json"\s*\)/.test(tests)) {
    error(`${dir}/tests`, 'manifest-planted', 'no test plants the real manifest, include_str!("../data-capability.json") beside its aggregate (platform_test_kit::Bench)');
  }
  if (src.some(({ code }) => code.includes('.rows(')) && !SCRATCH.test(tests)) {
    error(`${dir}/tests`, 'sql-tested', 'src/ reads SQL (`.rows(`) and no test runs it on PostgreSQL (platform_test_kit::Scratch): over memory `context.rows` is None, a broken query passes green');
  }
  for (const { path, code } of src) {
    if (/pub fn names\(|pub const MANIFEST\b/.test(code)) {
      error(path, 'legacy', 'holds a pre-plugin item, `pub fn names(` or `pub const MANIFEST`: dead code a neighbour carried, never written in a new unit');
    }
  }
}

/** The raw text of `pub const SQL`, or null. */
function sqlOf(code) {
  const raw = /pub const SQL: &str =\s*r(#*)"([\s\S]*?)"\1\s*;/.exec(code);
  if (raw) return raw[2];
  const plain = /pub const SQL: &str =\s*"((?:[^"\\]|\\.)*)"\s*;/.exec(code);
  return plain ? plain[1] : null;
}

/** `pub const VARIABLES`'s names, or null. */
function variablesOf(code) {
  const found = /pub const VARIABLES: &\[&str\] =\s*&\[([\s\S]*?)\]\s*;/.exec(code);
  return found ? [...found[1].matchAll(/"([^"]*)"/g)].map((m) => m[1]) : null;
}

/** The body of `pub fn answer(`, braces included, or null. */
function answerBody(code) {
  const at = code.indexOf('pub fn answer(');
  if (at < 0) return null;
  const open = code.indexOf('{', code.indexOf(')', at));
  if (open < 0) return null;
  let depth = 0;
  for (let i = open; i < code.length; i += 1) {
    const c = code[i];
    if (c === '"' || (c === 'r' && /^r#*"/.test(code.slice(i, i + 8)) && !/\w/.test(code[i - 1] || ''))) {
      const hashes = c === 'r' ? /^r(#*)"/.exec(code.slice(i))[1] : null;
      const start = c === 'r' ? i + hashes.length + 2 : i + 1;
      if (hashes !== null) {
        const end = code.indexOf(`"${hashes}`, start);
        i = end < 0 ? code.length : end + hashes.length;
      } else {
        let end = start;
        while (end < code.length && code[end] !== '"') end += code[end] === '\\' ? 2 : 1;
        i = end;
      }
      continue;
    }
    if (c === '\'' && (code[i + 2] === '\'' || (code[i + 1] === '\\' && code[i + 3] === '\''))) {
      i += code[i + 1] === '\\' ? 3 : 2;
      continue;
    }
    if (c === '{') depth += 1;
    if (c === '}') {
      depth -= 1;
      if (depth === 0) return code.slice(open, i + 1);
    }
  }
  return code.slice(open);
}

/**
 * The request variables `pub fn answer(` reads from its variables argument
 * (its second parameter): `<param>.get("x")`, `<param>["x"]`. None when that
 * parameter is `_`-named, which reads nothing; an output key is no read.
 */
function variableReads(code) {
  const signature = /pub fn answer\s*\(\s*(?:mut\s+)?\w+\s*:[^,]*,\s*(?:mut\s+)?(\w+)\s*:/.exec(code);
  if (!signature || signature[1].startsWith('_')) return [];
  const body = uncommented(answerBody(code));
  const param = signature[1];
  const access = new RegExp(`\\b${param}\\s*(?:\\.\\s*get\\s*\\(\\s*&?\\s*"([^"]*)"|\\[\\s*"([^"]*)"\\s*\\])`, 'g');
  return [...body.matchAll(access)].map((m) => m[1] ?? m[2]);
}

function capability(dir, isAdded) {
  const manifestPath = `${dir}/capability.json`;
  const manifest = json(manifestPath);
  if (!manifest) {
    error(manifestPath, 'manifest', 'is not JSON');
    return;
  }
  if (manifest.host === 'hostless') return;
  const folder = dir.split('/').pop();
  const system = dir.split('/')[1];
  const libPath = `${dir}/src/lib.rs`;
  const code = shipped(read(libPath) || '');
  const sql = sqlOf(code);
  const variables = variablesOf(code);
  if (sql === null || variables === null) {
    error(libPath, 'one-select', 'declares no `pub const SQL: &str` and `pub const VARIABLES: &[&str]` the gate can read');
    return;
  }
  const reads = Array.isArray(manifest.reads) ? manifest.reads : [];
  const queryPath = reads
    .filter((r) => typeof r === 'string')
    .flatMap((r) => [r, `${dir}/${r}`, `crates/${system}/${r}`])
    .find((p) => fs.existsSync(p));
  if (isAdded) {
    const wanted = `queries/${folder}.graphql`;
    if (JSON.stringify(reads) !== JSON.stringify([wanted])) {
      error(manifestPath, 'query-file-named', `reads ${JSON.stringify(reads)}: a Capability reads one query file named after its folder, ["${wanted}"]`);
    }
    const readVariables = variableReads(code);
    for (const variable of variables) {
      if (readVariables.includes(variable)) {
        error(libPath, 'no-rust-refilter', `answer() reads "${variable}", a variable the SQL binds: the filter is the SQL's, once — answer() maps the rows it already filtered`);
      }
    }
    const inputs = manifest.input && typeof manifest.input === 'object' ? Object.keys(manifest.input) : [];
    if (variables.length === 0 && inputs.length > 0) {
      error(libPath, 'variables-bound', `VARIABLES is empty while the input is {${inputs.join(', ')}}: a filtering input is bound in the SQL, never applied in Rust over every row`);
    }
  }
  const upper = sql.trim().toUpperCase();
  if (!(upper.startsWith('SELECT') || upper.startsWith('WITH')) || sql.includes(';')) {
    error(libPath, 'one-select', 'SQL is not one read statement: it starts with SELECT or WITH and holds no `;`');
  }
  const bound = [...new Set([...sql.matchAll(/\$(\d+)/g)].map((m) => Number(m[1])))].sort((a, b) => a - b);
  const expected = variables.map((_, n) => n + 1);
  if (JSON.stringify(bound) !== JSON.stringify(expected)) {
    error(libPath, 'binds', `SQL binds [${bound.map((n) => `$${n}`).join(', ')}] for VARIABLES [${variables.join(', ')}]: exactly $1..$${variables.length}, in their order`);
  }
  for (const n of expected) {
    if (!new RegExp(`::text\\s*=\\s*(?:lower\\(\\s*)?\\$${n}(?!\\d)`).test(sql)) {
      error(libPath, 'text-compare', `$${n} (${variables[n - 1]}) is not compared as text, \`<column>::text = $${n}\`: an id that is no uuid then fails instead of answering empty`);
    }
  }
  if (!queryPath || !queryPath.endsWith('.graphql')) return;
  const query = read(queryPath);
  const lines = query.split('\n');
  const start = lines.findIndex((line) => /^\s*query\b/.test(line));
  if (start < 0) return;
  const declared = [...lines[start].matchAll(/\$(\w+)\s*:/g)].map((m) => m[1]);
  if (JSON.stringify(declared) !== JSON.stringify(variables)) {
    error(queryPath, 'binds', `the query's variables [${declared.join(', ')}] are not VARIABLES [${variables.join(', ')}], in order`);
  }
  const words = new Set(sql.split(/[^A-Za-z0-9_]+/).filter(Boolean));
  for (const line of lines.slice(start + 1)) {
    const field = line.trim();
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(field)) continue;
    if (!words.has(snake(field))) {
      error(queryPath, 'parity', `selects \`${field}\` and the SQL has no \`${snake(field)}\`: the query file declares what the SQL really reads`);
    }
  }
}


/** The JSON files under `dir` named `name`, recursively, build output aside, as paths from the root. */
function manifestsNamed(dir, name) {
  return filesUnder(dir).filter((path) => posix.basename(path) === name);
}

/** The Micro-UIs a screen's layout names: an id, `{ "microUi": … }`, or keyed `{ "<Name>": {…} }`. */
function laidOut(layout) {
  const names = [];
  for (const entry of Array.isArray(layout) ? layout : []) {
    if (typeof entry === 'string') names.push(entry);
    else if (entry && typeof entry === 'object') {
      if (typeof entry.microUi === 'string') names.push(entry.microUi);
      else names.push(...Object.keys(entry));
    }
  }
  return [...new Set(names)];
}

/** The gate `screen-surfaces`: who acts on each screen, and whether its commands find their review roles there. */
function screens(args) {
  const warnOnly = args.includes('--warn');
  const all = args.includes('--all');
  const [since] = args.filter((a) => !a.startsWith('--'));
  if (!fs.existsSync('surfaces.json')) {
    process.stdout.write('no surfaces.json: nothing to judge\n');
    return 0;
  }
  let findings = 0;
  const say = (level, file, rule, message) => {
    if (level === 'error' && !warnOnly) findings += 1;
    process.stdout.write(`::${level === 'error' && !warnOnly ? 'error' : 'warning'} file=${file}::${rule}: ${message}\n`);
  };
  const declared = json('surfaces.json');
  if (!declared || !declared.surfaces || typeof declared.surfaces !== 'object' || Array.isArray(declared.surfaces)) {
    say('error', 'surfaces.json', 'surface-known', 'is not a surfaces declaration (contract S): `{ "default", "surfaces": { "<name>": { "by", "roles" } } }`');
    return findings ? 1 : 0;
  }
  if (!all && (!since || !reachable(since))) {
    process.stdout.write('no base to compare with: nothing to judge\n');
    return 0;
  }
  const changedFiles = all ? null : new Set(git('diff', '--name-only', '--diff-filter=AMR', `${since}...HEAD`).split('\n').filter(Boolean));
  const addedFiles = all ? null : new Set(git('diff', '--name-only', '--diff-filter=A', `${since}...HEAD`).split('\n').filter(Boolean));
  const changed = (path) => all || changedFiles.has(path);
  const added = (path) => all || addedFiles.has(path);
  const surfaces = declared.surfaces;
  const known = (name) => typeof name === 'string' && Object.hasOwn(surfaces, name);
  const held = (name, what) => (known(name) && Array.isArray(surfaces[name][what]) ? surfaces[name][what].filter((x) => typeof x === 'string') : []);

  const byField = (paths, field) => {
    const out = new Map();
    for (const path of paths) {
      const manifest = json(path);
      const id = manifest && manifest[field];
      if (typeof id === 'string' && !out.has(id)) out.set(id, { path, manifest });
    }
    return out;
  };
  const microUis = byField(manifestsNamed('apps', 'micro-ui.json'), 'microUi');
  const dataCapabilities = byField(manifestsNamed('crates', 'data-capability.json').filter((p) => /^crates\/[^/]+\/data-capabilities\//.test(p)), 'dataCapability');
  const aggregates = byField(manifestsNamed('crates', 'aggregate.json').filter((p) => /^crates\/[^/]+\/aggregates\//.test(p)), 'aggregate');
  const screenFiles = fs.existsSync('apps/screens') ? fs.readdirSync('apps/screens').filter((n) => n.endsWith('.json')).map((n) => `apps/screens/${n}`).sort() : [];
  const unversioned = (id) => String(id).split('@')[0];

  /** The critical fields an action writes, each with the review role it asks: `[]` when nothing is critical, `null` when it is not built. */
  const asks = (action) => {
    const dc = dataCapabilities.get(unversioned(action));
    if (!dc) return null;
    const target = dc.manifest.target && dc.manifest.target.aggregate;
    const aggregate = aggregates.get(target);
    const touches = Array.isArray(dc.manifest.touches) ? dc.manifest.touches.filter((t) => typeof t === 'string') : [];
    const fields = touches.length ? touches.map((t) => t.split('.').pop()) : Object.keys(dc.manifest.payload || {});
    const declaredFields = (aggregate && aggregate.manifest.fields) || {};
    return {
      dc,
      aggregate,
      critical: fields.filter((f) => Object.hasOwn(declaredFields, f) && declaredFields[f] && declaredFields[f].criticality === 'critical').map((f) => ({ field: f, role: declaredFields[f].review || 'role:approver' })),
    };
  };

  let judged = 0;
  for (const path of screenFiles) {
    const screen = json(path);
    if (!screen) continue;
    const name = screen.screen || posix.basename(path, '.json');
    const microUiNames = laidOut(screen.layout);
    const actions = microUiNames.flatMap((m) => {
      const found = microUis.get(m);
      return found ? (Array.isArray(found.manifest.actions) ? found.manifest.actions : []).map((a) => ({ microUi: m, action: a, file: found.path })) : [];
    });
    const unitFiles = [
      ...microUiNames.map((m) => microUis.get(m)).filter(Boolean).map((m) => m.path),
      ...actions.flatMap(({ action }) => {
        const a = asks(action);
        return a ? [a.dc.path, ...(a.aggregate ? [a.aggregate.path] : [])] : [];
      }),
    ];
    const screenChanged = changed(path);
    const unitsChanged = unitFiles.some((f) => changed(f));
    const surfacesChanged = changed('surfaces.json');
    if (!screenChanged && !unitsChanged && !surfacesChanged) continue;
    judged += 1;
    const surface = screen.surface;
    const pilot = screen.pilot && typeof screen.pilot === 'object' ? screen.pilot.surface : undefined;
    if ((added(path) || screenChanged) && typeof surface !== 'string') {
      say('error', path, 'surface-declared', `screen ${name} declares no \`surface\`: the shell then sends no role for it (fail closed) — name the surface of surfaces.json whose user acts on it (${Object.keys(surfaces).join(', ')})`);
    }
    for (const [what, value] of [['surface', surface], ['pilot.surface', pilot]]) {
      if (value !== undefined && !known(value) && (screenChanged || surfacesChanged)) {
        say('error', screenChanged ? path : 'surfaces.json', 'surface-known', `screen ${name}'s ${what} ${JSON.stringify(value)} is no surface of surfaces.json (${Object.keys(surfaces).join(', ')})`);
      }
    }
    if (typeof surface !== 'string') continue;
    const roles = new Set([...held(surface, 'roles'), ...held(pilot, 'roles')]);
    const permissions = new Set([...held(surface, 'permissions'), ...held(pilot, 'permissions')]);
    const on = pilot ? `surface ${surface}, pilot ${pilot}` : `surface ${surface}`;
    const missing = new Map();
    for (const { microUi, action } of actions) {
      const a = asks(action);
      if (!a) {
        process.stdout.write(`screen ${name}: ${microUi}'s action ${action} is not built yet: not judged\n`);
        continue;
      }
      for (const { field, role } of a.critical) {
        if (roles.has(role)) continue;
        const key = `${microUi}|${role}`;
        if (!missing.has(key)) missing.set(key, { microUi, role, actions: new Set(), fields: new Set() });
        missing.get(key).actions.add(unversioned(action));
        missing.get(key).fields.add(`${a.aggregate ? a.aggregate.manifest.aggregate : '?'}.${field}`);
      }
      const asked = Array.isArray(a.dc.manifest.permissions) ? a.dc.manifest.permissions.filter((p) => typeof p === 'string') : [];
      const lacking = asked.filter((p) => !permissions.has(p));
      if (lacking.length) {
        say(screenChanged || surfacesChanged ? 'error' : 'warning', screenChanged ? path : surfacesChanged ? 'surfaces.json' : a.dc.path, 'permissions-held', `screen ${name} (${on}) lays out ${microUi}, whose action ${unversioned(action)} asks the permission(s) ${lacking.join(', ')} its surface does not hold: the server refuses it`);
      }
    }
    for (const { microUi, role, actions: names, fields } of missing.values()) {
      say('warning', path, 'player-review-path', `screen ${name} (${on}) lays out ${microUi}, whose action(s) ${[...names].join(', ')} write the critical field(s) ${[...fields].join(', ')} asking ${role}, which its surface does not hold: each such command parks in awaiting_review with no screen to decide it — give the role to the surface whose gesture approves (surfaces.json), pilot the screen from one that holds it, or keep those actions off this screen`);
    }
  }

  // Every review role an aggregate asks is held by some surface.
  const anyHolds = (role) => Object.keys(surfaces).some((n) => held(n, 'roles').includes(role));
  for (const { path, manifest } of aggregates.values()) {
    const aggregateChanged = changed(path);
    if (!aggregateChanged && !changed('surfaces.json')) continue;
    const fields = manifest.fields && typeof manifest.fields === 'object' ? manifest.fields : {};
    for (const [field, spec] of Object.entries(fields)) {
      if (!spec || spec.criticality !== 'critical') continue;
      const role = spec.review || 'role:approver';
      if (anyHolds(role)) continue;
      say(changed('surfaces.json') ? 'error' : 'warning', changed('surfaces.json') ? 'surfaces.json' : path, 'reviews-held', `${manifest.aggregate}.${field} asks ${role} for its review, which no surface of surfaces.json holds: nobody can approve it — add the role to the surface whose gesture approves, or keep the default role (role:approver)`);
    }
  }

  // A page's surface, when it says one, is a surface and its screen's.
  if (changed('mock/pages.json')) {
    const pages = json('mock/pages.json');
    for (const page of (pages && Array.isArray(pages.pages) ? pages.pages : [])) {
      if (!page || page.surface === undefined) continue;
      if (!known(page.surface)) {
        say('error', 'mock/pages.json', 'page-surface', `${page.path}'s surface ${JSON.stringify(page.surface)} is no surface of surfaces.json (${Object.keys(surfaces).join(', ')})`);
        continue;
      }
      const screenFile = screenFiles.find((f) => (json(f) || {}).screen === page.screen);
      const screenSurface = screenFile ? (json(screenFile) || {}).surface : undefined;
      if (typeof screenSurface === 'string' && screenSurface !== page.surface) {
        say('error', 'mock/pages.json', 'page-surface', `${page.path} says surface ${page.surface} and its screen ${page.screen} (${screenFile}) says ${screenSurface}: the page follows its screen`);
      }
    }
  }
  process.stdout.write(`${judged} screen(s) judged: ${findings === 0 ? 'every surface holds' : `${findings} finding(s)`}\n`);
  return findings === 0 ? 0 : 1;
}
