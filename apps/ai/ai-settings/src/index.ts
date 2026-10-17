// AiSettings (system ai): the DM's generation provider on one card — API or
// binary, the models, the key — with a test of the connection and a save.
// It reads `ai.getSettings` and triggers `ai.testConnection` and
// `ai.saveSettings`, by identifier, through the shell it is handed: no
// network call of its own. A test never saves (`test-does-not-save`): the two
// buttons send two different commands, and only « Enregistrer » sends the
// save.

import { GET_SETTINGS, SAVE_SETTINGS, TEST_CONNECTION } from './ids.ts';
import {
  commandBody,
  commandId,
  initialForm,
  isFinal,
  parseSettings,
  payload,
  problem,
  reasons,
  savedTypeLabel,
  settled,
  testedTypeLabel,
  withBinary,
  withMode,
  type Settled,
} from './model.ts';
import { detected, fields, items, keyPlaceholder, modeChoice, result, skeleton } from './render.ts';
import { STYLE } from './style.ts';
import type { Form, Outcome, Settings, Shell } from './types.ts';

/** How a command is followed until it settles: the server applies it on its own clock. */
export interface Timing {
  sleep(ms: number): Promise<void>;
  everyMs: number;
  /** How many times it asks before saying it has not settled. */
  polls: number;
}

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };

/** The API type last known for a URL: what the card was saved with, or what a test detected. */
interface Known {
  url: string;
  label: string;
}

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="AiSettings"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'AiSettings';
  style.textContent = STYLE;
  doc.head.append(style);
}

/** What the shell calls: fills `host` with the card and wires it. */
export function mount(host: HTMLElement, _props: Record<string, unknown>, shell: Shell, timing: Timing = WAITING): void {
  ensureStyle(host.ownerDocument);
  host.innerHTML = skeleton();

  const part = (id: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(`#${id}`);
    if (!found) throw new Error(`AiSettings: #${id} is not in the card`);
    return found;
  };

  let settings: Settings = parseSettings(undefined);
  let form: Form = initialForm(settings);
  let outcome: Outcome = { kind: 'none' };
  let known: Known | null = null;

  const paintDetected = (): void => {
    const shown = form.mode === 'api' && known !== null && known.url === form.url.trim();
    part('ai-detected').innerHTML = detected(shown ? known?.label ?? null : null);
  };
  const paintResult = (): void => {
    part('ai-test-result').innerHTML = result(outcome);
  };
  const paintFields = (): void => {
    part('ai-fields').innerHTML = fields(form, settings);
    paintDetected();
  };
  const paintAll = (): void => {
    part('ai-provider').innerHTML = modeChoice(form);
    paintFields();
    paintResult();
    part('ai-local-list').innerHTML = items(settings.local);
    part('ai-sent-list').innerHTML = items(settings.sent);
  };
  const say = (next: Outcome): void => {
    outcome = next;
    paintResult();
  };

  /** While the settings load or a command runs, the form holds still: what is sent is what is on screen. */
  const lock = (locked: boolean): void => {
    for (const control of host.querySelectorAll<HTMLInputElement | HTMLSelectElement | HTMLButtonElement>('#ai-settings-form input, #ai-settings-form select, #ai-settings-form button')) control.disabled = locked;
  };

  const read = async (): Promise<Settings> => parseSettings((await shell.capability(GET_SETTINGS, undefined)).data);
  /** Reads the settings again after a command: false when they cannot be read, the command having applied all the same. */
  const refresh = async (): Promise<boolean> => {
    try {
      settings = await read();
      return true;
    } catch {
      return false;
    }
  };

  /** The command is sent, then followed until the server has applied or refused it. */
  async function send(dataCapability: string, roles: string[]): Promise<Settled> {
    const answer = await shell.command(commandBody(dataCapability, payload(form), roles));
    const id = commandId(answer);
    let state = settled(answer);
    for (let asked = 0; !isFinal(state.status) && id !== '' && asked < timing.polls; asked += 1) {
      await timing.sleep(timing.everyMs);
      state = settled(await shell.commandResult(id));
    }
    return state;
  }

  let busy = false;

  /** One command at a time: the form is locked until its end. */
  async function act(wrong: string, run: () => Promise<void>): Promise<void> {
    if (busy) return;
    const why = problem(form);
    if (why) return say({ kind: 'error', text: why });
    busy = true;
    lock(true);
    try {
      await run();
    } catch {
      say({ kind: 'error', text: wrong });
    } finally {
      busy = false;
      lock(false);
    }
  }

  const testConnection = (): Promise<void> =>
    act('Le test de connexion a échoué.', async () => {
      const before = settings.lastTest;
      say({ kind: 'busy', text: 'Test en cours…' });
      const state = await send(TEST_CONNECTION, []);
      if (state.status === 'rejected') return say({ kind: 'error', text: `Test refusé : ${reasons(state.violations)}.` });
      if (state.status !== 'applied') return say({ kind: 'error', text: `Le test n’a pas abouti (état : ${state.status || 'inconnu'}).` });
      if (!(await refresh())) return say({ kind: 'error', text: 'Le test a eu lieu, mais son résultat n’a pas pu être lu.' });
      const test = settings.lastTest;
      // The result of this test, not the one that was on record before it.
      if (test === null || (before !== null && before.at === test.at && before.ok === test.ok && before.message === test.message)) {
        return say({ kind: 'error', text: 'Le résultat du test n’est pas encore disponible.' });
      }
      if (form.mode === 'api') {
        const label = testedTypeLabel(settings);
        const url = form.url.trim();
        known = label !== null ? { url, label } : known?.url === url ? null : known;
        paintDetected();
      }
      say({ kind: test.ok ? 'ok' : 'error', text: test.message || (test.ok ? 'Connexion réussie.' : 'Connexion impossible.') });
    });

  const saveSettings = (): Promise<void> =>
    act('L’enregistrement a échoué.', async () => {
      say({ kind: 'busy', text: 'Enregistrement…' });
      const state = await send(SAVE_SETTINGS, ['role:approver']);
      if (state.status === 'rejected') return say({ kind: 'error', text: `Enregistrement refusé : ${reasons(state.violations)}.` });
      if (state.status !== 'applied') return say({ kind: 'error', text: `L’enregistrement n’a pas abouti (état : ${state.status || 'inconnu'}).` });
      // The clear key leaves the screen once it is saved: only its mask is ever shown.
      form = { ...form, key: '' };
      const read = await refresh();
      const label = savedTypeLabel(settings);
      if (read && form.mode === 'api' && label !== null) known = { url: form.url.trim(), label };
      paintFields();
      say(read ? { kind: 'ok', text: 'Paramètres enregistrés.' } : { kind: 'ok', text: 'Paramètres enregistrés, mais ils n’ont pas pu être relus.' });
    });

  host.addEventListener('submit', (event) => event.preventDefault());

  host.addEventListener('change', (event) => {
    // Read by shape, not by class: the card runs in whichever window the shell is.
    const target = event.target as { id?: string; name?: string; value?: string } | null;
    if (target?.name === 'ai-mode') form = withMode(settings, form, target.value === 'binary' ? 'binary' : 'api');
    else if (target?.id === 'ai-binary') form = withBinary(settings, form, target.value === 'claude' ? 'claude' : 'opencode');
    else return;
    say({ kind: 'none' });
    paintFields();
  });

  host.addEventListener('input', (event) => {
    const target = event.target as { id?: string; value?: string } | null;
    const value = target?.value ?? '';
    if (target?.id === 'ai-url') {
      form = { ...form, url: value };
      paintDetected();
      host.querySelector('#ai-key')?.setAttribute('placeholder', keyPlaceholder(form, settings));
    } else if (target?.id === 'ai-key') form = { ...form, key: value };
    else if (target?.id === 'ai-generation-model') form = { ...form, generationModel: value };
    else if (target?.id === 'ai-summary-model') form = { ...form, summaryModel: value };
  });

  host.addEventListener('click', (event) => {
    const button = (event.target as Element | null)?.closest('button');
    if (!button) return;
    // The actuator says which DataCapability it triggers: that, and nothing else, is what it sends.
    if (button.dataset.writes === TEST_CONNECTION) void testConnection();
    else if (button.dataset.writes === SAVE_SETTINGS) void saveSettings();
  });

  paintAll();
  lock(true);
  read().then(
    (loaded) => {
      settings = loaded;
      form = initialForm(settings);
      const label = savedTypeLabel(settings);
      known = label === null ? null : { url: form.url.trim(), label };
      outcome = settings.lastTest ? { kind: 'last', test: settings.lastTest } : { kind: 'none' };
      paintAll();
      lock(false);
    },
    () => {
      say({ kind: 'error', text: 'Impossible de lire les paramètres IA.' });
      lock(false);
    },
  );
}
