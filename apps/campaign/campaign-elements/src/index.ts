// CampaignElements (system campaign): the elements of a campaign, in two
// modes of its `mode` prop. Tabs (default): a grid of players, characters,
// places, events, media and sounds. Rail: a list of PJ, PNJ, places and events
// beside an entity sheet, with a fuzzy search on names and aliases (AliasMatch@1),
// sections that fold, a whole rail that folds (and is remembered), a drawer
// under 960 px and « Nouveau… » buttons. It reads the six Capabilities of its
// micro-ui.json by identifier, through the shell it is handed, and triggers no
// DataCapability: a click on an element only asks the sheet to open it (the
// bus), the sheet is not this Micro-UI's.

import { LIST_CHARACTERS, LIST_EVENTS, LIST_MEDIA, LIST_PLACES, LIST_PLAYERS, LIST_SOUNDS } from './ids.ts';
import {
  countText,
  parseCharacters,
  parseElements,
  parseMedia,
  parsePlayers,
  parseSounds,
  railIndex,
  readFolded,
  search,
  writeFolded,
  type Memory,
} from './model.ts';
import {
  charactersPanel,
  eventsPanel,
  mediaPanel,
  NEW_KINDS,
  placesPanel,
  playersPanel,
  railGroups,
  railSkeleton,
  soundsPanel,
  tabLabel,
  TABS,
  tabsSkeleton,
  unreadable,
} from './render.ts';
import { STYLE } from './style.ts';
import type { Bus, Character, Group, Media, Named, Opened, Player, Props, RailEntry, Shell, Sound } from './types.ts';

/** What the shell keeps of the panel: it follows its props. */
export interface Mounted {
  /** The props changed — another campaign, or the other mode: read again. */
  update(props: Props): void;
}

type Status = 'pending' | 'ok' | 'failed';

/** The drawer under this width. */
const NARROW = '(max-width: 960px)';

/** The sheet's side of the screen, as the mock-up words it: DOM events on the document, `fdm:` before the name. */
export function domBus(doc: Document): Bus {
  const win = doc.defaultView;
  const send = (name: string, detail: unknown): void => {
    if (win) doc.dispatchEvent(new win.CustomEvent(`fdm:${name}`, { detail }));
  };
  return {
    open: (request) => send('open-entity', request),
    navigate(href) {
      if (win) win.location.href = href;
    },
    query: () => send('entity-query', {}),
    onOpened(listener) {
      const handler = (event: Event): void => listener((event as CustomEvent<Opened | null>).detail ?? {});
      doc.addEventListener('fdm:entity-opened', handler);
      return () => doc.removeEventListener('fdm:entity-opened', handler);
    },
  };
}

function ensureStyle(doc: Document): void {
  if (doc.querySelector('style[data-micro-ui="CampaignElements"]')) return;
  const style = doc.createElement('style');
  style.dataset.microUi = 'CampaignElements';
  style.textContent = STYLE;
  doc.head.append(style);
}

/** The browser's local storage, which a browser may refuse to hand out. */
function memoryOf(doc: Document): Memory | null {
  try {
    return doc.defaultView?.localStorage ?? null;
  } catch {
    return null;
  }
}

/** What the shell calls: fills `host` with the panel of the mode and wires it. */
export function mount(host: HTMLElement, props: Props, shell: Shell, bus: Bus = domBus(host.ownerDocument)): Mounted {
  const doc = host.ownerDocument;
  const win = doc.defaultView;
  const memory = memoryOf(doc);
  ensureStyle(doc);

  let current: Props = props;
  const modeOf = (p: Props): 'tabs' | 'rail' => (p.mode === 'rail' ? 'rail' : 'tabs');
  const campaignOf = (p: Props): string => (p.campaign ?? '').trim() || (new URLSearchParams(win?.location.search ?? '').get('campaign') ?? '').trim();

  /** The campaign on show is the screen's: the sheet beside the rail reads its lists and files its new sheets there. */
  const publish = (p: Props): void => {
    const campaign = campaignOf(p);
    if (campaign !== '') shell.context?.set('campaign', campaign);
  };
  publish(current);

  let players: Player[] = [];
  let characters: Character[] = [];
  let places: Named[] = [];
  let events: Named[] = [];
  let media: Media[] = [];
  let sounds: Sound[] = [];
  const status = new Map<string, Status>();
  let generation = 0;

  let query = '';
  let shown: string = new URLSearchParams(win?.location.search ?? '').get('id') ?? '';
  const folded = new Set<Group>();
  const mobile = typeof win?.matchMedia === 'function' ? win.matchMedia(NARROW) : null;
  const narrow = (): boolean => mobile?.matches === true;

  /** A part is drawn again only when what it shows changed: a poll that finds the same lists keeps the focus and the scroll. */
  const drawn = new WeakMap<Element, string>();
  const put = (el: HTMLElement, html: string): void => {
    if (drawn.get(el) === html && el.isConnected) return;
    drawn.set(el, html);
    el.innerHTML = html;
  };

  const part = <T extends HTMLElement>(id: string): T => {
    const found = host.querySelector<T>(`#${id}`);
    if (!found) throw new Error(`CampaignElements: #${id} is not in the panel`);
    return found;
  };

  // ---- tabs

  /** Per tab: the answer it shows, the others it waits for, how it draws and what it counts. */
  const sections: Record<string, { own: string; also: string[]; html(): string; count(): number }> = {
    players: { own: LIST_PLAYERS, also: [LIST_CHARACTERS], html: () => playersPanel(players, characters, status.get(LIST_CHARACTERS) === 'failed', campaignOf(current)), count: () => players.length },
    characters: { own: LIST_CHARACTERS, also: [], html: () => charactersPanel(characters, campaignOf(current)), count: () => characters.length },
    places: { own: LIST_PLACES, also: [], html: () => placesPanel(places, campaignOf(current)), count: () => places.length },
    events: { own: LIST_EVENTS, also: [], html: () => eventsPanel(events, campaignOf(current)), count: () => events.length },
    media: { own: LIST_MEDIA, also: [], html: () => mediaPanel(media, campaignOf(current)), count: () => media.length },
    sounds: { own: LIST_SOUNDS, also: [], html: () => soundsPanel(sounds, campaignOf(current)), count: () => sounds.length },
  };

  function paintTabs(): void {
    TABS.forEach(([key, label], i) => {
      const section = sections[key];
      if (!section) return;
      const waiting = [section.own, ...section.also].some((id) => (status.get(id) ?? 'pending') === 'pending');
      const failed = status.get(section.own) === 'failed';
      const ready = !waiting && !failed;
      put(part(`elements-tabs-panel-${i}`), failed ? unreadable('cette liste') : ready ? section.html() : '<p class="small muted">Chargement…</p>');
      part(`elements-tabs-tab-${i}`).textContent = ready ? tabLabel(label, section.count()) : label;
    });
  }

  function selectTab(index: number): void {
    TABS.forEach((_, i) => {
      part(`elements-tabs-tab-${i}`).setAttribute('aria-selected', String(i === index));
      part(`elements-tabs-panel-${i}`).hidden = i !== index;
    });
  }

  // ---- rail

  let index: RailEntry[] = [];

  function paintRail(): void {
    const found = search(index, query);
    const failed = NEEDED.rail.some((id) => status.get(id) === 'failed');
    put(part('rail-groups'), (failed ? unreadable('une partie des éléments') : '') + railGroups(found, folded, shown, campaignOf(current)));
    part('rail-count').textContent = countText(query, found.length);
  }

  /** The rail's own state, drawn on its markup: folded to a strip on a wide screen, open as a drawer on a narrow one. */
  function syncRail(): void {
    const rail = part('elements-rail');
    const open = rail.dataset.open === 'true';
    const strip = rail.dataset.collapsed === 'true';
    const toggle = part('rail-toggle');
    toggle.textContent = narrow() ? '✕' : strip ? '»' : '«';
    toggle.title = narrow() ? 'Fermer la liste' : strip ? 'Déplier la liste' : 'Replier la liste';
    toggle.setAttribute('aria-expanded', String(narrow() ? open : !strip));
    part('rail-open').setAttribute('aria-expanded', String(open));
    part('rail-veil').hidden = !open;
  }

  function setOpen(open: boolean): void {
    part('elements-rail').dataset.open = String(open);
    syncRail();
  }

  function setFolded(strip: boolean): void {
    part('elements-rail').dataset.collapsed = String(strip);
    writeFolded(memory, strip);
    syncRail();
  }

  function toggleGroup(button: HTMLElement): void {
    const group = button.dataset.group as Group;
    if (folded.has(group)) folded.delete(group);
    else folded.add(group);
    const closed = folded.has(group);
    button.setAttribute('aria-expanded', String(!closed));
    const caret = button.querySelector('.mf-rail__caret');
    if (caret) caret.textContent = closed ? '▸' : '▾';
    part(`rail-list-${group}`).hidden = closed;
  }

  /** The sheet that is open: the editor tells us, so the list follows a click, a « Nouveau… » or the first load. */
  function follow(opened: Opened): void {
    shown = opened.id ?? '';
    for (const link of host.querySelectorAll<HTMLElement>('.mf-rail__entry')) {
      const on = link.dataset.entity === shown;
      link.classList.toggle('is-current', on);
      if (on) link.setAttribute('aria-current', 'true');
      else link.removeAttribute('aria-current');
    }
  }

  // ---- reading

  const readers: Record<string, (data: unknown) => void> = {
    [LIST_PLAYERS]: (data) => (players = parsePlayers(data)),
    [LIST_CHARACTERS]: (data) => (characters = parseCharacters(data)),
    [LIST_PLACES]: (data) => (places = parseElements(data)),
    [LIST_EVENTS]: (data) => (events = parseElements(data)),
    [LIST_MEDIA]: (data) => (media = parseMedia(data)),
    [LIST_SOUNDS]: (data) => (sounds = parseSounds(data)),
  };
  const NEEDED: Record<'tabs' | 'rail', string[]> = {
    tabs: [LIST_PLAYERS, LIST_CHARACTERS, LIST_PLACES, LIST_EVENTS, LIST_MEDIA, LIST_SOUNDS],
    rail: [LIST_CHARACTERS, LIST_PLACES, LIST_EVENTS],
  };

  function paint(): void {
    if (modeOf(current) === 'tabs') return paintTabs();
    index = railIndex(characters, places, events);
    paintRail();
  }

  /** Reads what the mode shows, each Capability on its own: one that fails leaves the others on screen. */
  async function load(): Promise<void> {
    const mine = (generation += 1);
    const variables = { campaign: campaignOf(current) };
    await Promise.all(
      NEEDED[modeOf(current)].map(async (id) => {
        try {
          const answer = await shell.capability(id, undefined, variables);
          if (mine !== generation) return;
          readers[id]?.(answer.data);
          status.set(id, 'ok');
        } catch {
          if (mine !== generation) return;
          status.set(id, 'failed');
        }
        paint();
      }),
    );
  }

  /** The skeleton of the mode, its reads pending. */
  function frame(): void {
    status.clear();
    query = '';
    host.innerHTML = modeOf(current) === 'rail' ? railSkeleton() : tabsSkeleton(campaignOf(current));
    if (modeOf(current) === 'tabs') return;
    part('elements-rail').dataset.collapsed = String(readFolded(memory));
    syncRail();
  }

  // ---- the panel's own events, delegated: they outlive a change of mode

  host.addEventListener('input', (event) => {
    const input = event.target as HTMLElement | null;
    if (input?.id !== 'rail-search-input') return;
    query = (input as HTMLInputElement).value;
    paintRail();
  });

  host.addEventListener('click', (event) => {
    const target = event.target as Element | null;
    if (!target) return;
    const link = target.closest<HTMLElement>('a[data-entity]');
    if (link) {
      const mouse = event as MouseEvent;
      if (mouse.metaKey || mouse.ctrlKey || mouse.shiftKey || mouse.button !== 0) return;
      event.preventDefault();
      bus.open({ id: link.dataset.entity });
      setOpen(false);
      return;
    }
    if (target.id === 'rail-veil') return setOpen(false);
    const tab = target.closest<HTMLElement>('[role="tab"]');
    if (tab) return selectTab(TABS.findIndex(([key]) => key === tab.dataset.tab));
    const button = target.closest<HTMLElement>('button');
    if (!button) return;
    if (button.dataset.href) return bus.navigate(button.dataset.href);
    if (button.dataset.group) return toggleGroup(button);
    if (button.id === 'rail-toggle') return narrow() ? setOpen(false) : setFolded(part('elements-rail').dataset.collapsed !== 'true');
    if (button.id === 'rail-open') return setOpen(true);
    const kind = /^rail-new-(.+)$/.exec(button.id)?.[1];
    const fresh = kind === undefined ? undefined : NEW_KINDS[kind];
    if (fresh !== undefined) {
      bus.open({ new: fresh });
      setOpen(false);
    }
  });
  host.addEventListener('keydown', (event) => {
    const rail = host.querySelector<HTMLElement>('#elements-rail');
    if (event.key === 'Escape' && rail?.dataset.open === 'true') {
      setOpen(false);
      part('rail-open').focus();
    }
  });

  const close = (): void => {
    if (host.querySelector('#elements-rail')) setOpen(false);
  };
  mobile?.addEventListener('change', close);
  const poll: { stop?: () => void } = {};
  const stopOpened = bus.onOpened((opened) => {
    if (host.isConnected) follow(opened);
    else release();
  });
  /** A panel the shell took off the page lets go of its poll and of its listeners. */
  function release(): void {
    poll.stop?.();
    stopOpened();
    mobile?.removeEventListener('change', close);
  }

  // The data moved under the panel (an editor saved, the AI, D&D Beyond): the lists may have changed.
  let first = true;
  poll.stop = shell.onDataVersion?.(() => {
    if (!host.isConnected) return release();
    // The first word of the shell only says where the data stands, which the first read has.
    if (first) first = false;
    else void load();
  });

  /** Reads what the mode shows; the sheet then answers with the element it shows, so the rail highlights it. */
  const start = (): void => {
    void load().then(() => {
      if (modeOf(current) === 'rail') bus.query();
    });
  };

  frame();
  start();

  return {
    update(next) {
      const before = current;
      current = next;
      publish(next);
      const another = campaignOf(before) !== campaignOf(next);
      const otherMode = modeOf(before) !== modeOf(next);
      // What is on screen was the other campaign's, or the other mode's search: nothing of it stays.
      if (another) [players, characters, places, events, media, sounds] = [[], [], [], [], [], []];
      if (another || otherMode) frame();
      start();
    },
  };
}
