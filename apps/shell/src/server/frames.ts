// The frame of a screen whose design is more than a column of Micro-UIs: a
// header, a left column, tabs at the centre and an overlay. A frozen screen
// (contract E) lists its Micro-UIs flat; the frame says where each one sits on
// the page. A screen with no frame here is laid out as a column.

/** One tab of the centre: the Micro-UI it shows, and the event on the page's bus that brings it forward. */
export interface Tab {
  label: string;
  microUi: string;
  opens?: string;
  /** The screen's context value the event sets, read from the event's `detail`: the Micro-UIs that follow it update. */
  sets?: { context: string; from: string };
}

export interface Frame {
  /** Micro-UIs across the top of the page. */
  header: string[];
  /** Micro-UIs of the left column, and what the column is called. */
  left: { label: string; microUis: string[] };
  /** The centre: the tabs between Micro-UIs, and what they are called. */
  centre: { label: string; tabs: Tab[] };
  /** Micro-UIs fixed over the page, whatever the centre shows. */
  overlay: string[];
}

export const FRAMES: Readonly<Record<string, Frame>> = {
  'dm-view': {
    header: ['LiveSessionBar'],
    left: { label: 'Éléments de la séance et de la campagne', microUis: ['ElementsPanel'] },
    centre: {
      label: 'Zone principale',
      tabs: [
        // An element opened from the panel, the sheet or the suggestions shows its sheet; the map asked for shows the battlemap.
        { label: 'Fiche', microUi: 'TableSheet', opens: 'fdm:open' },
        // The map asked for by instance (`detail.id`) is the one the Battlemap's `$ctx.battlemap` follows.
        { label: 'Battlemap', microUi: 'Battlemap', opens: 'fdm:open-map', sets: { context: 'battlemap', from: 'id' } },
        { label: 'Soundbox', microUi: 'LiveSoundbox' },
      ],
    },
    overlay: ['SuggestionPill'],
  },
};
