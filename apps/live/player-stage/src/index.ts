// PlayerStage (system live): what the players see — the campaign cover, an
// illustration, or the battlemap frame — and the floating illustration window
// over it. Both player views read the same diffusion (`single-diffusion-source`):
// the projected screen draws it, the DM's piloted tab draws it too and lets
// the DM drag the window by its bar (`live.moveWindow`) and resize it by its
// corner (`live.resizeWindow`), the image's ratio always kept
// (`window-proportional`). It reads `live.getDiffusion`, `campaign.getCampaign`
// and `media.listMedia` and triggers the two window commands, by identifier,
// through the shell it is handed: no network call of its own. It draws no HP
// figure, no note and no hidden token (`players-see-band-only`): the tokens over
// a battlemap are the Battlemap's, in view 'player', composed by the screen.

import { BATTLEMAP_CONTEXT, GET_CAMPAIGN, GET_DIFFUSION, LIST_MEDIA, MOVE_WINDOW, RESIZE_WINDOW } from './ids.ts';
import {
  commandBody,
  commandId,
  dragged,
  illustrationOf,
  isFinal,
  moved,
  parseCampaign,
  parseDiffusion,
  parseMedia,
  parseProps,
  reasons,
  resized,
  settled,
  type Settled,
} from './model.ts';
import { art, cover, mapSlot, scene, skeleton } from './render.ts';
import { STYLE } from './style.ts';
import type { Box, Campaign, Diffusion, Media, Props, Shell, Stage } from './types.ts';

/** How a command is followed until it settles, and how often the diffusion is asked: the projected screen follows within a second. */
export interface Timing {
  sleep(ms: number): Promise<void>;
  everyMs: number;
  /** How many times it asks before saying it has not settled. */
  polls: number;
  watchMs: number;
}

const WAITING: Timing = { sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)), everyMs: 300, polls: 60, watchMs: 500 };

/** What the shell keeps of a mounted Micro-UI: new props follow the screen's context. */
export interface Mounted {
  update(raw: Record<string, unknown>): void;
}

/** One drag of the DM's: the bar moves the window, the corner resizes it. */
interface Gesture {
  mode: 'move' | 'resize';
  x: number;
  y: number;
  from: Box;
  ratio: number;
  stage: Stage;
  moved: boolean;
  to: Box;
  /** The box the corner drew, as live.resizeWindow is asked for it. */
  box: { w: number; h: number };
}

const round = (value: number): number => Number(value.toFixed(4));

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="PlayerStage"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'PlayerStage';
  style.textContent = STYLE;
  doc.head.append(style);
}

/**
 * What the shell calls: fills `host` with the stage and wires it. `composed` is
 * told when the base holds a micro-frontend to compose (the Battlemap).
 */
export function mount(host: HTMLElement, raw: Record<string, unknown>, shell: Shell, timing: Timing = WAITING, composed: (base: HTMLElement) => void = () => undefined): Mounted {
  ensureStyle(host.ownerDocument);

  let props: Props = parseProps(raw, host.ownerDocument.defaultView?.location.search ?? '');
  let diffusion: Diffusion = parseDiffusion(undefined);
  let campaign: Campaign | null = null;
  let media: Media = parseMedia(undefined);
  let campaignRead = false;
  let mediaRead = false;
  let loaded = false;
  /** The window as the DM holds it while a drag or its commands are under way. */
  let local: Box | null = null;
  let inFlight = 0;
  let gesture: Gesture | null = null;
  let baseKey = '';
  let generation = 0;
  let readNo = 0;
  /** The data version of the last diffusion read. */
  let readAt: number | null = null;
  /** The images the library was asked for and did not list. */
  let missing = new Set<string>();
  let stopWatching: () => void = () => undefined;

  const part = <T extends HTMLElement = HTMLElement>(id: string): T | null => host.querySelector<T>(`#${id}`);
  const stageSize = (): Stage => {
    const stage = part('player-stage');
    return { width: stage?.clientWidth || 1, height: stage?.clientHeight || 1 };
  };

  // ---------- painting ----------
  function say(text: string): void {
    const box = part('player-status');
    if (!box) return;
    box.textContent = text;
    box.hidden = text === '';
  }

  function paintBase(): void {
    const box = part('player-base');
    if (!box) return;
    const { base, view } = diffusion;
    let instance = '';
    if (base.kind === 'battlemap' && base.id !== '') {
      instance = base.id;
      const key = `battlemap:${base.id}`;
      if (key !== baseKey) {
        baseKey = key;
        box.innerHTML = mapSlot(base.id, props.pilot);
        composed(box);
      }
      // The frame the DM gave the map, for the Battlemap composed here.
      const slot = box.querySelector<HTMLElement>('#player-map');
      if (slot && view !== null) {
        slot.dataset.zoom = String(view.zoom);
        slot.dataset.panX = String(view.panX);
        slot.dataset.panY = String(view.panY);
      }
    } else {
      const image = base.kind === 'illustration' ? illustrationOf(media, base.id) : undefined;
      const html = image ? scene(image) : cover(campaign, media);
      if (html !== baseKey) {
        baseKey = html;
        box.innerHTML = html;
      }
    }
    // The screen composes the Battlemap beside this stage: it is told which instance to show.
    shell.context?.set(BATTLEMAP_CONTEXT, instance);
  }

  /** The window the row holds, moved to where the DM has it while he drags. */
  function current(): (Box & { media: string }) | null {
    return diffusion.window === null ? null : { ...diffusion.window, ...local };
  }

  function paintWindow(): void {
    const figure = part('player-window');
    if (!figure) return;
    const win = current();
    const image = win === null ? undefined : illustrationOf(media, win.media);
    if (win === null || image === undefined) {
      figure.hidden = true;
      return;
    }
    figure.hidden = false;
    figure.style.left = `${win.x * 100}%`;
    figure.style.top = `${win.y * 100}%`;
    figure.style.width = `${win.w * 100}%`;
    figure.style.aspectRatio = String(image.ratio);
    const bar = part('player-window-bar');
    if (bar) bar.textContent = image.title;
    const picture = part('player-window-art');
    if (picture && picture.dataset.media !== image.id) {
      picture.dataset.media = image.id;
      picture.innerHTML = art(image);
    }
  }

  function paintAll(): void {
    paintBase();
    paintWindow();
  }

  // ---------- reading ----------
  /** Reads the diffusion again; the campaign and the library when the stage lacks them or the diffusion names an image it does not know. An older answer never replaces a newer one. */
  async function refresh(): Promise<void> {
    const mine = generation;
    const no = (readNo += 1);
    try {
      const read = await shell.capability(GET_DIFFUSION, undefined, { view: props.pilot ? 'dm' : 'player' });
      if (mine !== generation || no < readNo) return;
      const next = parseDiffusion(read.data);
      const unknown = (id: string): boolean => id !== '' && illustrationOf(media, id) === undefined && !missing.has(id);
      const wantsMedia = !mediaRead || unknown(next.window?.media ?? '') || (next.base.kind === 'illustration' && unknown(next.base.id));
      const [named, listed] = await Promise.all([
        campaignRead ? undefined : shell.capability(GET_CAMPAIGN, undefined, {}).catch(() => undefined),
        wantsMedia ? shell.capability(LIST_MEDIA, undefined, {}).catch(() => undefined) : undefined,
      ]);
      if (mine !== generation || no < readNo) return;
      if (named) {
        campaign = parseCampaign(named.data);
        campaignRead = true;
      }
      if (listed) {
        media = parseMedia(listed.data);
        mediaRead = true;
        missing = new Set([next.window?.media ?? '', next.base.id].filter((id) => illustrationOf(media, id) === undefined));
      }
      diffusion = next;
      readAt = read.asOf;
      loaded = true;
      paintAll();
    } catch {
      if (mine !== generation || no < readNo) return;
      if (props.pilot) say('Impossible de lire la diffusion.');
      if (!loaded) {
        loaded = true;
        paintAll();
      }
    }
  }

  async function start(): Promise<void> {
    generation += 1;
    const mine = generation;
    stopWatching();
    diffusion = parseDiffusion(undefined);
    campaign = null;
    media = parseMedia(undefined);
    campaignRead = false;
    mediaRead = false;
    missing = new Set();
    readAt = null;
    loaded = false;
    local = null;
    inFlight = 0;
    gesture = null;
    baseKey = '';
    host.innerHTML = skeleton(props.pilot);
    await refresh();
    if (mine !== generation) return;
    // The version a read was made at is not news; what moved since the last read is.
    stopWatching = shell.onDataVersion((version) => {
      if (version !== readAt) void refresh();
    }, timing.watchMs);
  }

  // ---------- commands ----------
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

  /** One drag, sent as the command of what it changed; the window is read again once none is left under way, so what the server refused goes back where it was. */
  async function commit(g: Gesture): Promise<void> {
    const mine = generation;
    inFlight += 1;
    say('');
    try {
      const campaignId = campaign?.id ?? '';
      const body =
        g.mode === 'move'
          ? commandBody(MOVE_WINDOW, { campaign: campaignId, x: round(g.to.x), y: round(g.to.y) })
          : commandBody(RESIZE_WINDOW, { campaign: campaignId, w: round(g.box.w), h: round(g.box.h) });
      const state = await send(body);
      if (state.status === 'rejected') say(`Fenêtre non modifiée : ${reasons(state.violations)}.`);
      else if (state.status !== 'applied') say(`La commande n’a pas abouti (état : ${state.status || 'inconnu'}).`);
    } catch {
      say('La commande n’a pas pu être envoyée.');
    } finally {
      // A stage started anew since counts its own commands.
      if (mine === generation) {
        inFlight -= 1;
        if (inFlight === 0) {
          // A drag begun meanwhile keeps the window where the DM holds it.
          if (gesture === null) local = null;
          await refresh();
        }
      }
    }
  }

  // ---------- the DM's gestures ----------
  host.addEventListener('pointerdown', (event) => {
    if (!props.pilot || gesture || event.button !== 0) return;
    const target = event.target as Element | null;
    const mode = target?.closest('#player-window-resize') ? 'resize' : target?.closest('#player-window-bar') ? 'move' : null;
    const win = current();
    const image = win === null ? undefined : illustrationOf(media, win.media);
    if (mode === null || win === null || image === undefined) return;
    event.preventDefault();
    // A command names its campaign: without it nothing is sent, and the next read tries again.
    if (campaign === null) {
      campaignRead = false;
      say('La campagne n’a pas pu être lue : la fenêtre ne peut pas être modifiée.');
      void refresh();
      return;
    }
    const from = { x: win.x, y: win.y, w: win.w };
    gesture = { mode, x: event.clientX, y: event.clientY, from, ratio: image.ratio, stage: stageSize(), moved: false, to: from, box: { w: from.w, h: from.w / image.ratio } };
    part('player-window')?.setPointerCapture?.(event.pointerId);
  });

  host.addEventListener('pointermove', (event) => {
    const g = gesture;
    if (!g) return;
    const [dx, dy] = [event.clientX - g.x, event.clientY - g.y];
    if (dx === 0 && dy === 0 && !g.moved) return;
    g.moved = true;
    if (g.mode === 'move') g.to = moved(g.from, dx, dy, g.ratio, g.stage);
    else {
      g.box = dragged(g.from, dx, dy, g.ratio, g.stage);
      g.to = resized(g.from, g.box, g.ratio, g.stage);
    }
    local = g.to;
    paintWindow();
  });

  const release = (event: Event): void => {
    const g = gesture;
    if (!g) return;
    gesture = null;
    if (event.type === 'pointercancel' || !g.moved) {
      if (inFlight === 0) local = null;
      paintWindow();
      return;
    }
    void commit(g);
  };
  host.addEventListener('pointerup', release);
  host.addEventListener('pointercancel', release);

  void start();

  return {
    update(next) {
      const before = props;
      props = parseProps(next, host.ownerDocument.defaultView?.location.search ?? '');
      if (props.pilot !== before.pilot) void start();
    },
  };
}
