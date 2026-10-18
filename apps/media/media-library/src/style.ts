// The card's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the layout helpers the mock-up's pages share
// and the mock-up's `.mf-media` rules, scoped to the card, every value a
// token. The kit's Select is a drop list opened by a click; the card needs a
// field a browser and a screen reader know as a list of options, so it dresses
// a native <select> as the kit's Select.

export const STYLE = `
.mf-media { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); display: grid; gap: var(--space-3); align-content: start; padding: var(--space-4); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); min-width: 0; }
.mf-media *, .mf-media *::before, .mf-media *::after { box-sizing: inherit; }
.mf-media h2, .mf-media h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-media h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-media h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-media .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-media .muted { color: var(--color-text-muted); }
.mf-media .small { font-size: var(--font-size-small); }
.mf-media .stack { display: grid; gap: var(--space-3); align-content: start; }
.mf-media .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); }
.mf-media .chip--lieu { border-color: var(--color-accent); }
.mf-media .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-media .kit-button--small button { padding: var(--space-1) var(--space-3); font-size: var(--font-size-small); }
.mf-media .kit-input.is-wide, .mf-media .kit-select.is-wide, .mf-media .kit-filedrop.is-wide { max-width: none; }
.mf-media .kit-select select { width: 100%; font: inherit; font-size: var(--font-size-base); color: inherit; padding: var(--space-2) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface-2); box-shadow: var(--shadow-low); cursor: pointer; }
.mf-media .kit-select select:hover { border-color: var(--color-primary); }
.mf-media .kit-select select:focus-visible { outline: calc(var(--border-width) * 2) solid var(--color-accent); outline-offset: calc(var(--border-width) * 2); border-color: var(--color-primary); }
.mf-media .art { position: relative; display: grid; place-items: center; width: 100%; height: 100%; overflow: hidden; background: var(--color-surface-2); color: var(--color-text); font-family: var(--font-family-heading); text-transform: uppercase; }
.mf-media .art::before { content: ""; position: absolute; inset: 0; background: radial-gradient(circle at 30% 25%, color-mix(in srgb, var(--art-a) 75%, transparent), transparent 60%), radial-gradient(circle at 78% 82%, color-mix(in srgb, var(--art-b) 80%, transparent), transparent 55%), linear-gradient(160deg, var(--color-surface-2), var(--color-surface)); }
.mf-media .art::after { content: ""; position: absolute; left: -10%; right: -10%; bottom: -28%; height: 58%; border-radius: 50% 50% 0 0; background: color-mix(in srgb, var(--color-surface) 72%, transparent); }
.mf-media .art--ember { --art-a: var(--color-primary); --art-b: var(--color-secondary); }
.mf-media .art--dusk { --art-a: var(--color-secondary); --art-b: var(--color-accent); }
.mf-media .art--forest { --art-a: var(--color-success); --art-b: var(--color-secondary); }
.mf-media .art--gold { --art-a: var(--color-accent); --art-b: var(--color-primary); }
.mf-media .art--blood { --art-a: var(--color-danger); --art-b: var(--color-secondary); }
.mf-media .art--night { --art-a: var(--color-secondary); --art-b: var(--color-border); }
.mf-media .art__glyph { position: relative; z-index: 1; font-size: var(--font-size-heading); text-shadow: var(--shadow-low); }
.mf-media__upload { display: grid; grid-template-columns: minmax(0, 3fr) minmax(0, 2fr); gap: var(--space-3); align-items: start; }
.mf-media__grid { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(calc(var(--space-5) * 4.5), 1fr)); gap: var(--space-3); }
.mf-media__item { display: grid; gap: var(--space-1); padding: var(--space-2); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-media__item.is-selected { border-color: var(--color-primary); box-shadow: var(--shadow-high); }
.mf-media__thumb { height: calc(var(--space-5) * 2.5); border-radius: var(--radius-small); overflow: hidden; }
.mf-media__item b { font-size: var(--font-size-small); }
.mf-media__item small { color: var(--color-text-muted); }
.mf-media__message { min-height: var(--space-4); }
.mf-media__message p { margin: 0; padding: var(--space-2) var(--space-3); border-radius: var(--radius-medium); background: var(--color-surface); border: var(--border-width) solid var(--color-border); }
.mf-media__message .is-ok { border-color: var(--color-success); color: var(--color-success); }
.mf-media__message .is-error { border-color: var(--color-danger); color: var(--color-danger); }
.mf-media .kit-filedrop.has-file .kit-filedrop__file { display: flex; }
@media (max-width: 900px) { .mf-media__upload { grid-template-columns: minmax(0, 1fr); } }
`;
