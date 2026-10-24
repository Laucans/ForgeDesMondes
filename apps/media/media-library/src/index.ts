// MediaLibrary (system media): the DM's battlemaps and illustrations with
// their place. It reads `media.listMedia`, `session.listScenes` and
// `campaign.listPlaces` and triggers `media.uploadMedia`,
// `media.setMediaPlace` and `session.addPlaceToSession`, by identifier,
// through the shell it is handed: no network call of its own.
//
// session-media-by-place: a medium is attached to the session when its place
// is in one of the session's scenes. The card never stores that: after every
// command it reads the three Capabilities again, so adding a place to the
// session updates every card of that place, and a medium's new place updates
// its association.

import { ADD_PLACE_TO_SESSION, BATTLEMAP_CONTEXT, LIST_MEDIA, LIST_PLACES, LIST_SCENES, SET_MEDIA_PLACE, UPLOAD_MEDIA } from './ids.ts';
import {
  addPlaceCommand,
  byKind,
  commandId,
  isFinal,
  parseMedia,
  parsePlaces,
  parseProps,
  parseSessionPlaces,
  reasons,
  setPlaceCommand,
  settled,
  stem,
  titleFor,
  uploadCommand,
  type Settled,
  type Upload,
} from './model.ts';
import { card, count, fileDrop, message, skeleton, TYPES, uploadFields } from './render.ts';
import { STYLE } from './style.ts';
import type { Draft, Medium, Place, Props, Shell } from './types.ts';

/** How a command is followed until it settles: the server applies it on its own clock. */
export interface Timing {
  sleep(ms: number): Promise<void>;
  everyMs: number;
  /** How many times it asks before saying it has not settled. */
  polls: number;
}

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 400, polls: 120 };

/** How a command ended for the card: applied, still queued behind another one, or not sent or refused. */
type Outcome = 'applied' | 'pending' | 'failed';

/** What the card takes from the browser, so a test can stand in for it. */
export interface Io {
  /** A fresh idempotency key: a replayed command does not act twice. */
  key(): string;
  /** The pixel size of an image file; null when it cannot be read. */
  measure(file: File): Promise<{ width: number; height: number } | null>;
}

const BROWSER: Io = {
  key: () => crypto.randomUUID(),
  async measure(file) {
    if (typeof createImageBitmap !== 'function') return null;
    try {
      const bitmap = await createImageBitmap(file);
      const size = { width: bitmap.width, height: bitmap.height };
      bitmap.close();
      return size;
    } catch {
      return null;
    }
  },
};

export interface Mounted {
  /** The screen changed a prop (`$ctx.session`): the card follows. */
  update(props: Record<string, unknown>): void;
  /** Stops following the data version. */
  destroy(): void;
}

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="MediaLibrary"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'MediaLibrary';
  style.textContent = STYLE;
  doc.head.append(style);
}

const fmtSize = (bytes: number): string => {
  if (bytes < 1024) return `${bytes} o`;
  if (bytes < 1048576) return `${(bytes / 1024).toFixed(1).replace('.', ',')} Ko`;
  return `${(bytes / 1048576).toFixed(1).replace('.', ',')} Mo`;
};

const accepted = (name: string): boolean => TYPES.some((type) => name.toLowerCase().endsWith(`.${type.toLowerCase()}`)) || /\.jpeg$/i.test(name);

/** What the shell calls: fills `host` with the card and wires it. */
export function mount(host: HTMLElement, rawProps: Record<string, unknown>, shell: Shell, timing: Timing = WAITING, io: Io = BROWSER): Mounted {
  ensureStyle(host.ownerDocument);
  host.innerHTML = skeleton();

  const part = (id: string): HTMLElement => {
    const found = host.querySelector<HTMLElement>(`#${id}`);
    if (!found) throw new Error(`MediaLibrary: #${id} is not in the card`);
    return found;
  };

  let props: Props = parseProps(rawProps);
  let media: Medium[] = [];
  let places: Place[] = [];
  let sessionPlaces: Set<string> | null = null;
  let selected: string | null = null;
  let file: File | null = null;
  let draft: Draft = { kind: 'battlemap', title: '', place: '' };
  let busy = false;
  let ticket = 0;
  /** The data version the last read answered as of. */
  let asOf = 0;
  /** A prop changed while a command ran: the card reads again once it is over. */
  let dirty = false;
  /** The newest data version a notice told, whether or not the card could read at that moment. */
  let newest = 0;
  /** The title the card filled from the file, so a second file replaces it and the DM's own is kept. */
  let autoTitle = '';

  const say = (kind: 'ok' | 'error' | 'busy' | 'none', words = ''): void => {
    part('media-message').innerHTML = kind === 'none' ? '' : message(kind, words);
  };

  const paintCards = (): void => {
    const cards = (list: Medium[]): string => list.map((m) => card(m, places, sessionPlaces, props.sessionNumber, m.id === selected)).join('');
    const battlemaps = byKind(media, 'battlemap');
    const illustrations = byKind(media, 'illustration');
    part('media-battlemaps-list').innerHTML = cards(battlemaps);
    part('media-illustrations-list').innerHTML = cards(illustrations);
    part('media-count').textContent = count(battlemaps.length, illustrations.length);
  };
  const paintUpload = (): void => {
    part('media-upload-fields').innerHTML = uploadFields(draft, places);
  };
  const paintAll = (): void => {
    paintCards();
    paintUpload();
  };

  /** While a command runs the card holds still: what is sent is what is on screen. */
  const lock = (locked: boolean): void => {
    for (const control of host.querySelectorAll<HTMLInputElement | HTMLSelectElement | HTMLButtonElement>('#media-library input, #media-library select, #media-library button')) control.disabled = locked;
  };

  /** The three reads, again: the library, the places, the session's scenes. Answers what could not be read, or null when a later read has begun. */
  async function read(): Promise<string | null> {
    const mine = (ticket += 1);
    const withSession = props.session !== '';
    if (props.campaign === '') {
      // No campaign yet (an unresolved prop): nothing is asked, and the card says why instead of showing an empty library.
      media = [];
      places = [];
      sessionPlaces = null;
      selected = null;
      shell.context?.set(BATTLEMAP_CONTEXT, '');
      return 'Aucune campagne n’est ouverte.';
    }
    const [library, campaignPlaces, scenes] = await Promise.allSettled([
      shell.capability(LIST_MEDIA, undefined, { campaignId: props.campaign }),
      shell.capability(LIST_PLACES, undefined, { campaign: props.campaign }),
      withSession ? shell.capability(LIST_SCENES, undefined, { session: props.session }) : Promise.resolve(null),
    ]);
    // A later read has begun: this answer is out of date.
    if (mine !== ticket) return null;
    if (library.status === 'rejected') throw new Error('the library cannot be read');
    asOf = Math.max(0, ...[library, campaignPlaces, scenes].flatMap((r) => (r.status === 'fulfilled' && r.value !== null ? [r.value.asOf] : [])));
    media = parseMedia(library.value.data);
    places = campaignPlaces.status === 'fulfilled' ? parsePlaces(campaignPlaces.value.data) : [];
    sessionPlaces = withSession && scenes.status === 'fulfilled' && scenes.value !== null ? parseSessionPlaces(scenes.value.data) : null;
    if (selected === null || !media.some((m) => m.id === selected)) {
      selected = byKind(media, 'battlemap')[0]?.id ?? null;
      // What is highlighted is what GridSettings shows; with no battlemap it shows none, not another campaign's.
      shell.context?.set(BATTLEMAP_CONTEXT, selected ?? '');
    }
    if (campaignPlaces.status === 'rejected') return 'Les lieux n’ont pas pu être lus.';
    if (withSession && scenes.status === 'rejected') return 'Les scènes de la séance n’ont pas pu être lues.';
    return '';
  }

  /** The command is sent, then followed until the server has applied or refused it. */
  async function send(body: unknown): Promise<Settled> {
    const answer = await shell.command(body);
    const id = commandId(answer);
    let state = settled(answer);
    for (let asked = 0; !isFinal(state.status) && id !== '' && asked < timing.polls; asked += 1) {
      await timing.sleep(timing.everyMs);
      state = settled(await shell.commandResult(id));
    }
    return state;
  }

  /**
   * One command at a time: the card is locked until its end. When the server has applied it, the card reads
   * everything again, so the association follows; otherwise it paints what it already knew.
   */
  async function act(wrong: string, body: () => Promise<unknown>, done: () => string): Promise<Outcome> {
    if (busy) return 'failed';
    busy = true;
    // The card is repainted and its controls disabled meanwhile: the DM's place in the page is kept.
    const focused = host.ownerDocument.activeElement?.id ?? '';
    lock(true);
    let outcome: Outcome = 'failed';
    try {
      const state = await send(await body());
      if (state.status === 'rejected') say('error', `${wrong} Refusé : ${reasons(state.violations)}.`);
      else if (!isFinal(state.status) && state.status !== '') {
        // Queued or parked behind another command: it is not lost, and it will apply; do not send it twice.
        outcome = 'pending';
        say('busy', `La commande est en file d’attente (état : ${state.status}) : elle s’appliquera dès que possible.`);
        await read().catch(() => '');
      } else if (state.status !== 'applied') say('error', `${wrong} (état : ${state.status || 'inconnu'}).`);
      else {
        outcome = 'applied';
        let unread = '';
        try {
          unread = (await read()) ?? '';
        } catch {
          unread = 'La bibliothèque n’a pas pu être relue.';
        }
        say(unread === '' ? 'ok' : 'error', unread === '' ? done() : `${done()} ${unread}`);
      }
    } catch {
      say('error', wrong);
    } finally {
      busy = false;
      paintAll();
      lock(false);
      if (focused !== '') host.ownerDocument.getElementById(focused)?.focus();
      // A prop changed, or another screen moved the data, while the command ran.
      if (dirty || newest > asOf) {
        dirty = false;
        refresh();
      }
    }
    return outcome;
  }

  const selectBattlemap = (id: string): void => {
    selected = id;
    paintCards();
    // GridSettings follows the screen's context.
    shell.context?.set(BATTLEMAP_CONTEXT, id);
  };

  const changePlace = (mediumId: string, value: string): Promise<Outcome> => {
    const m = media.find((x) => x.id === mediumId);
    const place = value === '' ? null : value;
    if (m === undefined || m.place === place) return Promise.resolve('failed');
    const name = places.find((p) => p.id === place)?.name ?? place;
    return act('Le lieu n’a pas pu être changé.', () => Promise.resolve(setPlaceCommand(m.id, place, io.key())), () => (place === null ? `« ${m.title} » n’a plus de lieu.` : `« ${m.title} » est rattaché au lieu « ${name ?? ''} ».`));
  };

  const addPlace = (place: string): Promise<Outcome> => {
    const name = places.find((p) => p.id === place)?.name ?? place;
    return act('Le lieu n’a pas pu être ajouté à la séance.', () => Promise.resolve(addPlaceCommand(props.session, place, io.key())), () => `« ${name} » est dans la séance : ses médias y sont associés.`);
  };

  async function upload(): Promise<void> {
    if (busy) return;
    if (props.campaign === '') return say('error', 'Aucune campagne n’est ouverte : le média ne peut pas lui être ajouté.');
    if (file === null) return say('error', 'Choisissez d’abord une image à téléverser.');
    const chosen = file;
    const title = titleFor(draft, chosen);
    const before = new Set(media.map((m) => m.id));
    const { kind } = draft;
    const { campaign } = props;
    const place = draft.place === '' ? null : draft.place;
    // Measuring is part of the command: the card is locked from the click, so the form cannot move under it.
    const sent = async (): Promise<unknown> => uploadCommand({ campaign, kind, title, file: chosen.name, place, size: await io.measure(chosen) } satisfies Upload, io.key());
    const outcome = await act('Le téléversement a échoué.', sent, () => `« ${title} » est ajouté à la campagne.`);
    // A command still queued is not lost: the form is emptied so the same file is not sent a second time.
    if (outcome === 'failed') return;
    file = null;
    draft = { ...draft, title: '', place: '' };
    autoTitle = '';
    part('media-upload-drop').innerHTML = fileDrop();
    paintUpload();
    // A battlemap arrives with its default grid: GridSettings opens on it.
    const fresh = byKind(media, 'battlemap').find((m) => !before.has(m.id));
    if (fresh !== undefined) selectBattlemap(fresh.id);
  }

  /** The file the DM chose, shown in the kit's FileDrop; a type the card does not take is refused. */
  function choose(chosen: File | undefined): void {
    if (chosen === undefined) return;
    const zone = part('media-upload-drop-zone');
    if (!accepted(chosen.name)) {
      file = null;
      zone.classList.remove('has-file');
      zone.classList.add('has-error');
      return;
    }
    file = chosen;
    zone.classList.remove('has-error');
    zone.classList.add('has-file');
    const text = (selector: string, value: string): void => {
      const target = zone.querySelector(selector);
      if (target) target.textContent = value;
    };
    text('.kit-filedrop__name', chosen.name);
    text('.kit-filedrop__size', fmtSize(chosen.size));
    text('.kit-filedrop__thumb', (chosen.name.split('.').pop() ?? '').toUpperCase());
    if (draft.title.trim() === '' || draft.title === autoTitle) {
      autoTitle = stem(chosen.name);
      draft = { ...draft, title: autoTitle };
      const field = host.querySelector<HTMLInputElement>('#media-upload-title');
      if (field) field.value = draft.title;
    }
  }

  host.addEventListener('submit', (event) => event.preventDefault());

  host.addEventListener('change', (event) => {
    // Read by shape, not by class: the card runs in whichever window the shell is.
    const target = event.target as { id?: string; name?: string; value?: string; files?: FileList | null; dataset?: DOMStringMap; closest?: (selector: string) => HTMLElement | null } | null;
    if (target?.dataset?.writes === SET_MEDIA_PLACE) {
      const li = target.closest?.('li[data-id]');
      if (li?.dataset.id !== undefined) void changePlace(li.dataset.id, target.value ?? '');
    } else if (target?.id === 'media-upload-file') choose(target.files?.[0]);
    else if (target?.id === 'media-upload-place') draft = { ...draft, place: target.value ?? '' };
    else if (target?.name === 'media-kind') draft = { ...draft, kind: target.value === 'illustration' ? 'illustration' : 'battlemap' };
  });

  host.addEventListener('input', (event) => {
    const target = event.target as { id?: string; value?: string } | null;
    if (target?.id === 'media-upload-title') draft = { ...draft, title: target.value ?? '' };
  });

  host.addEventListener('dragover', (event) => event.preventDefault());
  host.addEventListener('drop', (event) => {
    if (!(event.target as Element | null)?.closest('#media-upload-zone')) return;
    event.preventDefault();
    choose(event.dataTransfer?.files[0]);
  });

  host.addEventListener('click', (event) => {
    const button = (event.target as Element | null)?.closest('button');
    if (!button) return;
    const action = button.dataset.action ?? '';
    // The actuator says which DataCapability it triggers: that, and nothing else, is what it sends.
    if (button.dataset.writes === UPLOAD_MEDIA) void upload();
    else if (button.dataset.writes === ADD_PLACE_TO_SESSION && action.startsWith('add-place:')) void addPlace(action.slice('add-place:'.length));
    else if (action.startsWith('grid:')) selectBattlemap(action.slice('grid:'.length));
  });

  /** Reads everything and paints; what cannot be read is said, not hidden. */
  function refresh(): void {
    lock(true);
    read().then(
      (unread) => {
        // A later read has begun: it paints and unlocks the card when it ends.
        if (unread === null) return;
        paintAll();
        say(unread === '' ? 'none' : 'error', unread);
        lock(false);
      },
      () => {
        say('error', 'Impossible de lire la bibliothèque de médias.');
        lock(false);
      },
    );
  }

  // Another screen's command moved the data (session preparation added a place): read again when the
  // version is newer than the one the last read answered as of. A card taken out of the page stops listening.
  const following: { stop?: () => void } = {};
  following.stop = shell.onDataVersion?.((version) => {
    newest = Math.max(newest, version);
    if (!host.isConnected) following.stop?.();
    else if (!busy && version > asOf) refresh();
  });

  paintAll();
  say('busy', 'Chargement des médias…');
  refresh();

  return {
    update(next) {
      const before = props;
      props = parseProps(next);
      if (props.campaign === before.campaign && props.session === before.session && props.sessionNumber === before.sessionNumber) return;
      if (busy) dirty = true;
      else refresh();
    },
    destroy() {
      following.stop?.();
    },
  };
}
