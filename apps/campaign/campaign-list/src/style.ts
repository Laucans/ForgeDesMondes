// The list's own look, for the shell, which loads the design system's two
// stylesheets and no page's: the mock-up's `.mf-campaigns` rules, the
// illustration helper (`.art`), the layout helpers the fragment uses and the
// modal the dialogs open in, scoped to the list, every value a token.

export const STYLE = `
.mf-campaigns, .mf-modal { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); line-height: 1.5; }
.mf-campaigns *, .mf-campaigns *::before, .mf-campaigns *::after, .mf-modal *, .mf-modal *::before, .mf-modal *::after { box-sizing: inherit; }
.mf-campaigns [hidden], .mf-modal [hidden] { display: none !important; }
.mf-campaigns h2 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); font-size: var(--font-size-large); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; color: var(--color-accent); }
.mf-campaigns p, .mf-modal p { margin: 0; }
.mf-campaigns .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-campaigns .muted { color: var(--color-text-muted); }
.mf-campaigns .small, .mf-modal .small { font-size: var(--font-size-small); }
.mf-campaigns .is-error, .mf-modal .is-error { color: var(--color-danger); }
.mf-campaigns { display: grid; gap: var(--space-3); }
.mf-campaigns__grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(calc(var(--space-5) * 7), 1fr)); gap: var(--space-4); }
.mf-campaigns .kit-card { max-width: none; height: 100%; display: flex; flex-direction: column; }
.mf-campaigns .kit-card .body { flex: 1; }
.mf-campaigns .kit-card .media { height: calc(var(--space-5) * 4); }
.mf-campaigns .kit-card .media .art { width: 100%; height: 100%; }
.mf-campaigns__meta { display: flex; flex-wrap: wrap; gap: var(--space-2); align-items: center; font-size: var(--font-size-small); color: var(--color-text-muted); }
.mf-campaigns .kit-button.kit-button--bare { padding: 0; background: transparent; border: 0; gap: var(--space-2); }
.mf-campaigns .art { position: relative; display: grid; place-items: center; width: 100%; height: 100%; overflow: hidden; background: var(--color-surface-2); color: var(--color-text); font-family: var(--font-family-heading); text-transform: uppercase; }
.mf-campaigns .art::before { content: ""; position: absolute; inset: 0; background: radial-gradient(circle at 30% 25%, color-mix(in srgb, var(--art-a) 75%, transparent), transparent 60%), radial-gradient(circle at 78% 82%, color-mix(in srgb, var(--art-b) 80%, transparent), transparent 55%), linear-gradient(160deg, var(--color-surface-2), var(--color-surface)); }
.mf-campaigns .art::after { content: ""; position: absolute; left: -10%; right: -10%; bottom: -28%; height: 58%; border-radius: 50% 50% 0 0; background: color-mix(in srgb, var(--color-surface) 72%, transparent); }
.mf-campaigns .art--ember { --art-a: var(--color-primary); --art-b: var(--color-secondary); }
.mf-campaigns .art--dusk { --art-a: var(--color-secondary); --art-b: var(--color-accent); }
.mf-campaigns .art--forest { --art-a: var(--color-success); --art-b: var(--color-secondary); }
.mf-campaigns .art--gold { --art-a: var(--color-accent); --art-b: var(--color-primary); }
.mf-campaigns .art--blood { --art-a: var(--color-danger); --art-b: var(--color-secondary); }
.mf-campaigns .art--night { --art-a: var(--color-secondary); --art-b: var(--color-border); }
.mf-campaigns .art__glyph { position: relative; z-index: 1; font-size: var(--font-size-heading); text-shadow: var(--shadow-low); }
.mf-modal { position: fixed; inset: 0; z-index: 100; }
.mf-modal .kit-dialog, .mf-modal .kit-dialog__veil { height: 100%; }
.mf-modal .kit-dialog__window { max-height: 88vh; }
.mf-modal .kit-dialog__body > * + * { margin-top: var(--space-3); }
`;
