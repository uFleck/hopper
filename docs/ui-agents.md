# Rules for the UI view agents

Repo: `C:\Projects\hopper` (front end only: `src/`). Three agents work in parallel on DISJOINT files; never edit another agent's files.

Read first: `CLAUDE.md`, `docs/PLAN.md` sections 0, 6 ("What the UI shows"), 8 (design direction A "Dock", panel anatomy), the contract `src/ipc/types.ts` (read-only), the store API in `src/store/` (read-only; if you need a new store action or selector tell the orchestrator in your report instead of editing `src/store`... except where your brief says you may add one clearly-named file), the UI kit `src/ui/` (use it; extend it only by adding NEW files), tokens `src/styles/tokens.css`, `src/styles/ui.css`, `src/App.tsx`.

Hard rules:
- Panel, not an app: one vertical column that must be fully usable at 430 px (no horizontal scroll of the page at 430; the code editors and tables scroll inside themselves) and better at 640 and 860. Sheets slide over the panel, never a new window.
- NO native dialogs (`alert`, `confirm`, `prompt`, `@tauri-apps/plugin-dialog` message/ask). Use `ConfirmDialog`/`confirm()` from the UI kit. The folder picker goes through the store's `pickAndOpenWorkspace` only.
- Secrets never reach the UI as values: show status dots and `••••••••`; the only value path is the explicit `secret_reveal` click that auto-hides after 10 s. Password inputs for setting a secret clear themselves after saving.
- Production environment must be unmistakable (red tint) wherever a request can be sent or approved.
- Dark first (tokens); keep it light-theme-safe by using tokens, never hex literals. Honour `prefers-reduced-motion`. Keyboard: everything reachable, visible focus, Esc closes the innermost sheet/dialog, Enter submits forms. Buttons that need a label have `aria-label`.
- Visual direction: Spawner's companion look (4 px radius, hairline dividers, Inter + JetBrains Mono, Phosphor icons) with Hopper's own accent; dense but calm; no decorative ornament that costs width. Use the `frontend-design` skill's guidance for craft, but do not introduce new dependencies: NO changes to `package.json` or `pnpm-lock.yaml`.
- Style: match the existing code (function components, hooks, plain CSS in a file per area imported by your top-level component, no CSS-in-JS). No filler comments.
- Tests: Vitest for any pure logic you add (formatting, parsing, grouping); component behaviour is verified in the browser with the Playwright MCP against the dev server and the mock backend (`window.__hopperMock` helpers). Take screenshots at 430, 640 and 860 of each screen/state you build and save them under `C:\Projects\hopper\.playwright-mcp\<yourarea>-<state>-<width>.png`.
- Dev server: run `pnpm exec vite --port <your port> --strictPort` as a background process (the port is in your brief; ports differ so the agents don't collide), open `http://localhost:<port>/` with Playwright, and stop the server when done. Do not use port 1420.
- Checks that must pass before you report: `pnpm typecheck` and `pnpm test` (other agents are editing other files at the same moment; if a failure is clearly in someone else's file, wait a minute and re-run, and tell me if it persists).
- Do not commit or push. Report: components exported (name, props), store actions/selectors you used or need, anything in `types.ts` that is wrong or missing, screenshots, known gaps.
