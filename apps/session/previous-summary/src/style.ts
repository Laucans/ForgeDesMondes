// The card's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the layout helpers the mock-up's pages share and
// the mock-up's `.mf-previous` rules, scoped to the card, every value a token.

export const STYLE = `
.mf-previous { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-previous *, .mf-previous *::before, .mf-previous *::after { box-sizing: inherit; }
.mf-previous [hidden] { display: none !important; }
.mf-previous h2, .mf-previous h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-previous h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-previous h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-previous p { margin: 0; }
.mf-previous.panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-previous .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-previous .layout-2 { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: var(--space-4); align-items: start; }
@media (max-width: 960px) { .mf-previous .layout-2 { grid-template-columns: minmax(0, 1fr); } }
.mf-previous .muted { color: var(--color-text-muted); }
.mf-previous .small { font-size: var(--font-size-small); }
.mf-previous .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-previous .stack { display: grid; gap: var(--space-3); align-content: start; }
.mf-previous .list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-2); }
.mf-previous .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-previous .kit-button--small button { padding: var(--space-1) var(--space-3); font-size: var(--font-size-small); }
.mf-previous .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); text-decoration: none; cursor: pointer; }
.mf-previous .chip:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-previous .chip--pnj { border-color: var(--color-secondary); }
.mf-previous .chip--lieu { border-color: var(--color-accent); }
.mf-previous .chip--media { border-color: var(--color-primary); }
.mf-previous .icon-btn { display: inline-grid; place-items: center; min-width: calc(var(--space-4) + var(--space-2)); height: calc(var(--space-4) + var(--space-2)); padding: 0 var(--space-1); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; cursor: pointer; }
.mf-previous .icon-btn:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-previous__text { font-size: var(--font-size-large); line-height: 1.5; }
.mf-previous #previous-threads-list li { padding: var(--space-2) var(--space-3); background: var(--color-surface); border-left: calc(var(--border-width) * 3) solid var(--color-accent); border-radius: var(--radius-small); }
.mf-previous__journal { position: fixed; top: 0; right: 0; bottom: 0; z-index: 60; width: min(100%, calc(var(--space-5) * 11)); display: grid; grid-template-rows: auto auto minmax(0, 1fr); gap: var(--space-2); padding: var(--space-4); background: var(--color-surface); border-left: var(--border-width) solid var(--color-secondary); box-shadow: var(--shadow-high); }
.mf-previous__lines { list-style: none; margin: 0; padding: 0; overflow-y: auto; display: grid; gap: var(--space-2); align-content: start; }
.mf-previous__lines li { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-3); }
.mf-previous__lines time { color: var(--color-text-muted); font-variant-numeric: tabular-nums; font-size: var(--font-size-small); }
`;
