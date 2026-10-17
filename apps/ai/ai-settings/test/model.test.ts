import assert from 'node:assert/strict';
import { test } from 'node:test';
import { binaryModels, commandBody, initialForm, parseSettings, payload, problem, reasons, settled, withBinary, withMode } from '../src/model.ts';
import type { Form } from '../src/types.ts';

const home = { label: 'Endpoint opencode du serveur maison', url: 'http://192.168.1.40:4096', generationModel: 'qwen-big', summaryModel: 'qwen-small', key: '' };
const cloud = { label: 'API cloud', url: 'https://api.anthropic.com/v1', generationModel: 'sonnet', summaryModel: 'haiku', key: 'sk-ant-••••3f9c' };
const answer = { provider: 'opencode', providers: { cloud, opencode: home }, lastTest: null, local: ['a'], sent: ['b'] };

test('an empty answer is an empty card in API mode, not an error', () => {
  const settings = parseSettings(undefined);
  assert.equal(settings.mode, 'api');
  assert.equal(settings.provider, null);
  assert.deepEqual(initialForm(settings), { mode: 'api', binary: 'opencode', url: '', key: '', generationModel: '', summaryModel: '' });
});

test('the card opens on the active provider, its key left out of the form', () => {
  const form = initialForm(parseSettings(answer));
  assert.deepEqual(form, { mode: 'api', binary: 'opencode', url: home.url, key: '', generationModel: 'qwen-big', summaryModel: 'qwen-small' });
});

test('a saved binary opens the card in binary mode, on its own models', () => {
  const settings = parseSettings({ ...answer, mode: 'binary', provider: 'claude-code', binary: { binary: 'claude-code', generationModel: 'g', summaryModel: 's' } });
  assert.deepEqual(initialForm(settings), { mode: 'binary', binary: 'claude', url: '', key: '', generationModel: 'g', summaryModel: 's' });
});

test('a binary brings its own models, else those of the API provider it speaks for', () => {
  const settings = parseSettings(answer);
  assert.deepEqual(binaryModels(settings, 'claude'), { generationModel: 'sonnet', summaryModel: 'haiku' });
  assert.deepEqual(binaryModels(settings, 'opencode'), { generationModel: 'qwen-big', summaryModel: 'qwen-small' });
  const own = parseSettings({ ...answer, binaries: { 'claude-code': { generationModel: 'cc-g', summaryModel: 'cc-s' } } });
  assert.deepEqual(binaryModels(own, 'claude'), { generationModel: 'cc-g', summaryModel: 'cc-s' });
});

test('switching mode and binary swaps the models, and back to API keeps what was typed', () => {
  const settings = parseSettings(answer);
  const typed: Form = { ...initialForm(settings), url: 'http://x', generationModel: 'mine' };
  const binary = withMode(settings, typed, 'binary');
  assert.equal(binary.mode, 'binary');
  assert.equal(binary.generationModel, 'qwen-big');
  assert.equal(withBinary(settings, binary, 'claude').generationModel, 'sonnet');
  assert.equal(withMode(settings, binary, 'api').url, 'http://x');
});

test('a URL is checked for being one, never for its API type', () => {
  const form = initialForm(parseSettings(answer));
  assert.equal(problem(form), '');
  assert.equal(problem({ ...form, url: 'https://inconnu.example.org/pas-une-api' }), '');
  assert.match(problem({ ...form, url: 'not a url' }), /URL d’API valide/);
  assert.match(problem({ ...form, url: 'ftp://host' }), /URL d’API valide/);
  assert.match(problem({ ...form, generationModel: ' ' }), /génération/);
  assert.match(problem({ ...form, summaryModel: '' }), /résumé/);
  assert.equal(problem({ ...form, mode: 'binary', url: '' }), '');
});

test('the payload carries the mode’s fields and never an API type', () => {
  const form: Form = { mode: 'api', binary: 'claude', url: ' http://h ', key: 'k', generationModel: 'g', summaryModel: 's' };
  assert.deepEqual(payload(form), { mode: 'api', url: 'http://h', key: 'k', generationModel: 'g', summaryModel: 's' });
  assert.deepEqual(payload({ ...form, mode: 'binary' }), { mode: 'binary', binary: 'claude', generationModel: 'g', summaryModel: 's' });
});

test('a command names its DataCapability by identifier and version', () => {
  assert.deepEqual(commandBody('ai.testConnection', { mode: 'api' }, []), {
    dataCapability: 'ai.testConnection@1',
    payload: { mode: 'api' },
    caller: { by: 'dm', from: 'AiSettings', roles: [] },
  });
});

test('the server’s answers are read for their status and violations', () => {
  assert.deepEqual(settled({ commandId: 'c', result: { status: 'rejected', violations: ['api-type-recognized'] } }), { status: 'rejected', violations: ['api-type-recognized'] });
  assert.deepEqual(settled(null), { status: '', violations: [] });
  assert.equal(reasons(['api-type-recognized', 'unknown-id']), 'type d’API non reconnu, unknown-id');
});
