// EntityEditor (system campaign): the sheet of a character, a place or an
// event — identity, aliases, the initial prompt told or typed, the stats, the
// token, the notes, D&D Beyond. It reads `campaign.getEntity`,
// `campaign.listPlayers` and `media.listMedia` and triggers
// `campaign.saveEntity`, `ai.proposeCharacter`, `dndbeyond.importCharacter`
// and `dndbeyond.resyncCharacter`, by identifier, through the shell it is
// handed: no network call of its own.
//
// Only the DM's explicit save writes the sheet (explicit-save-only): what is
// typed is a draft, and leaving a modified sheet from the rail asks first. A
// change by the AI or D&D Beyond applies at once and is versioned: the sheet
// is read again, shows « Mis à jour » and the way to the diff.

import {
  BUS_OPEN,
  BUS_OPENED,
  BUS_QUERY,
  CAMPAIGN_CONTEXT,
  ENTITY_CONTEXT,
  EVENT_HISTORY,
  EVENT_OPENED,
  EVENT_UPDATED,
  GET_ENTITY,
  IMPORT_CHARACTER,
  LIST_MEDIA,
  LIST_PLAYERS,
  NEW_PREFIX,
  PROPOSE_CHARACTER,
  RESYNC_CHARACTER,
  SAVE_ENTITY,
} from './ids.ts';
import {
  campaignVariables,
  changesBetween,
  commandBody,
  commandId,
  entityTarget,
  freshEntity,
  initialsOf,
  isCharacter,
  isDirty,
  isFinal,
  isKind,
  newId,
  overlay,
  parseAliases,
  parseEntity,
  parseIllustrations,
  parsePlayers,
  parseTarget,
  reasons,
  sameTarget,
  savePayload,
  settled,
  signed,
  targetProp,
  type Settled,
} from './model.ts';
import { ddb as ddbHtml, diffView, features as featuresHtml, head as headHtml, identity, kept, MIC_TEXTS, type MicState, notice as noticeHtml, prompt as promptHtml, sheet as sheetHtml, skeleton, statblock, table as tableHtml, token as tokenHtml } from './render.ts';
import { STYLE } from './style.ts';
import { ABILITIES, type Change, type Dictation, type Entity, type Media, type Opened, type OpenRequest, type Outcome, type Player, type Shell, type Target, type Timing } from './types.ts';

export type { Dictation, Shell, Timing } from './types.ts';

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };

/** What the shell keeps of the sheet: it follows its props, and the rail's request goes through them. */
export interface Mounted {
  update(props: Record<string, unknown>): void;
}

/** One click is one command: a new key per click, the same for every attempt of it. */
let clicks = 0;
const keyOf = (dataCapability: string, entity: string): string => `EntityEditor:${dataCapability}:${entity}:${Date.now().toString(36)}-${(clicks += 1)}`;

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="EntityEditor"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'EntityEditor';
  style.textContent = STYLE;
  doc.head.append(style);
}

/** What a change by the AI or D&D Beyond is called, and where it comes from. */
interface ChangeSpec {
  dataCapability: string;
  payload: Record<string, unknown>;
  busy: string;
  failed: string;
  origin: 'ia' | 'dndbeyond';
  source: string;
}

/** What the shell calls: fills `host` with the sheet and wires it. Answers how to follow new props. */
export function mount(host: HTMLElement, props: Record<string, unknown>, shell: Shell, timing: Timing = WAITING, dictation?: Dictation): Mounted {
  const doc = host.ownerDocument;
  ensureStyle(doc);
  host.innerHTML = skeleton();

  const part = <T extends HTMLElement = HTMLElement>(id: string): T => {
    const found = host.querySelector<T>(`#${id}`);
    if (!found) throw new Error(`EntityEditor: #${id} is not in the sheet`);
    return found;
  };
  const field = <T extends HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>(id: string): T | null => host.querySelector<T>(`#${id}`);

  /** The sheet on show: what the server holds, and the draft the DM edits. */
  let target: Target | null = null;
  let stored: Entity | null = null;
  let draft: Entity | null = null;
  let players: Player[] = [];
  let illustrations: Media[] = [];
  /** The id a sheet to create will have once saved, so a retry names the same element. */
  let freshId = '';
  /** What the rail asked for while the sheet was modified: the bar waits for the DM's word. */
  let pending: Target | null = null;
  /** What the rail asked for while a command ran. */
  let deferred: Target | null = null;
  let busy = false;
  let opening = 0;
  let ddbDiff: Change[] | null = null;
  const dm = { url: '' };

  const campaign = (): string => {
    const known = shell.context?.get(CAMPAIGN_CONTEXT);
    return typeof known === 'string' ? known : '';
  };
  const isNew = (): boolean => stored !== null && stored.id === '';
  const dirty = (): boolean => draft !== null && stored !== null && isDirty(draft, stored);

  const tell = (name: string, detail: unknown): void => {
    const view = doc.defaultView;
    if (view) host.dispatchEvent(new view.CustomEvent(name, { bubbles: true, detail }));
  };

  /** The sheet on show, as the page's bus words it (`fdm:entity-opened`): the rail follows, and the address names the sheet. */
  let opened: Opened | null = null;
  const announce = (detail: Opened): void => {
    opened = detail;
    tell(EVENT_OPENED, detail);
    const view = doc.defaultView;
    if (!view) return;
    doc.dispatchEvent(new view.CustomEvent(BUS_OPENED, { detail }));
    try {
      // The address, same document: no reload, whatever else it carries stays.
      const url = new URL(view.location.href);
      url.searchParams.delete('id');
      url.searchParams.delete('new');
      url.searchParams.set(detail.id === null ? 'new' : 'id', detail.id ?? detail.kind);
      view.history.replaceState(null, '', url);
    } catch {
      // A page in a frame without access to its history keeps its address.
    }
  };

  const say = (outcome: Outcome): void => {
    part('entity-status').innerHTML = outcome.kind === 'none' ? '' : `<p class="${outcome.kind === 'busy' ? '' : `is-${outcome.kind}`}">${outcome.text.replace(/[&<>]/g, (c) => `&#${c.charCodeAt(0)};`)}</p>`;
  };

  // ---------- reading and painting ----------

  const playerName = (): string | null => {
    if (draft === null || draft.kind !== 'pj') return null;
    return players.find((p) => p.id === draft?.player)?.name ?? null;
  };

  const paintHead = (): void => {
    if (draft === null) return;
    part('entity-head').innerHTML = headHtml(draft, playerName(), isNew());
    doc.title = `${draft.name || 'Nouvelle fiche'} — Fiche d’entité — ForgeDesMondes`;
  };

  const stopMic = { stop: (): void => undefined };
  const paintPrompt = (): void => {
    const box = part('entity-prompt');
    box.hidden = draft === null || !isCharacter(draft.kind);
    stopMic.stop();
    if (draft === null || box.hidden) return;
    box.innerHTML = promptHtml(draft, !isNew());
    wireMic();
  };

  const paintDdb = (): void => {
    const box = part('entity-ddb');
    box.hidden = draft === null || draft.kind !== 'pj';
    if (draft === null || box.hidden) return;
    box.innerHTML = ddbHtml(draft, dm.url);
    if (ddbDiff !== null) part('entity-ddb-diff').innerHTML = (ddbDiff.length > 0 ? diffView('Resynchronisation', ddbDiff) : '') + kept(draft);
  };

  const paintSliders = (): void => {
    for (const slider of host.querySelectorAll<HTMLInputElement>('.kit-slider__input')) {
      const min = Number(slider.min);
      const max = Number(slider.max);
      slider.style.setProperty('--fill', `${((Number(slider.value) - min) / (max - min)) * 100}%`);
      const out = slider.closest('.kit-slider__field')?.querySelector('.kit-slider__out');
      if (out) out.textContent = slider.value;
    }
  };

  /** The token preview follows the name, the colour and the crop. */
  const paintToken = (): void => {
    const box = host.querySelector<HTMLElement>('#entity-token-preview');
    if (box === null || draft === null) return;
    box.innerHTML = tokenHtml(draft);
    const portrait = box.querySelector<HTMLElement>('.kit-token__portrait');
    if (portrait) portrait.style.transform = `scale(${draft.crop.zoom / 100}) translate(${(50 - draft.crop.x) / 2}%, ${(50 - draft.crop.y) / 2}%)`;
  };

  const paintTable = (): void => {
    if (draft === null) return;
    part('entity-table').innerHTML = tableHtml(draft, illustrations);
    paintToken();
    paintSliders();
  };

  const paintSheet = (): void => {
    const box = part('entity-sheet');
    box.hidden = draft === null || !isCharacter(draft.kind);
    if (draft === null || box.hidden) return;
    box.innerHTML = sheetHtml(draft);
  };

  const paintIdentity = (): void => {
    if (draft !== null) part('entity-identity').innerHTML = identity(draft, players);
  };

  const paintAll = (): void => {
    paintHead();
    paintIdentity();
    paintPrompt();
    paintSheet();
    paintDdb();
    paintTable();
    paintControls();
  };

  /** What follows the typing without repainting the fields: the title, the statblock, the modifiers, the token. */
  const paintLive = (): void => {
    if (draft === null) return;
    const title = host.querySelector('#entity-title');
    if (title) title.textContent = draft.name || 'Sans nom';
    const subtitle = host.querySelector('#entity-subtitle');
    if (subtitle) subtitle.textContent = draft.aliases.join(', ') || 'Renseignez le nom, les alias et la description.';
    const glyph = host.querySelector('.mf-entity__portrait .mf-entity__glyph');
    if (glyph) glyph.textContent = initialsOf(draft.name) || '?';
    for (const ability of ABILITIES) {
      const label = field(`entity-stat-${ability}`)?.closest('label');
      if (label?.firstChild) label.firstChild.textContent = `${ability} ${signed(draft.stats[ability])}`;
    }
    const block = host.querySelector('#entity-statblock');
    if (block) block.innerHTML = statblock(draft);
    const counter = host.querySelector('#entity-description ~ .kit-textarea__footer .kit-textarea__counter');
    if (counter) counter.textContent = `${draft.description.length} / 600`;
    paintToken();
    paintSliders();
  };

  /** Which actuators can act now: a sheet not saved has no one to ask the AI or D&D Beyond about. */
  const paintControls = (): void => {
    const enable = (id: string, on: boolean): void => {
      const control = host.querySelector<HTMLButtonElement>(`#${id}`);
      if (control) control.disabled = busy || !on;
    };
    enable('entity-save', true);
    enable('entity-propose', !isNew());
    enable('entity-ddb-import', true);
    enable('entity-ddb-resync', draft?.ddb != null && !isNew());
  };

  /** While a command runs, the sheet holds still: what is sent is what is on screen. */
  const lock = (locked: boolean): void => {
    busy = locked;
    for (const control of host.querySelectorAll<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement | HTMLButtonElement>('button, input, textarea, select')) control.disabled = locked;
    if (locked) return;
    paintControls();
    // What the rail asked for while the command ran is judged now: the sheet is as the command left it.
    const next = deferred;
    deferred = null;
    if (next !== null) request(next);
  };

  // ---------- the draft ----------

  const text = (id: string): string | null => field(id)?.value ?? null;
  const num = (id: string, otherwise: number): number => {
    const value = text(id);
    const parsed = value === null || value.trim() === '' ? NaN : Number(value);
    return Number.isFinite(parsed) ? parsed : otherwise;
  };

  /** The fields on screen go into the draft. */
  const collect = (): void => {
    if (draft === null) return;
    const d = draft;
    d.name = text('entity-name') ?? d.name;
    const aliases = text('entity-aliases');
    if (aliases !== null) d.aliases = parseAliases(aliases);
    d.description = text('entity-description') ?? d.description;
    d.notes = text('entity-notes') ?? d.notes;
    d.prompt = text('entity-prompt-text') ?? d.prompt;
    d.background = text('entity-background') ?? d.background;
    d.behavior = text('entity-behavior') ?? d.behavior;
    d.player = text('entity-player') ?? d.player;
    for (const ability of ABILITIES) d.stats[ability] = num(`entity-stat-${ability}`, d.stats[ability]);
    d.ca = num('entity-ca', d.ca);
    d.pvMax = num('entity-pv', d.pvMax);
    d.crop = { zoom: num('entity-crop-zoom', d.crop.zoom), x: num('entity-crop-x', d.crop.x), y: num('entity-crop-y', d.crop.y) };
    dm.url = text('entity-ddb-url') ?? dm.url;
    const media = (id: string, current: string | null | undefined): string | null | undefined => {
      const chosen = text(id);
      return chosen === null ? current : chosen === '' ? undefined : chosen === 'none' ? null : chosen;
    };
    d.portraitMedia = media('entity-portrait', d.portraitMedia);
    d.tokenMedia = media('entity-token-image', d.tokenMedia);
  };

  const read = async (id: string): Promise<Entity | null> => {
    const answer = await shell.capability(GET_ENTITY, undefined, { id });
    return parseEntity(answer.data);
  };

  // ---------- opening ----------

  /** The lists beside the sheet, of the campaign the screen shows: a campaign without players or media is not a sheet that cannot open. */
  let listsFor: string | null = null;
  async function readLists(): Promise<void> {
    const known = campaign();
    const variables = campaignVariables(known);
    [players, illustrations] = await Promise.all([
      shell.capability(LIST_PLAYERS, undefined, variables).then((answer) => parsePlayers(answer.data), () => []),
      shell.capability(LIST_MEDIA, undefined, variables).then((answer) => parseIllustrations(answer.data), () => []),
    ]);
    listsFor = known;
  }

  /** The sheet changes in place: the element is read, the lists beside it too. Answers whether it opened. */
  async function open(wanted: Target): Promise<boolean> {
    const mine = (opening += 1);
    let entity: Entity | null;
    try {
      if ('fresh' in wanted) entity = freshEntity(wanted.fresh);
      else entity = await read(wanted.id);
      if (mine !== opening) return false;
      await readLists();
    } catch {
      if (mine === opening) say({ kind: 'error', text: 'Impossible de lire la fiche.' });
      return false;
    }
    if (mine !== opening) return false;
    if (entity === null) {
      say({ kind: 'error', text: 'Cette fiche est introuvable.' });
      return false;
    }
    target = wanted;
    stored = entity;
    draft = structuredClone(entity);
    freshId = newId();
    pending = null;
    ddbDiff = null;
    dm.url = entity.ddb?.url ?? '';
    part('entity-unsaved').hidden = true;
    part('entity-notice').hidden = true;
    say({ kind: 'none' });
    paintAll();
    shell.context?.set(ENTITY_CONTEXT, targetProp(wanted));
    announce({ id: entity.id === '' ? null : entity.id, name: entity.name, kind: entity.kind });
    return true;
  }

  const showBar = (): void => {
    if (draft === null || pending === null) return;
    part('entity-unsaved-text').textContent = `« ${draft.name || 'Nouvelle fiche'} » a des modifications non enregistrées. Ouvrir ${'fresh' in pending ? 'une nouvelle fiche' : 'l’élément demandé'} ?`;
    part('entity-unsaved').hidden = false;
  };

  /** A click in the rail: the sheet opens at once when it is clean, else after a choice. */
  function request(wanted: Target): void {
    if (target !== null && sameTarget(wanted, target)) {
      pending = null;
      part('entity-unsaved').hidden = true;
      return;
    }
    if (busy) {
      deferred = wanted;
      return;
    }
    collect();
    if (draft !== null && dirty()) {
      pending = wanted;
      showBar();
      return;
    }
    void open(wanted);
  }

  // ---------- commands ----------

  /** The command is sent, then followed until the server has applied or refused it. */
  async function send(dataCapability: string, payload: Record<string, unknown>, to: { aggregate: string; id?: string }, key: string): Promise<Settled> {
    const answer = await shell.command(commandBody(dataCapability, payload, to, key));
    const id = commandId(answer);
    let state = settled(answer);
    for (let asked = 0; !isFinal(state.status) && id !== '' && asked < timing.polls; asked += 1) {
      await timing.sleep(timing.everyMs);
      state = settled(await shell.commandResult(id));
    }
    return state;
  }

  /** Whether the command applied; if not, the status line says why. */
  function applied(state: Settled, what: string): boolean {
    if (state.status === 'rejected') say({ kind: 'error', text: `${what} refusé${reasons(state.violations) === '' ? '' : ` : ${reasons(state.violations)}`}.` });
    else if (state.status !== 'applied') say({ kind: 'error', text: `${what} n’a pas abouti (état : ${state.status || 'inconnu'}).` });
    return state.status === 'applied';
  }

  /** explicit-save-only: the draft is written by this and nothing else. Answers whether it was saved. */
  async function save(): Promise<boolean> {
    if (busy || draft === null || stored === null) return false;
    collect();
    if (draft.name.trim() === '') {
      say({ kind: 'error', text: 'Donnez un nom à la fiche.' });
      field('entity-name')?.focus();
      return false;
    }
    const creating = isNew();
    if (creating && campaign() === '') {
      say({ kind: 'error', text: 'Aucune campagne choisie.' });
      return false;
    }
    const id = creating ? freshId : stored.id;
    const sending: Entity = { ...structuredClone(draft), id };
    lock(true);
    try {
      say({ kind: 'busy', text: 'Enregistrement en cours…' });
      const state = await send(SAVE_ENTITY, savePayload(sending, campaign(), creating), entityTarget(id), keyOf(SAVE_ENTITY, id));
      if (!applied(state, 'L’enregistrement')) return false;
      let fresh: Entity | null = null;
      try {
        fresh = await read(id);
      } catch {
        // Saved all the same: the sheet is what was sent.
      }
      stored = fresh ?? structuredClone({ ...sending, portraitMedia: undefined, tokenMedia: undefined });
      draft = structuredClone(stored);
      target = { id };
      busy = false;
      paintAll();
      shell.context?.set(ENTITY_CONTEXT, id);
      if (creating) announce({ id, name: stored.name, kind: stored.kind });
      else if (opened !== null) opened = { ...opened, name: stored.name };
      say({ kind: 'ok', text: 'Fiche enregistrée.' });
      return true;
    } catch {
      say({ kind: 'error', text: 'L’enregistrement a échoué.' });
      return false;
    } finally {
      lock(false);
    }
  }

  /** The AI or D&D Beyond changes the sheet at once, versioned: the sheet is read again and says « Mis à jour ». */
  async function change(what: ChangeSpec, then?: (changes: Change[]) => void): Promise<void> {
    if (busy || draft === null || stored === null || isNew()) return;
    collect();
    const before = stored;
    const id = before.id;
    lock(true);
    try {
      say({ kind: 'busy', text: what.busy });
      const state = await send(what.dataCapability, what.payload, entityTarget(id), keyOf(what.dataCapability, id));
      if (!applied(state, what.failed)) return;
      const after = await read(id).catch(() => null);
      if (after === null) {
        say({ kind: 'error', text: `${what.failed} a abouti, mais la fiche n’a pas pu être relue.` });
        return;
      }
      const changes = changesBetween(before, after);
      // What the DM typed elsewhere stays, unsaved; what the change touched is the server's.
      draft = overlay(draft, before, after);
      stored = after;
      then?.(changes);
      busy = false;
      paintAll();
      if (changes.length === 0) say({ kind: 'ok', text: 'Aucun champ de la fiche n’a changé.' });
      else {
        say({ kind: 'none' });
        const box = part('entity-notice');
        box.innerHTML = noticeHtml(what.source, changes);
        box.hidden = false;
        tell(EVENT_UPDATED, { id, origin: what.origin, source: what.source, changes });
      }
    } catch {
      say({ kind: 'error', text: `${what.failed} a échoué.` });
    } finally {
      lock(false);
    }
  }

  const propose = (): Promise<void> => {
    collect();
    if (draft === null || draft.prompt.trim() === '') {
      field('entity-prompt-text')?.focus();
      return Promise.resolve();
    }
    return change({ dataCapability: PROPOSE_CHARACTER, payload: { prompt: draft.prompt }, busy: 'Proposition en cours…', failed: 'La proposition', origin: 'ia', source: 'Proposition de l’IA' });
  };

  const resync = (): Promise<void> =>
    change({ dataCapability: RESYNC_CHARACTER, payload: {}, busy: 'Resynchronisation en cours…', failed: 'La resynchronisation', origin: 'dndbeyond', source: 'Resynchronisation D&D Beyond' }, (changes) => {
      ddbDiff = changes;
    });

  async function importCharacter(): Promise<void> {
    collect();
    const url = dm.url.trim();
    if (draft === null || stored === null || busy) return;
    if (url === '') {
      field('entity-ddb-url')?.focus();
      return;
    }
    const known = campaign();
    const payload = { url, ...(known === '' ? {} : { campaign: known }), ...(draft.player === '' ? {} : { player: draft.player }) };
    // A sheet already saved is read again after the import; a new one has no one to read.
    if (!isNew()) return change({ dataCapability: IMPORT_CHARACTER, payload, busy: 'Import en cours…', failed: 'L’import', origin: 'dndbeyond', source: 'Import D&D Beyond' });
    lock(true);
    try {
      say({ kind: 'busy', text: 'Import en cours…' });
      const state = await send(IMPORT_CHARACTER, payload, entityTarget(''), keyOf(IMPORT_CHARACTER, freshId));
      if (applied(state, 'L’import')) say({ kind: 'ok', text: 'Personnage importé : retrouvez-le dans la liste des éléments.' });
    } catch {
      say({ kind: 'error', text: 'L’import a échoué.' });
    } finally {
      lock(false);
    }
  }

  // ---------- the bar ----------

  async function onBar(choice: 'save' | 'discard' | 'stay'): Promise<void> {
    const wanted = pending;
    if (wanted === null) return;
    if (choice === 'stay') {
      pending = null;
      part('entity-unsaved').hidden = true;
      // The rail follows the sheet that stayed.
      if (target !== null) shell.context?.set(ENTITY_CONTEXT, targetProp(target));
      return;
    }
    if (choice === 'save') {
      part('entity-unsaved').hidden = true;
      if (!(await save())) {
        // Not written: the sheet is still modified, the choice is still the DM's.
        pending = wanted;
        showBar();
        return;
      }
    }
    await open(wanted);
  }

  // ---------- the microphone ----------

  function wireMic(): void {
    const mic = host.querySelector<HTMLElement>('#entity-prompt-mic-box');
    const button = host.querySelector<HTMLButtonElement>('#entity-prompt-mic');
    if (mic === null || button === null) return;
    const label = mic.querySelector<HTMLElement>('.kit-micbutton__state');
    const timer = mic.querySelector<HTMLElement>('.kit-micbutton__timer');
    let state: MicState = 'idle';
    let seconds = 0;
    let tick: ReturnType<typeof setInterval> | null = null;
    let pulse: ReturnType<typeof setInterval> | null = null;
    const clock = (): string => `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`;
    const stopTimers = (): void => {
      if (tick !== null) clearInterval(tick);
      if (pulse !== null) clearInterval(pulse);
      tick = pulse = null;
      mic.style.setProperty('--kit-level', '0');
    };
    const set = (next: MicState): void => {
      state = next;
      stopTimers();
      if (next === 'recording') {
        tick = setInterval(() => {
          // A sheet that left the page stops listening.
          if (!mic.isConnected) return stopTimers();
          seconds += 1;
          if (timer) timer.textContent = clock();
        }, 1000);
        pulse = setInterval(() => mic.style.setProperty('--kit-level', (Math.random() * 0.9 + 0.1).toFixed(2)), 140);
      }
      mic.setAttribute('data-state', next);
      if (label) label.textContent = MIC_TEXTS[next];
      if (timer) timer.textContent = clock();
      button.setAttribute('aria-pressed', String(next === 'recording'));
      button.setAttribute('aria-label', `Raconter au micro — ${MIC_TEXTS[next]}`);
    };
    stopMic.stop = stopTimers;
    button.addEventListener('click', () => {
      if (state !== 'recording') return set('recording');
      set('paused');
      // The pause hands the transcript to the prompt: the text that goes to the provider, nothing else.
      const heard = dictation?.transcript() ?? '';
      const prompt = field<HTMLTextAreaElement>('entity-prompt-text');
      if (heard === '' || prompt === null) return;
      prompt.value = prompt.value === '' ? heard : `${prompt.value}\n${heard}`;
      collect();
    });
    set('idle');
  }

  // ---------- the events ----------

  host.addEventListener('input', (event) => {
    const el = event.target as HTMLInputElement | null;
    if (!el?.id.startsWith('entity-') || draft === null) return;
    collect();
    if (el.id !== 'entity-ddb-url') paintLive();
  });

  host.addEventListener('change', (event) => {
    const el = event.target as HTMLInputElement | null;
    if (el === null || draft === null) return;
    if (el.name === 'entity-kind' && isKind(el.value) && el.value !== draft.kind) {
      collect();
      draft.kind = el.value;
      // Only a PC has a player; a character that stops being one loses its place at the table.
      if (draft.kind !== 'pj') draft.player = '';
      paintAll();
      return;
    }
    if (el.id.startsWith('entity-')) {
      collect();
      if (el.id === 'entity-player') paintHead();
      paintLive();
    }
  });

  host.addEventListener('click', (event) => {
    const clicked = (event.target as Element | null)?.closest('button');
    if (!clicked || draft === null) return;
    const removed = clicked.dataset.removeFeature;
    if (removed !== undefined) {
      collect();
      draft.features.splice(Number(removed), 1);
      part('entity-features').innerHTML = featuresHtml(draft);
      paintLive();
      return;
    }
    // The actuator says which DataCapability it triggers: that, and nothing else, is what it sends.
    const writes = clicked.dataset.writes;
    if (clicked.id === 'entity-unsaved-save') void onBar('save');
    else if (writes === SAVE_ENTITY) void save();
    else if (writes === PROPOSE_CHARACTER) void propose();
    else if (writes === IMPORT_CHARACTER) void importCharacter();
    else if (writes === RESYNC_CHARACTER) void resync();
    else if (clicked.id === 'entity-unsaved-discard') void onBar('discard');
    else if (clicked.id === 'entity-unsaved-stay') void onBar('stay');
    else if (clicked.id === 'entity-feature-submit') {
      collect();
      const name = text('entity-feature-name')?.trim() ?? '';
      if (name === '') return;
      draft.features.push({ name, text: text('entity-feature-text')?.trim() ?? '' });
      for (const id of ['entity-feature-name', 'entity-feature-text']) {
        const input = field(id);
        if (input) input.value = '';
      }
      part('entity-features').innerHTML = featuresHtml(draft);
      paintLive();
    } else if (clicked.id === 'entity-notice-diff' && stored !== null) tell(EVENT_HISTORY, { id: stored.id });
  });

  // The page's bus: the rail asks for a sheet, or for the one on show.
  const view = doc.defaultView;
  if (view) {
    const listen = (name: string, handle: (detail: OpenRequest) => void): void => {
      const listener = (event: Event): void => {
        // A sheet that left the page stops listening.
        if (!host.isConnected) return doc.removeEventListener(name, listener);
        handle((event as CustomEvent<OpenRequest | null>).detail ?? {});
      };
      doc.addEventListener(name, listener);
    };
    listen(BUS_OPEN, (asked) => {
      const wanted = parseTarget(asked.new ? `${NEW_PREFIX}${asked.new}` : asked.id);
      if (wanted !== null) request(wanted);
    });
    listen(BUS_QUERY, () => {
      if (opened !== null) doc.dispatchEvent(new view.CustomEvent(BUS_OPENED, { detail: opened }));
    });
  }

  // The Micro-UIs of a screen mount in no set order: a campaign the rail publishes once this sheet is open still reaches the lists beside it.
  const stopFollowing = shell.context?.on?.(() => {
    if (!host.isConnected) return stopFollowing?.();
    if (draft === null || campaign() === listsFor) return;
    void readLists().then(() => {
      // A command running holds the sheet still; its end paints it again.
      if (busy) return;
      collect();
      paintAll();
    });
  });

  /** The data moved under the sheet (a restore from VersionHistory, the AI, an editor): a clean sheet reads itself again, a modified one keeps its draft. */
  async function follow(): Promise<void> {
    if (busy || target === null || stored === null || isNew()) return;
    collect();
    if (dirty()) return;
    const mine = opening;
    const id = stored.id;
    let fresh: Entity | null;
    try {
      fresh = await read(id);
    } catch {
      // The next move of the data tries again.
      return;
    }
    if (fresh === null || mine !== opening || busy || stored === null || stored.id !== id) return;
    // What was typed while the sheet was read is a draft now.
    collect();
    if (dirty() || !isDirty(fresh, stored)) return;
    stored = fresh;
    draft = structuredClone(fresh);
    dm.url = fresh.ddb?.url ?? '';
    ddbDiff = null;
    paintAll();
  }

  // The data moves under the sheet: its first word only says where the data stands, which the first read has.
  let firstWord = true;
  const stopWatching: { stop?: () => void } = {};
  stopWatching.stop = shell.onDataVersion?.(() => {
    // A sheet the shell took off the page lets go of the poll.
    if (!host.isConnected) return stopWatching.stop?.();
    if (firstWord) firstWord = false;
    else void follow();
  });

  // The first sheet: the screen's prop, else the address's (`?id=`, or `?new=` for a sheet to create).
  const query = new URLSearchParams(view?.location.search ?? '');
  const first = parseTarget(props.entity) ?? parseTarget(query.get('id') || (query.get('new') ? `${NEW_PREFIX}${query.get('new')}` : null));
  if (first === null) say({ kind: 'error', text: 'Aucune fiche choisie.' });
  else void open(first);

  return {
    update(next) {
      const wanted = parseTarget(next.entity);
      if (wanted !== null) request(wanted);
    },
  };
}
