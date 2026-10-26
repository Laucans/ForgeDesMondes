// CampaignOverview (system campaign): the panel at the top of a campaign —
// its cover, pitch, counts, and the button that exports it. It reads
// `campaign.getCampaign` and triggers `campaign.exportCampaign`, by
// identifier, through the shell it is handed: no network call of its own.
// After an export the campaign is read again, so the date shown is the one
// the export stamped (`export-stamps-date`), not one the panel made up.

import { EXPORT_CAMPAIGN, GET_CAMPAIGN } from './ids.ts';
import { campaignId, commandId, exportCommand, isFinal, lastExportText, parseCampaign, safeFileName, settled, type Archive, type Settled } from './model.ts';
import { actions, badges, counts, cover, skeleton } from './render.ts';
import { STYLE } from './style.ts';
import type { Campaign, Shell } from './types.ts';

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

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="CampaignOverview"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'CampaignOverview';
  style.textContent = STYLE;
  doc.head.append(style);
}

/** The click listener each host holds: a panel mounted again in the same host replaces it, so one click is one export. */
const listening = new WeakMap<HTMLElement, (event: Event) => void>();

/** What the shell calls: fills `host` with the panel and wires it. */
export function mount(host: HTMLElement, props: Record<string, unknown>, shell: Shell, timing: Timing = WAITING, save: Save = downloadIn(host.ownerDocument)): void {
  ensureStyle(host.ownerDocument);
  const previous = listening.get(host);
  if (previous) host.removeEventListener('click', previous);
  host.innerHTML = skeleton();

  const part = (id: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(`#${id}`);
    if (!found) throw new Error(`CampaignOverview: #${id} is not in the panel`);
    return found;
  };

  const id = campaignId(props, host.ownerDocument.defaultView?.location.search ?? '');
  let campaign: Campaign | null = null;
  let busy = false;

  const say = (words: string): void => {
    part('overview-export-status').textContent = words;
  };
  const paintExportButton = (): void => {
    for (const button of host.querySelectorAll<HTMLButtonElement>('#overview-export')) button.disabled = busy;
  };
  const paint = (shown: Campaign): void => {
    part('overview-cover').innerHTML = cover(shown);
    part('overview-type').innerHTML = badges(shown);
    part('overview-title').textContent = shown.name;
    part('overview-pitch').textContent = shown.pitch;
    part('overview-counts').innerHTML = counts(shown);
    part('overview-actions').innerHTML = actions(shown);
    say(lastExportText(shown.lastExport));
    paintExportButton();
  };

  const read = async (): Promise<Campaign | null> => parseCampaign((await shell.capability(GET_CAMPAIGN, undefined, { id })).data);

  /** The export is sent, then followed until the server has applied or refused it. */
  async function send(): Promise<Settled> {
    const answer = await shell.command(exportCommand(EXPORT_CAMPAIGN, id));
    const followed = commandId(answer);
    let state = settled(answer);
    for (let asked = 0; !isFinal(state.status) && followed !== '' && asked < timing.polls; asked += 1) {
      await timing.sleep(timing.everyMs);
      state = settled(await shell.commandResult(followed));
    }
    return state;
  }

  /** Saves the archive an applied export answered; why it could not be, when it could not. */
  function keep(archive: Archive | null): string[] {
    if (archive === null) return ['le serveur n’a pas rendu l’archive'];
    try {
      save(archive.file, archive.content);
      return [];
    } catch {
      return ['le fichier n’a pas pu être enregistré'];
    }
  }

  async function exportCampaign(): Promise<void> {
    if (busy || campaign === null) return;
    const before = lastExportText(campaign.lastExport);
    busy = true;
    paintExportButton();
    say('Export en cours…');
    try {
      const state = await send();
      if (state.status === 'rejected') return say(`Export refusé : ${state.violations.join(', ') || 'raison inconnue'}. ${before}`);
      if (state.status === 'cancelled' || state.status === 'expired') return say(`L’export est ${state.status === 'cancelled' ? 'annulé' : 'expiré'} : l’état est final, aucune archive ne viendra. ${before}`);
      if (state.status !== 'applied') return say(`L’export n’est pas encore appliqué (état : ${state.status || 'inconnu'}). ${before}`);
      // The file first: it is what the DM asked for, whether or not the date can be read again.
      const problems = keep(state.archive);
      // The date shown is the one the campaign now holds.
      let fresh: Campaign | null = null;
      try {
        fresh = await read();
      } catch {
        // Applied all the same: only the date could not be read again.
      }
      if (fresh !== null) {
        campaign = fresh;
        paint(fresh);
      } else problems.push('la date n’a pas pu être relue');
      if (problems.length > 0) say(`Export effectué, mais ${problems.join(' et ')}. ${fresh === null ? before : lastExportText(fresh.lastExport)}`);
    } catch {
      say(`L’export a échoué. ${before}`);
    } finally {
      busy = false;
      paintExportButton();
    }
  }

  const onClick = (event: Event): void => {
    const button = (event.target as Element | null)?.closest('button');
    if (!button) return;
    // The actuator says which DataCapability it triggers: that, and nothing else, is what it sends.
    if (button.dataset.writes === EXPORT_CAMPAIGN) void exportCampaign();
    else if (button.dataset.href) host.ownerDocument.defaultView?.location.assign(button.dataset.href);
  };
  listening.set(host, onClick);
  host.addEventListener('click', onClick);

  if (id === '') return say('Aucune campagne ouverte.');
  read().then(
    (loaded) => {
      if (loaded === null) return say('Campagne introuvable.');
      campaign = loaded;
      paint(loaded);
    },
    () => say('Impossible de lire la campagne.'),
  );
}
