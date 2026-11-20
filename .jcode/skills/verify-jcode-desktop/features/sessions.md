# Find and resume sessions

Users search history and return to the intended conversation while preserving
their current draft.

## Sub-features

- sessions.search: query titles, folders and session IDs.
- sessions.cancel: close the browser without replacing the conversation.
- sessions.resume: select a conversation with its existing history.
- sessions.standalone: resume within a standalone window.

## How to get to it (user POV)

Alt+R, /resume, /sessions or /session opens the browser. Sidebar history is another
workspace entry. Starting with --resume opens a standalone browser. Up/Down and
Page Up/Down select, Enter or a click resumes, Ctrl+1/2/3 select All/Active/Saved,
and Escape/Back cancels.

## Driving it with native-control

Preconditions: fresh healthy helper run with offline session catalog. It is not
the user's real history.

- **Search:** `python3 scripts/verify-jcode-desktop.py drive --run "$RUN" --journey search`.
  Require `search_open: true`, exact query `fixture`, and visibly open browser in
  the after PNG. This proves query entry, not a matching result or resume.
- **Cancel:** `python3 scripts/verify-jcode-desktop.py drive --run "$RUN" --action press_key --params '{"key":"Escape"}'`,
  then `python3 scripts/verify-jcode-desktop.py drive --run "$RUN" --action state`.
  Require `search_open: false`, unchanged session ID and composer.
- **Background attachment:**
  `python3 scripts/verify-jcode-desktop.py drive --run "$RUN" --action open_session --params '{"session_id":"swarm-fixture-0"}'`,
  then `python3 scripts/verify-jcode-desktop.py drive --run "$RUN" --action state --session swarm-fixture-0`.
  Require the exact session. This is not clicking a search result.
- **Sidebar todo counts:** launch with `--fixture sidebar-todos`. Capture the
  sidebar with two project headers and no duplicate dock above the account footer.
  Click `sidebar-todos:screenshot-fixture`, then
  `sidebar-todos:session_owl_1795071600000_sidebar` with `drive --action click`.
  Capture the orchestration panel after each click. The second capture must show
  **Verify a background project**, phases 1 and 2, and the Review and Verification
  lanes. `drive --action list` must show one orchestration panel and the original
  chat only. This uses a private persisted snapshot, not a live daemon.
- **Native browser paths, Linux:**
  `python3 scripts/verify-resume-panel.py target/resume-proof.png --binary target/debug/jcode-desktop`.
  Existing private-Xvfb acceptance covers workspace/single-panel browsing,
  search, selection, cancel/draft retention and resume.

## Gotchas

- No matching fixture is not proof of result selection. Record the unmet seed
  precondition instead of switching to an unrelated successful path.
- The macOS helper cannot dispatch browser Up/Down, filter controls or result
  clicks. Those entry points stay gaps until native acceptance is run.
- Background-only `open_session` cannot open arbitrary real sessions or another
  visible window. Catalog absence is an error, not permission to use user data.
