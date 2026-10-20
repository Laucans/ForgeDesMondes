// The panel's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the helpers the mock-up's pages share and the
// mock-up's `.mf-versions` rules, scoped to the panel and to the modal it
// opens, every value a token.

export const STYLE = `
.mf-versions, .mf-modal { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); line-height: 1.5; }
.mf-versions *, .mf-versions *::before, .mf-versions *::after, .mf-modal *, .mf-modal *::before, .mf-modal *::after { box-sizing: inherit; }
.mf-versions [hidden], .mf-modal [hidden] { display: none !important; }
.mf-versions h2, .mf-modal h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-versions h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-modal h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-versions p, .mf-modal p { margin: 0; }
.mf-versions .panel__head, .mf-modal .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-versions .muted, .mf-modal .muted { color: var(--color-text-muted); }
.mf-versions .small, .mf-modal .small { font-size: var(--font-size-small); }
.mf-versions { display: inline-flex; }
.mf-versions.is-panel { display: grid; gap: var(--space-3); }
.mf-versions__list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-2); }
.mf-versions__item { display: grid; gap: var(--space-1); padding: var(--space-2) var(--space-3); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-versions__item.is-unseen { border-color: var(--color-accent); }
.mf-versions__item .panel__head b { font-family: var(--font-family-heading); color: var(--color-accent); }
.mf-versions__status { padding: var(--space-2) var(--space-3); border-radius: var(--radius-medium); background: var(--color-surface); border: var(--border-width) solid var(--color-border); }
.mf-versions__status.is-ok { border-color: var(--color-success); color: var(--color-success); }
.mf-versions__status.is-error { border-color: var(--color-danger); color: var(--color-danger); }
.mf-modal { position: fixed; inset: 0; z-index: 100; }
.mf-modal .kit-dialog, .mf-modal .kit-dialog__veil { height: 100%; }
.mf-modal .kit-dialog__window { max-height: 88vh; }
.mf-modal--wide .kit-dialog__window { max-width: calc(var(--space-5) * 20); }
.mf-modal .kit-dialog__body > * + * { margin-top: var(--space-3); }
`;
