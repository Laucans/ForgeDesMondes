// The Micro-UI’s own look, for the shell, which loads the design system’s two
// stylesheets and no page’s: the helpers the mock-up’s pages share (the
// illustration `.art`, the chip and the small round button) scoped to the panel,
// then the mock-up’s `.mf-elements` and `.mf-rail` rules, every value a token.

export const STYLE = `
.mf-elements, .mf-rail { box-sizing: border-box; color: var(--color-text); font-family: var(--font-family); font-size: var(--font-size-base); line-height: 1.5; }
.mf-elements *, .mf-elements *::before, .mf-elements *::after, .mf-rail *, .mf-rail *::before, .mf-rail *::after { box-sizing: inherit; }
.mf-elements [hidden], .mf-rail [hidden] { display: none !important; }
.mf-elements h2, .mf-elements h3, .mf-rail h2, .mf-rail h3 { margin: 0; font-family: var(--font-family-heading); font-weight: var(--font-weight-heading); text-transform: uppercase; letter-spacing: 0.03em; line-height: 1.15; }
.mf-elements h2 { font-size: var(--font-size-large); color: var(--color-accent); }
.mf-elements h3 { font-size: var(--font-size-base); color: var(--color-text); }
.mf-elements p, .mf-rail p { margin: 0; }
.mf-elements .panel__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); }
.mf-elements .muted, .mf-rail .muted { color: var(--color-text-muted); }
.mf-elements .small, .mf-rail .small { font-size: var(--font-size-small); }
.mf-elements .is-error { color: var(--color-danger); }
.mf-elements .item { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--space-2); padding: var(--space-2) var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); }
.mf-rail .chip { display: inline-flex; align-items: center; gap: var(--space-1); padding: var(--space-1) var(--space-3); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; font-size: var(--font-size-small); cursor: pointer; }
.mf-rail .chip:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-rail .icon-btn { display: inline-grid; place-items: center; min-width: calc(var(--space-4) + var(--space-2)); height: calc(var(--space-4) + var(--space-2)); padding: 0 var(--space-1); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-pill); background: var(--color-surface); color: var(--color-text); font: inherit; cursor: pointer; }
.mf-rail .icon-btn:hover { border-color: var(--color-primary); color: var(--color-accent); }
.mf-elements .kit-tabs, .mf-elements .kit-table { font-size: var(--font-size-base); }
.mf-elements .kit-table th.num, .mf-elements .kit-table td.num { text-align: right; }
.mf-elements .art, .mf-rail .art { position: relative; display: grid; place-items: center; width: 100%; height: 100%; overflow: hidden; background: var(--color-surface-2); color: var(--color-text); font-family: var(--font-family-heading); text-transform: uppercase; }
.mf-elements .art::before, .mf-rail .art::before { content: ""; position: absolute; inset: 0; background: radial-gradient(circle at 30% 25%, color-mix(in srgb, var(--art-a) 75%, transparent), transparent 60%), radial-gradient(circle at 78% 82%, color-mix(in srgb, var(--art-b) 80%, transparent), transparent 55%), linear-gradient(160deg, var(--color-surface-2), var(--color-surface)); }
.mf-elements .art::after, .mf-rail .art::after { content: ""; position: absolute; left: -10%; right: -10%; bottom: -28%; height: 58%; border-radius: 50% 50% 0 0; background: color-mix(in srgb, var(--color-surface) 72%, transparent); }
.mf-elements .art__glyph, .mf-rail .art__glyph { position: relative; z-index: 1; font-size: var(--font-size-heading); text-shadow: var(--shadow-low); }
.mf-elements .art--ember, .mf-rail .art--ember { --art-a: var(--color-primary); --art-b: var(--color-secondary); }
.mf-elements .art--dusk, .mf-rail .art--dusk { --art-a: var(--color-secondary); --art-b: var(--color-accent); }
.mf-elements .art--forest, .mf-rail .art--forest { --art-a: var(--color-success); --art-b: var(--color-secondary); }
.mf-elements .art--gold, .mf-rail .art--gold { --art-a: var(--color-accent); --art-b: var(--color-primary); }
.mf-elements .art--blood, .mf-rail .art--blood { --art-a: var(--color-danger); --art-b: var(--color-secondary); }
.mf-elements .art--night, .mf-rail .art--night { --art-a: var(--color-secondary); --art-b: var(--color-border); }
.mf-elements { display: grid; gap: var(--space-3); }
.mf-elements__grid { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(calc(var(--space-5) * 6), 1fr)); gap: var(--space-2); }
.mf-elements__entry { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-3); align-items: center; padding: var(--space-2) var(--space-3); background: var(--color-surface); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-medium); color: var(--color-text); text-decoration: none; }
.mf-elements__entry:hover { border-color: var(--color-primary); }
.mf-elements__thumb { width: calc(var(--space-5) + var(--space-2)); height: calc(var(--space-5) + var(--space-2)); border-radius: var(--radius-pill); overflow: hidden; }
.mf-elements__entry b { display: block; }
.mf-elements__entry small { display: block; color: var(--color-text-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.mf-elements__links { margin-top: var(--space-3); }

/* Rail mode: a sticky list beside the sheet, folded to a thin strip, a drawer under 960 px. */
.mf-rail__panel { display: flex; flex-direction: column; gap: var(--space-3); width: calc(var(--space-5) * 7); max-height: calc(100vh - var(--space-5)); padding: var(--space-3); background: var(--color-surface-2); border: var(--border-width) solid var(--color-border); border-radius: var(--radius-large); box-shadow: var(--shadow-low); }
.mf-rail__bar { display: flex; align-items: center; gap: var(--space-2); }
.mf-rail__bar h2 { font-size: var(--font-size-base); }
.mf-rail__strip { display: none; writing-mode: vertical-rl; color: var(--color-text-muted); font-family: var(--font-family-heading); text-transform: uppercase; letter-spacing: 0.08em; }
.mf-rail__body { display: flex; flex-direction: column; gap: var(--space-2); min-height: 0; flex: 1; }
.mf-rail__body .kit-input { max-width: none; }
.mf-rail__groups { overflow-y: auto; min-height: 0; flex: 1; display: grid; gap: var(--space-3); align-content: start; }
.mf-rail__group h3 { font-size: var(--font-size-small); color: var(--color-text-muted); margin-bottom: var(--space-1); }
.mf-rail__group h3 small { margin-left: var(--space-1); }
.mf-rail__group-toggle { display: flex; align-items: center; gap: var(--space-1); width: 100%; margin: 0; padding: 0; border: 0; background: none; color: inherit; font: inherit; text-align: left; cursor: pointer; }
.mf-rail__group-toggle:hover { color: var(--color-text); }
.mf-rail__caret { display: inline-block; width: var(--space-3); flex-shrink: 0; text-align: center; }
.mf-rail__list { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--space-1); }
.mf-rail__list[hidden] { display: none; }
.mf-rail__entry { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--space-2); align-items: center; padding: var(--space-1) var(--space-2); border: var(--border-width) solid transparent; border-radius: var(--radius-medium); color: var(--color-text); text-decoration: none; }
.mf-rail__entry:hover { background: var(--color-surface); border-color: var(--color-border); }
.mf-rail__entry.is-current { background: var(--color-surface); border-color: var(--color-primary); }
.mf-rail__thumb { width: var(--space-5); height: var(--space-5); border-radius: var(--radius-pill); overflow: hidden; }
.mf-rail__thumb .art__glyph { font-size: var(--font-size-base); }
.mf-rail__text b { display: block; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.mf-rail__text small { display: block; color: var(--color-text-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.mf-rail__new { flex-shrink: 0; padding-top: var(--space-2); border-top: var(--border-width) solid var(--color-border); }
.mf-rail__new .kit-button { flex-direction: column; align-items: stretch; }
.mf-rail__open, .mf-rail__veil { display: none; }
@media (min-width: 961px) {
  .mf-rail[data-collapsed="true"] .mf-rail__panel { width: calc(var(--space-5) + var(--space-2)); align-items: center; padding: var(--space-2); }
  .mf-rail[data-collapsed="true"] .mf-rail__bar h2, .mf-rail[data-collapsed="true"] .mf-rail__body { display: none; }
  .mf-rail[data-collapsed="true"] .mf-rail__strip { display: block; }
}
@media (max-width: 960px) {
  .mf-rail__open { display: inline-flex; }
  .mf-rail[data-open="true"] .mf-rail__veil { display: block; position: fixed; inset: 0; z-index: 149; background: color-mix(in srgb, var(--color-surface) 70%, transparent); }
  .mf-rail__panel { position: fixed; top: 0; bottom: 0; left: 0; z-index: 150; width: min(85vw, calc(var(--space-5) * 9)); max-height: none; border-radius: 0 var(--radius-large) var(--radius-large) 0; transform: translateX(-105%); visibility: hidden; }
  .mf-rail[data-open="true"] .mf-rail__panel { transform: none; visibility: visible; }
}
`;
