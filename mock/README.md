# ForgeDesMondes — maquette

Static mock-up of the proof of concept: plain HTML and JavaScript, no build, no network.

- `index.html` lists the screens; `pages.json` registers every page and micro-frontend.
- `pages/*.html` — one page per screen. Each links `../design/tokens.css`, `../kit/kit.css`, `../style.css` and loads `../mock.js`.
- `components/<Name>.html` — hand-written micro-frontends (no Micro-UI exists under `apps/` yet). Each finds its root with `document.currentScript.closest('[data-component]')`, reads its props from `data-props` and its data from `fixtures/`.
- `mock.js` — renders the design system's kit (`data-kit`) from `data-props`, loads the fragments, and gives them `Mock.fixture`, `Mock.command` (queued → confirmed → applied), `Mock.dialog`, a page bus (`Mock.on` / `Mock.emit`) and `Mock.store`.
- `Mock.store` keeps the battlemap state and what the player view shows in `localStorage`, so the DM view and the player view stay in sync across two tabs and survive a reload. « Réinitialiser la carte » goes back to the fixture.
- `style.css` holds page layout and shared helpers, only through the design system's variables. Never write under `design/` or `kit/`.
- Each fragment names its parts (contract D): `data-writes="<DataCapability>"` on every actuator, `data-reads="<Capability>"` on every part that shows a read, written in the file itself (see `CLAUDE.md`). Kit buttons take `'data-writes'` as an item key.
