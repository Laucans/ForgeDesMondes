// The bar's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the layout helpers the mock-up's pages share and
// the mock-up's `.mf-live-bar` rules, scoped to the bar, every value a token.

export const STYLE = `
.mf-live-bar { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-live-bar *, .mf-live-bar *::before, .mf-live-bar *::after { box-sizing: inherit; }
.mf-live-bar [hidden] { display: none !important; }
.mf-live-bar h1, .mf-live-bar h2, .mf-live-bar h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-live-bar h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-live-bar p { margin: 0; }
.mf-live-bar .muted { color: var(--color-text-muted); }
.mf-live-bar .small { font-size: var(--font-size-small); }
.mf-live-bar .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-live-bar .list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-2); }
.mf-live-bar .item { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); padding: var(--space-2) var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-live-bar .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-live-bar .kit-button--small button { padding: var(--space-1) var(--space-3); font-size: var(--font-size-small); }
.mf-live-bar .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); text-decoration: none; cursor: pointer; }
.mf-live-bar .chip:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-live-bar .chip.is-on { background: var(--color-primary); border-color: var(--color-primary); color: var(--color-primary-contrast); font-weight: var(--font-weight-heading); }
.mf-live-bar { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-3) var(--space-4); padding: var(--space-2) var(--space-4); background: var(--color-surface-2); border-bottom: var(--border-width) solid var(--color-border); }
.mf-live-bar__title { display: grid; }
.mf-live-bar__title h1 { font-size: var(--font-size-large); color: var(--color-primary); }
.mf-live-bar__scenes { display: flex; flex-wrap: wrap; gap: var(--space-1); }
.mf-live-bar__broadcast { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); margin-left: auto; padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); }
.mf-live-bar__broadcast b { color: var(--color-accent); }
.mf-live-bar__summary { display: grid; gap: var(--space-2); }
.mf-live-bar__status { min-height: var(--space-4); }
.mf-live-bar__error { color: var(--color-danger); }
.mf-live-bar .mf-modal { position: fixed; inset: 0; z-index: 100; }
.mf-live-bar .mf-modal .kit-dialog, .mf-live-bar .mf-modal .kit-dialog__veil { height: 100%; }
.mf-live-bar .mf-modal .kit-dialog__window { max-width: calc(var(--space-5) * 20); max-height: 88vh; }
.mf-live-bar .kit-dialog__btn:disabled { opacity: 0.55; cursor: not-allowed; }
`;
