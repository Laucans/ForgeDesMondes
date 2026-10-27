// The stage's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the mock-up's `.pv` rules (mock/components/
// PlayerStage.html) and the mock-up's `.art` placeholder, scoped to the
// stage, every value a token. The stage fills the screen it is projected on (the
// mock-up's page gives it the whole viewport), so a window sent over it sits
// whole on the stage and its corner can be reached.

export const STYLE = `
.pv { box-sizing: border-box; position: relative; width: 100%; height: 100%; min-height: max(calc(var(--space-5) * 12), calc(100vh - var(--space-4) * 2)); overflow: hidden; background: var(--color-surface); color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.pv *, .pv *::before, .pv *::after { box-sizing: inherit; }
.pv h1, .pv p { margin: 0; }
.pv .art { position: relative; display: grid; place-items: center; width: 100%; height: 100%; overflow: hidden; background: var(--color-surface-2); color: var(--color-text); font-family: var(--font-family-heading); text-transform: uppercase; }
.pv .art::before { content: ""; position: absolute; inset: 0; background: radial-gradient(circle at 30% 25%, color-mix(in srgb, var(--art-a) 75%, transparent), transparent 60%), radial-gradient(circle at 78% 82%, color-mix(in srgb, var(--art-b) 80%, transparent), transparent 55%), linear-gradient(160deg, var(--color-surface-2), var(--color-surface)); }
.pv .art::after { content: ""; position: absolute; left: -10%; right: -10%; bottom: -28%; height: 58%; border-radius: 50% 50% 0 0; background: color-mix(in srgb, var(--color-surface) 72%, transparent); }
.pv .art--ember { --art-a: var(--color-primary); --art-b: var(--color-secondary); }
.pv .art--dusk { --art-a: var(--color-secondary); --art-b: var(--color-accent); }
.pv .art--forest { --art-a: var(--color-success); --art-b: var(--color-secondary); }
.pv .art--gold { --art-a: var(--color-accent); --art-b: var(--color-primary); }
.pv .art--blood { --art-a: var(--color-danger); --art-b: var(--color-secondary); }
.pv .art--night { --art-a: var(--color-secondary); --art-b: var(--color-border); }
.pv .art__glyph { position: relative; z-index: 1; font-size: var(--font-size-heading); text-shadow: var(--shadow-low); }
.pv-base, .pv-map { position: absolute; inset: 0; }
.pv-cover { position: absolute; inset: 0; }
.pv-cover .art__glyph { font-size: calc(var(--font-size-heading) * 4); opacity: 0.35; }
.pv-cover__title { position: absolute; left: var(--space-5); bottom: var(--space-5); z-index: 1; display: grid; gap: var(--space-2); }
.pv-cover__title h1 { font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); font-size: calc(var(--font-size-heading) * 2); text-shadow: var(--shadow-high); }
.pv-cover__title p { font-size: var(--font-size-large); color: var(--color-text); text-shadow: var(--shadow-low); }
.pv-scene { position: absolute; inset: var(--space-4); display: grid; place-items: center; }
.pv-scene__art { height: 100%; max-width: 100%; border-radius: var(--radius-large); overflow: hidden; box-shadow: var(--shadow-high); }
.pv-scene__art .art__glyph { font-size: calc(var(--font-size-heading) * 3); }
.pv-window { position: absolute; z-index: 20; margin: 0; display: block; background: var(--color-surface-2); border: calc(var(--border-width) * 2) solid var(--color-accent); border-radius: var(--radius-medium); box-shadow: var(--shadow-high); overflow: hidden; touch-action: none; }
.pv-window__bar { position: absolute; top: 0; left: 0; right: 0; z-index: 1; padding: var(--space-1) var(--space-3); font-size: var(--font-size-small); font-family: var(--font-family-heading); text-transform: uppercase; color: var(--color-accent); background: color-mix(in srgb, var(--color-surface) 85%, transparent); cursor: move; user-select: none; }
.pv-window__art { position: absolute; inset: 0; }
.pv-window__resize { position: absolute; z-index: 1; right: 0; bottom: 0; width: var(--space-4); height: var(--space-4); background: linear-gradient(135deg, transparent 50%, var(--color-accent) 50%); cursor: nwse-resize; }
.pv-pilot { position: absolute; left: var(--space-3); bottom: var(--space-3); z-index: 30; padding: var(--space-1) var(--space-3); background: var(--color-primary); color: var(--color-primary-contrast); border-radius: var(--radius-pill); font-size: var(--font-size-small); font-weight: var(--font-weight-heading); }
.pv-status { position: absolute; left: var(--space-3); bottom: calc(var(--space-3) + var(--space-5)); z-index: 30; padding: var(--space-1) var(--space-3); background: var(--color-surface-2); border: var(--border-width) solid var(--color-danger); color: var(--color-danger); border-radius: var(--radius-medium); font-size: var(--font-size-small); }
.pv-status[hidden], .pv-window[hidden] { display: none; }
.pv-map .bm-carousel { position: absolute; top: var(--space-3); left: 50%; transform: translateX(-50%); z-index: 10; max-width: calc(100% - var(--space-5)); }
`;
