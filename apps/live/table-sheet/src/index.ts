// TableSheet (system live): the sheet of one element — a PC, an NPC, a place, an
// event — read, never edited in place, and laid out for the table. It reads
// `campaign.getEntity` and `media.listMedia`; a link leads to the entity editor;
// the element's illustration is sent to the players in scene
// (`live.showOnPlayerView`) or in a floating window
// (`live.showWindowOnPlayerView`), by identifier, through the shell it is handed:
// no network call of its own. A place lists the characters met there, each
// opening its own sheet on a click: the same `open` event the DM view routes
// from the elements panel.

import { GET_ENTITY, LIST_MEDIA, OPEN_EVENT, OPEN_MAP_EVENT, SHOW_ON_PLAYER_VIEW, SHOW_WINDOW_ON_PLAYER_VIEW } from './ids.ts';
import { commandBody, commandId, illustrationFor, illustrationOf, isFinal, parseEntity, parseMedia, parseProps, parseTarget, reasons, refOf, settled, type Settled } from './model.ts';
import { notice, sheet, skeleton, viewOf, viewOfIllustration } from './render.ts';
import { STYLE } from './style.ts';
import type { Illustration, Media, Props, Ref, Shell, Target } from './types.ts';

/** How a command is followed until it settles. */
export interface Timing {
  sleep(ms: number): Promise<void>;
  everyMs: number;
  /** How many times it asks before saying it has not settled. */
  polls: number;
}

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 300, polls: 60 };

/** What the shell keeps of a mounted Micro-UI: new props follow the screen's context. */
export interface Mounted {
  update(raw: Record<string, unknown>): void;
}

type Mode = 'scene' | 'window';

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="TableSheet"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'TableSheet';
  style.textContent = STYLE;
  doc.head.append(style);
}

/** What the shell calls: fills `host` with the sheet of the element the props name, and follows them. */
export function mount(host: HTMLElement, raw: Record<string, unknown>, shell: Shell, timing: Timing = WAITING): Mounted {
  const doc = host.ownerDocument;
  ensureStyle(doc);

  let props: Props = parseProps(raw);
  let shown: Target = { type: props.type, id: props.id };
  /** The campaign a send names: the screen's, else the one the entity's answer gives. */
  let campaign = props.campaign;
  /** The illustration the open element's send buttons name. */
  let image: Illustration | undefined;
  let generation = 0;
  let sending = false;

  const part = <T extends HTMLElement = HTMLElement>(id: string): T | null => host.querySelector<T>(`#${id}`);

  // ---------- painting ----------
  function say(message: string): void {
    const box = part('sheet-status');
    if (!box) return;
    box.textContent = message;
    box.hidden = message === '';
  }

  function busy(on: boolean): void {
    for (const button of host.querySelectorAll<HTMLButtonElement>('button[data-writes]')) button.disabled = on;
  }

  function put(head: string, body: string): void {
    const [headBox, bodyBox] = [part('sheet-head'), part('sheet-body')];
    if (headBox) headBox.innerHTML = head;
    if (bodyBox) bodyBox.innerHTML = body;
  }

  /** The characters a place lets the DM meet, each read for its name; one with no sheet is left out. */
  async function met(ids: readonly string[]): Promise<Ref[]> {
    const read = await Promise.all(ids.map((id) => shell.capability(GET_ENTITY, undefined, { id }).then((answer) => refOf(answer.data), () => null)));
    return read.filter((ref): ref is Ref => ref !== null);
  }

  // ---------- reading ----------
  async function load(target: Target): Promise<void> {
    generation += 1;
    const mine = generation;
    sending = false;
    image = undefined;
    campaign = props.campaign;
    host.innerHTML = skeleton();
    if (target.id === '') {
      put(notice('sheet-empty', 'Aucun élément ouvert.'), '');
      return;
    }
    try {
      const entity = target.type === 'illustration' ? null : parseEntity((await shell.capability(GET_ENTITY, undefined, { id: target.id })).data, target);
      if (mine !== generation) return;
      if (target.type !== 'illustration' && entity === null) {
        put(notice('sheet-missing', 'Élément introuvable.'), '');
        return;
      }
      campaign = props.campaign || entity?.campaign || '';
      let media: Media = { illustrations: [] };
      try {
        media = parseMedia((await shell.capability(LIST_MEDIA, undefined, campaign === '' ? {} : { campaign })).data);
      } catch (error) {
        // An element is read without its image; an image has nothing else to show.
        if (entity === null) throw error;
      }
      const characters = entity?.type === 'place' ? await met(entity.characters) : [];
      if (mine !== generation) return;
      let view;
      if (entity === null) {
        const picked = illustrationOf(media, target.id);
        if (picked === undefined) {
          put(notice('sheet-missing', 'Élément introuvable.'), '');
          return;
        }
        view = viewOfIllustration(picked);
      } else {
        view = viewOf(entity, illustrationFor(entity, media), characters);
      }
      image = view.sends;
      const { head, body } = sheet(view);
      put(head, body);
    } catch {
      if (mine === generation) put(notice('sheet-error', 'Impossible de lire la fiche.'), '');
    }
  }

  // ---------- sending ----------
  /** The command is sent, then followed until the server has applied or refused it. */
  async function submit(body: unknown): Promise<Settled> {
    const answer = await shell.command(body);
    const id = commandId(answer);
    let state = settled(answer);
    for (let asked = 0; !isFinal(state.status) && id !== '' && asked < timing.polls; asked += 1) {
      await timing.sleep(timing.everyMs);
      state = settled(await shell.commandResult(id));
    }
    return state;
  }

  async function send(mode: Mode, media: string): Promise<void> {
    const picked = image;
    if (sending || picked === undefined || picked.id !== media) return;
    // A window names its campaign; a base may leave it to the illustration's.
    if (mode === 'window' && campaign === '') {
      say('La campagne n’est pas connue : l’illustration ne peut pas être envoyée en fenêtre.');
      return;
    }
    const named = campaign === '' ? {} : { campaign };
    const body = mode === 'scene' ? commandBody(SHOW_ON_PLAYER_VIEW, { ...named, kind: 'illustration', media }) : commandBody(SHOW_WINDOW_ON_PLAYER_VIEW, { ...named, media });
    const mine = generation;
    sending = true;
    busy(true);
    say('');
    let message: string;
    try {
      const state = await submit(body);
      if (state.status === 'applied') message = `« ${picked.title} » est envoyée aux joueurs, en mode ${mode === 'scene' ? 'scène' : 'fenêtre'}.`;
      else if (state.status === 'rejected') message = `Envoi refusé : ${reasons(state.violations)}.`;
      else message = `La commande n’a pas abouti (état : ${state.status || 'inconnu'}).`;
    } catch {
      message = 'La commande n’a pas pu être envoyée.';
    }
    // The sheet has moved to another element meanwhile: its own buttons are not this send's to free.
    if (mine !== generation) return;
    sending = false;
    busy(false);
    say(message);
  }

  // ---------- the DM's gestures ----------
  const raise = (name: string, detail: Record<string, unknown>): void => {
    const win = doc.defaultView;
    if (win) doc.dispatchEvent(new win.CustomEvent(name, { detail }));
  };

  host.addEventListener('click', (event) => {
    const button = (event.target as Element | null)?.closest('button');
    if (!button || !host.contains(button)) return;
    const { open, action, media } = button.dataset;
    if (open !== undefined) {
      const at = open.indexOf(':');
      raise(OPEN_EVENT, { type: open.slice(0, at), id: open.slice(at + 1) });
    } else if (action === 'to-map') raise(OPEN_MAP_EVENT, { place: shown.id });
    else if (action === 'send-scene' || action === 'send-window') void send(action === 'send-scene' ? 'scene' : 'window', media ?? '');
  });

  /** An element opened anywhere on the screen — the panel, the suggestions, a place's chip — is shown here. */
  const onOpen = (event: Event): void => {
    // The host left the page: nothing follows the screen any more.
    if (!host.isConnected) {
      doc.removeEventListener(OPEN_EVENT, onOpen);
      return;
    }
    const detail = (event as CustomEvent<unknown>).detail;
    const target = parseTarget(typeof detail === 'object' && detail !== null ? (detail as Record<string, unknown>) : {});
    if (target.id === '') return;
    shown = target;
    void load(shown);
  };
  doc.addEventListener(OPEN_EVENT, onOpen);

  void load(shown);

  return {
    update(next) {
      const before = props;
      props = parseProps(next);
      if (props.type === before.type && props.id === before.id && props.campaign === before.campaign) return;
      shown = { type: props.type, id: props.id };
      void load(shown);
    },
  };
}
