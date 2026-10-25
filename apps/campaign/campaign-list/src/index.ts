// CampaignList (system campaign): the home page's campaigns and oneshots as
// cards, the button that creates one, the one that exports one to an archive
// file and the dialog that imports an archive. It reads `campaign.listCampaigns`
// and triggers `campaign.createCampaign`, `campaign.exportCampaign` and
// `campaign.importCampaign`, by identifier, through the shell it is handed:
// no network call of its own. An archive is only made when the DM clicks
// Exporter, and an import is always a new campaign: nothing here names the
// campaign an import would overwrite.

import { CREATE_CAMPAIGN, EXPORT_CAMPAIGN, IMPORT_CAMPAIGN, LIST_CAMPAIGNS } from './ids.ts';
import {
  commandId,
  createCommand,
  exportCommand,
  importCommand,
  isFinal,
  mayBeQueued,
  parseArchive,
  parseCampaigns,
  failed,
  refusedBy,
  safeFileName,
  settled,
  unapplied,
  type Settled,
  type Words,
} from './model.ts';
import { cards, createDialog, empty, importDialog, skeleton } from './render.ts';
import { STYLE } from './style.ts';
import type { CampaignCard, Shell } from './types.ts';

/** How a command is followed until it settles: the server applies it on its own clock. */
export interface Timing {
  sleep(ms: number): Promise<void>;
  everyMs: number;
  /** How many times it asks before saying it has not settled. */
  polls: number;
}

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };

/** Hands the DM a file: the archive an export answered, as a download. */
export type Save = (file: string, content: unknown) => void;

const REVOKE_AFTER_MS = 10_000;

/** The download the page offers: the archive as a JSON file, saved under the name the export gave. */
export const downloadIn =
  (doc: Document): Save =>
  (file, content) => {
    const win = doc.defaultView;
    if (!win) throw new Error('no window to download from');
    const url = win.URL.createObjectURL(new win.Blob([JSON.stringify(content, null, 2)], { type: 'application/json' }));
    const link = doc.createElement('a');
    link.href = url;
    link.download = safeFileName(file);
    doc.body.append(link);
    link.click();
    link.remove();
    // Some browsers start the download after the click returns.
    win.setTimeout(() => win.URL.revokeObjectURL(url), REVOKE_AFTER_MS);
  };

const EXPORT_WORDS: Words = { refused: 'Export refusé', pending: 'Export non appliqué', failed: 'L’export a échoué.' };
const CREATE_WORDS: Words = { refused: 'Création refusée', pending: 'Création non appliquée', failed: 'La création a échoué.' };
const IMPORT_WORDS: Words = { refused: 'Import refusé', pending: 'Import non appliqué', failed: 'L’import a échoué.' };

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="CampaignList"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'CampaignList';
  style.textContent = STYLE;
  doc.head.append(style);
}

/** What a file holds, read as text by the page's own reader. */
const readText = (doc: Document, file: File): Promise<string> =>
  new Promise((resolve, reject) => {
    const Reader = doc.defaultView?.FileReader;
    if (!Reader) return reject(new Error('no reader'));
    const reader = new Reader();
    reader.onload = () => resolve(typeof reader.result === 'string' ? reader.result : '');
    reader.onerror = () => reject(new Error('the file could not be read'));
    reader.readAsText(file);
  });

const size = (bytes: number): string =>
  bytes < 1024 ? `${bytes} o` : bytes < 1048576 ? `${(bytes / 1024).toFixed(1).replace('.', ',')} Ko` : `${(bytes / 1048576).toFixed(1).replace('.', ',')} Mo`;

/** A new command's key: the server keeps one command per key, so the same command sent again is not a second one. */
let keys = 0;
const newKey = (): string => `CampaignList:${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}-${(keys += 1)}`;

/** What each host listens with: a list mounted again in the same host drops them, so one click is one command. */
const listening = new WeakMap<HTMLElement, Array<() => void>>();

/** What the shell calls: fills `host` with the list and wires it. */
export function mount(host: HTMLElement, _props: Record<string, unknown>, shell: Shell, timing: Timing = WAITING, save: Save = downloadIn(host.ownerDocument)): void {
  ensureStyle(host.ownerDocument);
  for (const drop of listening.get(host) ?? []) drop();
  const dropped: Array<() => void> = [];
  listening.set(host, dropped);
  /** A command or a read that outlives its mount answers into the DOM of the one that replaced it: it must not. */
  const current = (): boolean => listening.get(host) === dropped;
  const on = <K extends keyof HTMLElementEventMap>(type: K, handler: (event: HTMLElementEventMap[K]) => void): void => {
    host.addEventListener(type, handler);
    dropped.push(() => host.removeEventListener(type, handler));
  };
  host.innerHTML = skeleton();

  const part = <T extends HTMLElement = HTMLElement>(id: string): T => {
    const found = host.querySelector<T>(`#${id}`);
    if (!found) throw new Error(`CampaignList: #${id} is not in the list`);
    return found;
  };

  let campaigns: CampaignCard[] = [];
  let busy = false;
  /** The archive the import dialog holds, once a file was read as one. */
  let archive: unknown;
  /** The key of the open dialog's command and what it was sent with: kept while that may still be queued, so a second click is the same command. */
  let key = { value: newKey(), sent: '' };
  const fresh = (): void => {
    key = { value: newKey(), sent: '' };
  };
  /** The command `build` makes, keyed: an edited form or another archive is another command, never answered as the first. */
  const keyed = (build: (idempotencyKey: string) => unknown): unknown => {
    const sent = JSON.stringify(build(''));
    if (key.sent !== '' && key.sent !== sent) fresh();
    key.sent = sent;
    return build(key.value);
  };

  const say = (words: string): void => {
    if (current()) part('campaign-status').textContent = words;
  };
  const paintBusy = (): void => {
    if (!current()) return;
    for (const button of host.querySelectorAll<HTMLButtonElement>('[data-writes]')) {
      button.disabled = busy || (button.id === 'campaign-import-submit' && archive === undefined);
    }
  };
  const paint = (): void => {
    if (!current()) return;
    part('campaign-cards').innerHTML = campaigns.length === 0 ? empty() : cards(campaigns);
    paintBusy();
  };

  const read = async (): Promise<CampaignCard[]> => parseCampaigns((await shell.capability(LIST_CAMPAIGNS, undefined, {})).data);

  /** A command is sent, then followed until the server has applied or refused it. */
  async function send(body: unknown): Promise<Settled> {
    const answer = await shell.command(body);
    const followed = commandId(answer);
    let state = settled(answer);
    for (let asked = 0; !isFinal(state.status) && followed !== '' && asked < timing.polls; asked += 1) {
      await timing.sleep(timing.everyMs);
      try {
        state = settled(await shell.commandResult(followed));
      } catch {
        // The command is queued all the same: a lookup that fails is asked again, never taken for a refusal that would have the DM send it twice.
      }
    }
    return state;
  }

  /** Runs one command with the buttons held; a server that could not be asked, or refused it at once, settles it as failed. */
  async function run(body: unknown): Promise<Settled> {
    busy = true;
    paintBusy();
    try {
      return await send(body);
    } catch (error) {
      return failed(refusedBy(error));
    } finally {
      busy = false;
      paintBusy();
    }
  }

  /** The list is read again after a command: the new campaign is the one the server holds, not one the list made up. */
  async function reload(applied: string): Promise<void> {
    try {
      campaigns = await read();
      paint();
      say(applied);
    } catch {
      say(`${applied} La liste n’a pas pu être relue.`);
    }
  }

  /** Which dialog is open: a file read for one that was closed since is no one's. */
  let dialogs = 0;
  const closeDialog = (): void => {
    dialogs += 1;
    part('campaign-dialogs').innerHTML = '';
    archive = undefined;
  };
  /** A dialog is left by its DM while no command of it is followed: its answer has somewhere to land. */
  const leave = (): void => {
    if (!busy && part('campaign-dialogs').firstElementChild) closeDialog();
  };
  const dialogError = (id: string, words: string): void => {
    if (current()) part(id).textContent = words;
  };

  async function exportCampaign(id: string): Promise<void> {
    if (busy) return;
    const campaign = campaigns.find((c) => c.id === id);
    if (!campaign) return;
    say(`Export de « ${campaign.name} » en cours…`);
    const state = await run(exportCommand(EXPORT_CAMPAIGN, id));
    // A replaced mount hands nothing over: the one that holds the host answers its own clicks.
    if (!current()) return;
    if (state.status !== 'applied') return say(unapplied(state, EXPORT_WORDS));
    // What is handed over is the archive the command answered, nothing the list built.
    if (state.archive === null) return say('Export effectué, mais l’archive n’a pas été reçue.');
    try {
      save(state.archive.file, state.archive.content);
      say(`Archive « ${state.archive.file} » enregistrée.`);
    } catch {
      say('Export effectué, mais l’archive n’a pas pu être enregistrée.');
    }
  }

  async function createCampaign(): Promise<void> {
    if (busy) return;
    const name = part<HTMLInputElement>('campaign-create-name').value;
    if (name.trim() === '') return dialogError('campaign-create-error', 'Le nom est obligatoire.');
    const type = part('campaign-create-dialog').querySelector<HTMLInputElement>('input[name="campaign-type"]:checked')?.value === 'oneshot' ? 'oneshot' : 'campagne';
    const pitch = part<HTMLInputElement>('campaign-create-pitch').value;
    dialogError('campaign-create-error', '');
    const state = await run(keyed((k) => createCommand(CREATE_CAMPAIGN, { name, type, pitch }, k)));
    if (!current()) return;
    if (state.status !== 'applied') {
      if (!mayBeQueued(state)) fresh();
      return dialogError('campaign-create-error', unapplied(state, CREATE_WORDS));
    }
    closeDialog();
    await reload(`Campagne « ${name.trim()} » créée.`);
  }

  async function importArchive(): Promise<void> {
    if (busy || archive === undefined) return;
    dialogError('campaign-import-error', '');
    const state = await run(keyed((k) => importCommand(IMPORT_CAMPAIGN, archive, k)));
    if (!current()) return;
    // A refusal leaves the dialog open with its reason, and the list as it was.
    if (state.status !== 'applied') {
      if (!mayBeQueued(state)) fresh();
      return dialogError('campaign-import-error', unapplied(state, IMPORT_WORDS));
    }
    closeDialog();
    await reload('Archive importée : une nouvelle campagne a été ajoutée.');
  }

  /** A file chosen or dropped: shown in the FileDrop if it is an archive, refused otherwise. */
  async function takeFile(file: File): Promise<void> {
    const drop = part('campaign-import-field');
    const asked = dialogs;
    dialogError('campaign-import-error', '');
    let parsed: unknown;
    try {
      parsed = parseArchive(await readText(host.ownerDocument, file));
    } catch {
      parsed = undefined;
    }
    // The dialog that asked is gone, or replaced, or so is its mount: what was read is no one's.
    if (!current() || asked !== dialogs) return;
    archive = parsed;
    if (archive === undefined) {
      drop.classList.remove('has-file', 'is-done');
      drop.classList.add('has-error');
      return paintBusy();
    }
    drop.classList.remove('has-error');
    drop.classList.add('has-file', 'is-done');
    const named = (cls: string): HTMLElement => {
      const found = drop.querySelector<HTMLElement>(`.${cls}`);
      if (!found) throw new Error(`CampaignList: .${cls} is not in the drop zone`);
      return found;
    };
    named('kit-filedrop__name').textContent = file.name;
    named('kit-filedrop__size').textContent = size(file.size);
    named('kit-filedrop__thumb').textContent = (file.name.split('.').pop() ?? '').toUpperCase();
    named('kit-filedrop__fill').style.width = '100%';
    paintBusy();
  }

  const open = (html: string): void => {
    dialogs += 1;
    archive = undefined;
    fresh();
    part('campaign-dialogs').innerHTML = html;
    paintBusy();
  };

  on('click', (event) => {
    const button = (event.target as Element | null)?.closest('button');
    if (!button) return;
    // The actuator says which DataCapability it triggers: that, and nothing else, is what it sends.
    const writes = button.dataset.writes;
    if (button.hasAttribute('data-close')) leave();
    else if (button.id === 'campaign-new') open(createDialog());
    else if (button.id === 'campaign-import') open(importDialog());
    else if (writes === EXPORT_CAMPAIGN) void exportCampaign(button.dataset.campaign ?? '');
    else if (writes === CREATE_CAMPAIGN && button.id === 'campaign-create-submit') void createCampaign();
    else if (writes === IMPORT_CAMPAIGN && button.id === 'campaign-import-submit') void importArchive();
    else if (button.dataset.href) host.ownerDocument.defaultView?.location.assign(button.dataset.href);
  });

  on('change', (event) => {
    const target = event.target as HTMLInputElement | null;
    if (target?.id !== 'campaign-import-file') return;
    const file = target.files?.[0];
    if (file) void takeFile(file);
  });

  // The kit's FileDrop: a file dropped on the zone is taken as one chosen.
  on('dragover', (event) => {
    if ((event.target as Element | null)?.closest('#campaign-import-drop')) {
      event.preventDefault();
      part('campaign-import-drop').classList.add('is-over');
    }
  });
  on('dragleave', (event) => {
    if ((event.target as Element | null)?.closest('#campaign-import-drop')) part('campaign-import-drop').classList.remove('is-over');
  });
  on('drop', (event) => {
    if (!(event.target as Element | null)?.closest('#campaign-import-drop')) return;
    event.preventDefault();
    part('campaign-import-drop').classList.remove('is-over');
    const file = event.dataTransfer?.files[0];
    if (file) void takeFile(file);
  });
  on('keydown', (event) => {
    const key = event.key;
    if ((event.target as Element | null)?.id === 'campaign-import-drop' && (key === 'Enter' || key === ' ')) {
      event.preventDefault();
      part('campaign-import-file').click();
    } else if (key === 'Escape') leave();
  });

  read().then(
    (loaded) => {
      campaigns = loaded;
      paint();
    },
    () => say('Impossible de lire les campagnes.'),
  );
}
