// The card's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the layout helpers the mock-up's pages share
// and the mock-up's `.mf-ai` rules, scoped to the card, every value a token.
// The kit's Select is a drop list opened by a click; the card needs a field
// a browser and a screen reader know as a list of options, so it dresses a
// native <select> as the kit's Select.

export const STYLE = `
.mf-ai { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); }
.mf-ai *, .mf-ai *::before, .mf-ai *::after { box-sizing: inherit; }
.mf-ai.layout-2 { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 1fr); gap: var(--space-4); align-items: start; }
@media (max-width: 960px) { .mf-ai.layout-2 { grid-template-columns: minmax(0, 1fr); } }
.mf-ai h2, .mf-ai h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-ai h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-ai h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-ai p { margin: 0; }
.mf-ai .panel { display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-ai .muted { color: var(--color-text-muted); }
.mf-ai .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
.mf-ai .stack { display: grid; gap: var(--space-3); align-content: start; }
.mf-ai .list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-2); }
.mf-ai .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-ai .kit-input.is-wide, .mf-ai .kit-select.is-wide { max-width: none; }
.mf-ai .kit-select small { display: block; margin-top: var(--space-2); font-size: var(--font-size-small); color: var(--color-text-muted); }
.mf-ai .kit-select select { width: 100%; font: inherit; font-size: var(--font-size-base); color: inherit; padding: var(--space-2) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface-2); box-shadow: var(--shadow-low); cursor: pointer; }
.mf-ai .kit-select select:hover { border-color: var(--color-primary); }
.mf-ai .kit-select select:focus-visible { outline: calc(var(--border-width) * 2) solid var(--color-accent); outline-offset: calc(var(--border-width) * 2); border-color: var(--color-primary); }
.mf-ai__fields { display: grid; grid-template-columns: repeat(auto-fit, minmax(calc(var(--space-5) * 6), 1fr)); gap: var(--space-3); }
.mf-ai__detected { min-height: var(--space-4); }
.mf-ai__result { min-height: var(--space-4); }
.mf-ai__result p { padding: var(--space-2) var(--space-3); border-radius: var(--radius-medium); background: var(--color-surface); border: var(--border-width) solid var(--color-border); }
.mf-ai__result .is-ok { border-color: var(--color-success); color: var(--color-success); }
.mf-ai__result .is-error { border-color: var(--color-danger); color: var(--color-danger); }
.mf-ai .list li::before { content: "• "; color: var(--color-secondary); }
.mf-ai #ai-local-list li::before { content: "✓ "; color: var(--color-success); }
`;
