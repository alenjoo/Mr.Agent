# OwnPager Frontend

This folder contains a frontend-only landing page prototype for `OwnPager`.

## What it is

- A marketing-style landing page that explains what OwnPager does.
- Built with TypeScript source files in `src/`.
- Shipped with prebuilt browser-ready files in `dist/` so you can preview it without wiring a backend.

## Product framing used in this prototype

The page presents OwnPager as:

- the intake layer for user requests
- channel-aware across CLI and Telegram
- responsible for normalization, session continuity, and workspace hints
- intentionally stopping at the `prepare_turn` boundary before deeper reasoning begins

## Files

- `index.html`: page shell
- `src/main.ts`: page composition and section rendering
- `src/data.ts`: landing page content model
- `src/styles.css`: visual design and responsive layout
- `dist/*.js`: prebuilt modules used by `index.html`

## Preview locally

From this folder:

```bash
python3 -m http.server 4173
```

Then open `http://localhost:4173`.

## Rebuild TypeScript later

If TypeScript is available in your environment, run:

```bash
tsc --project tsconfig.json
```
