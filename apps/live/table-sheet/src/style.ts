// The sheet's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the mock-up's `.mf-sheet` rules (mock/components/
// TableSheet.html), the page-level classes the sheet uses (chip, row, muted,
// the placeholder art, the small bare kit Button) scoped to it, every value a
// token.

export const STYLE = `
.mf-sheet { box-sizing: border-box; display: grid; gap: var(--space-4); color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-sheet *, .mf-sheet *::before, .mf-sheet *::after { box-sizing: inherit; }
.mf-sheet h2, .mf-sheet h3, .mf-sheet p { margin: 0; }
.mf-sheet .muted { color: var(--color-text-muted); }
.mf-sheet .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-sheet .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); cursor: pointer; }
.mf-sheet .chip:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-sheet .chip--pj { border-color: var(--color-success); }
.mf-sheet .chip--pnj { border-color: var(--color-secondary); }
.mf-sheet .art { position: relative; display: grid; place-items: center; width: 100%; height: 100%; overflow: hidden; background: var(--color-surface-2); color: var(--color-text); font-family: var(--font-family-heading); text-transform: uppercase; }
.mf-sheet .art::before { content: ""; position: absolute; inset: 0; background: radial-gradient(circle at 30% 25%, color-mix(in srgb, var(--art-a) 75%, transparent), transparent 60%), radial-gradient(circle at 78% 82%, color-mix(in srgb, var(--art-b) 80%, transparent), transparent 55%), linear-gradient(160deg, var(--color-surface-2), var(--color-surface)); }
.mf-sheet .art::after { content: ""; position: absolute; left: -10%; right: -10%; bottom: -28%; height: 58%; border-radius: 50% 50% 0 0; background: color-mix(in srgb, var(--color-surface) 72%, transparent); }
.mf-sheet .art--ember { --art-a: var(--color-primary); --art-b: var(--color-secondary); }
.mf-sheet .art--dusk { --art-a: var(--color-secondary); --art-b: var(--color-accent); }
.mf-sheet .art--forest { --art-a: var(--color-success); --art-b: var(--color-secondary); }
.mf-sheet .art--gold { --art-a: var(--color-accent); --art-b: var(--color-primary); }
.mf-sheet .art--blood { --art-a: var(--color-danger); --art-b: var(--color-secondary); }
.mf-sheet .art--night { --art-a: var(--color-secondary); --art-b: var(--color-border); }
.mf-sheet .art__glyph { position: relative; z-index: 1; font-size: var(--font-size-heading); text-shadow: var(--shadow-low); }
.mf-sheet .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-sheet .kit-button--small button, .mf-sheet .kit-button--small a { padding: var(--space-1) var(--space-3); font-size: var(--font-size-small); }
.mf-sheet .kit-button a { font: inherit; font-weight: 600; letter-spacing: .04em; padding: var(--space-2) var(--space-4); border-radius: var(--radius-pill); border: var(--border-width) solid transparent; text-decoration: none; cursor: pointer; }
.mf-sheet .kit-button a.ghost { background: transparent; color: var(--color-text); }
.mf-sheet .kit-button a.ghost:hover { background: var(--color-surface-2); color: var(--color-accent); }
.mf-sheet .kit-button button:disabled { cursor: progress; }
.mf-sheet .kit-statblock { max-width: none; }
.mf-sheet__head { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-4); align-items: center; }
.mf-sheet__art { width: calc(var(--space-5) * 3); height: calc(var(--space-5) * 3); border-radius: var(--radius-large); overflow: hidden; box-shadow: var(--shadow-high); }
.mf-sheet__title { display: grid; gap: var(--space-2); }
.mf-sheet__title h2 { font-size: var(--font-size-heading); color: var(--color-primary); }
.mf-sheet__body { display: grid; grid-template-columns: minmax(0, 3fr) minmax(0, 2fr); gap: var(--space-4); align-items: start; }
.mf-sheet__text { display: grid; gap: var(--space-3); font-size: var(--font-size-large); line-height: 1.55; }
.mf-sheet__block { display: grid; gap: var(--space-1); }
.mf-sheet__block h3 { color: var(--color-accent); font-size: var(--font-size-small); }
.mf-sheet__play { padding: var(--space-3); background: var(--color-surface); border-left: calc(var(--border-width) * 4) solid var(--color-primary); border-radius: var(--radius-medium); }
.mf-sheet__notes { padding: var(--space-3); background: var(--color-surface); border: var(--border-width) dashed var(--color-accent); border-radius: var(--radius-medium); font-size: var(--font-size-base); }
.mf-sheet details { font-size: var(--font-size-base); color: var(--color-text-muted); }
.mf-sheet summary { cursor: pointer; color: var(--color-accent); }
.mf-sheet__status { padding: var(--space-1) var(--space-3); background: var(--color-surface-2); border: var(--border-width) solid var(--color-accent); border-radius: var(--radius-medium); color: var(--color-text); font-size: var(--font-size-small); }
.mf-sheet__status[hidden] { display: none; }
@media (max-width: 1100px) { .mf-sheet__body { grid-template-columns: minmax(0, 1fr); } }
`;
