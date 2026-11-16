# Embedded terminal

Users open a real shell beside coding sessions, type commands, read styled
output and scroll history.

## Sub-features

- terminal.pty: shell and fixture execute on a real PTY, not an emulator setter.
- terminal.input: native input reaches the shell and changes visible output.
- terminal.render: ANSI, cursor addressing, alternate screen and color queries.
- terminal.images: supported Kitty placement/deletion and scrollback.
- terminal.focus: focus/resizing preserve the terminal session.

## How to get to it (user POV)

Cmd+T on macOS or Super+T on Linux opens a terminal in the full workspace. Type in
its shell. Use terminal selection/copy/paste and scroll history. Single-panel chat
mode does not expose this workspace path.

## Driving it with private-Xvfb acceptance

Preconditions: Linux, current host from
`justrust build -p jcode-desktop --target-dir target`, Xvfb, Openbox, xdotool,
ImageMagick, Tesseract, Mesa lavapipe and Pillow. No daemon/model auth required.

- **Shell/rendering/images:**
  `python3 scripts/terminal_acceptance.py target/v-terminal --binary target/debug/jcode-desktop`.
  The runner launches its fixture through the app's actual shell/PTY and sends
  native input only on private Xvfb. Require exit 0, both `stdin_tty`/`stdout_tty`
  true in `pty-proof.json`, expected keys recorded there, and inspect its PNGs.
  Read the retained report for individual outcomes.
- **Light theme:**
  `python3 scripts/terminal_acceptance.py target/v-terminal-light --binary target/debug/jcode-desktop --theme neutral-light`.
  Require real OSC replies and visible foreground/background to agree.

## Gotchas

- macOS helper has no terminal route. It refuses unsupported key injection.
  A chat screenshot cannot prove a shell command ran.
- `--no-kitty` omits image acceptance. Report reduced coverage, not a full pass.
- Initial macOS proof did not run Linux terminal acceptance. macOS input,
  clipboard, hot-reload PTY retention and image scrolling need separate proof.
  See `docs/terminal.md` for supported protocol limits.
