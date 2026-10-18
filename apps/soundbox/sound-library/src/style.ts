// The card's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the layout helpers the mock-up's pages share
// and the mock-up's `.mf-sounds` rules, scoped to the card, every value a token.

export const STYLE = `
.mf-sounds { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-sounds *, .mf-sounds *::before, .mf-sounds *::after { box-sizing: inherit; }
.mf-sounds.layout-2 { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 1fr); gap: var(--space-4); align-items: start; }
@media (max-width: 960px) { .mf-sounds.layout-2 { grid-template-columns: minmax(0, 1fr); } }
.mf-sounds h2 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; font-size: var(--font-size-large); color: var(--color-accent); }
.mf-sounds p { margin: 0; }
.mf-sounds .panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-sounds .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-sounds .muted { color: var(--color-text-muted); }
.mf-sounds .small { font-size: var(--font-size-small); }
.mf-sounds .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-sounds .stack { display: grid; gap: var(--space-3); align-content: start; }
.mf-sounds .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); cursor: pointer; }
.mf-sounds .chip:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-sounds .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-sounds .kit-slider.kit-slider--bare { padding: 0; background: transparent; border: 0; box-shadow: none; max-width: none; }
.mf-sounds .kit-input, .mf-sounds .kit-filedrop { max-width: none; }
.mf-sounds #sounds-upload-fields, .mf-sounds #sounds-edit-fields { display: grid; gap: var(--space-3); }
.mf-sounds__tags { display: flex; flex-wrap: wrap; gap: var(--space-1); }
.mf-sounds__tags button { padding: 0 var(--space-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: transparent; font: inherit; font-size: var(--font-size-small); color: var(--color-text-muted); cursor: pointer; }
.mf-sounds__tags button:hover, .mf-sounds__tags button:focus-visible { border-color: var(--color-primary); color: var(--color-accent); }
.mf-sounds__tags button[aria-pressed="true"] { border-color: var(--color-primary); color: var(--color-primary); }
.mf-sounds tr.is-playing td:first-child::before { content: "▶ "; color: var(--color-success); }
.mf-sounds .is-ok, .mf-sounds .is-error { padding: var(--space-2) var(--space-3); border-radius: var(--radius-medium); background: var(--color-surface); border: var(--border-width) solid var(--color-border); }
.mf-sounds .is-ok { border-color: var(--color-success); color: var(--color-success); }
.mf-sounds .is-error { border-color: var(--color-danger); color: var(--color-danger); }
`;
