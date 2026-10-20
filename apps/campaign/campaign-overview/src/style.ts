// The panel's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the mock-up's `.mf-overview` rules, the
// illustration helper (`.art`) and the layout helpers the fragment uses,
// scoped to the panel, every value a token.

export const STYLE = `
.mf-overview { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); line-height: 1.5; }
.mf-overview *, .mf-overview *::before, .mf-overview *::after { box-sizing: inherit; }
.mf-overview { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 2fr); gap: var(--space-4); align-items: stretch; background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); overflow: hidden; box-shadow: var(--shadow-high); }
.mf-overview h1 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); font-size: var(--font-size-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; color: var(--color-primary); }
.mf-overview p { margin: 0; }
.mf-overview .muted { color: var(--color-text-muted); }
.mf-overview .small { font-size: var(--font-size-small); }
.mf-overview .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-overview__cover { min-height: calc(var(--space-5) * 5); }
.mf-overview__body { display: grid; gap: var(--space-2); align-content: center; padding: var(--space-4); }
.mf-overview__counts { list-style: none; margin: 0; padding: 0; display: flex; flex-wrap: wrap; gap: var(--space-3); font-size: var(--font-size-small); color: var(--color-text-muted); }
.mf-overview__counts b { color: var(--color-text); font-size: var(--font-size-large); margin-right: var(--space-1); }
.mf-overview .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-overview .art { position: relative; display: grid; place-items: center; width: 100%; height: 100%; overflow: hidden; background: var(--color-surface-2); color: var(--color-text); font-family: var(--font-family-heading); text-transform: uppercase; }
.mf-overview .art::before { content: ""; position: absolute; inset: 0; background: radial-gradient(circle at 30% 25%, color-mix(in srgb, var(--art-a) 75%, transparent), transparent 60%), radial-gradient(circle at 78% 82%, color-mix(in srgb, var(--art-b) 80%, transparent), transparent 55%), linear-gradient(160deg, var(--color-surface-2), var(--color-surface)); }
.mf-overview .art::after { content: ""; position: absolute; left: -10%; right: -10%; bottom: -28%; height: 58%; border-radius: 50% 50% 0 0; background: color-mix(in srgb, var(--color-surface) 72%, transparent); }
.mf-overview .art--ember { --art-a: var(--color-primary); --art-b: var(--color-secondary); }
.mf-overview .art--dusk { --art-a: var(--color-secondary); --art-b: var(--color-accent); }
.mf-overview .art--forest { --art-a: var(--color-success); --art-b: var(--color-secondary); }
.mf-overview .art--gold { --art-a: var(--color-accent); --art-b: var(--color-primary); }
.mf-overview .art--blood { --art-a: var(--color-danger); --art-b: var(--color-secondary); }
.mf-overview .art--night { --art-a: var(--color-secondary); --art-b: var(--color-border); }
.mf-overview .art__glyph { position: relative; z-index: 1; font-size: var(--font-size-heading); text-shadow: var(--shadow-low); }
.mf-overview .art__caption { position: absolute; z-index: 1; left: var(--space-3); right: var(--space-3); bottom: var(--space-2); font-family: var(--font-family); font-size: var(--font-size-small); text-transform: none; color: var(--color-text); text-shadow: var(--shadow-low); }
@media (max-width: 760px) { .mf-overview { grid-template-columns: minmax(0, 1fr); } }
`;
