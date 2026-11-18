# macOS input source diagnostics

The standalone recorder observes selected keyboard source notifications system-wide. It logs the source ID and frontmost bundle ID/PID, not typed text. Install or update it with `scripts/install-input-source-log.sh`. It starts automatically at login as `dev.xenking.input-source-log`. Its executable and log are private to the user.

For the Desktop-specific part, launch Desktop with `JCODE_DESKTOP_IME_LOG=1`. On macOS, `launchctl setenv JCODE_DESKTOP_IME_LOG 1` enables it for future applications started by launchd. A running Desktop host must be restarted after the new host is installed, and must inherit the variable. Hot-reloading the UI alone cannot add the host hooks. Disable it for future launches with `launchctl unsetenv JCODE_DESKTOP_IME_LOG` and restart Desktop.

Both components append UTC millisecond timestamps to `~/.jcode/logs/input-source.log`. Desktop records AppKit input client calls, input-context activate/deactivate/discard, key-window changes, key codes and modifier flags (`key-down`, `flags-changed`, never characters), the input source the app saw at that moment, and composer focus and focused handler registrations. A registration is a GPUI paint request, not proof that a native context was reactivated. The source observer includes the frontmost app name even outside Desktop, so treat the log as sensitive. It does not rotate automatically.

Immediately after reproducing, add a marker:

```sh
~/.jcode/bin/input-source-log --mark
```

## Code investigation

In GPUI revision `be875c5`, every painted focused composer calls `Window::handle_input`; the frame machinery swaps its `PlatformInputHandler` as part of rendering. On macOS this assignment does not itself call `NSTextInputContext.activate`, `deactivate`, or `discardMarkedText`. The macOS `text_input_state_changed` uses the default no-op. IME positioning schedules `invalidateCharacterCoordinates`, not activation. Ghost completion and the one-second recovery checkpoint update or snapshot the existing `PromptInput`; they do not recreate it. The composer editor ID is `prompt-editor`.

One concrete focus transition remains worth correlating: `Panel::resume_composer_typing` moves focus from a neutral workspace or transcript surface to the composer on a printable key, then inserts that key asynchronously. Picker and panel creation also legitimately replace or redirect focus. None of these establishes the reported layout reversion: do not suppress focus or pin a keyboard source without a trace showing which event causes it. The test exercises focus and text delivery across ghost rendering and draft snapshots, but does not reproduce a native TIS switch or prove a fix.

## Shift reverts the layout

Reproduce with the desktop recorder enabled, then read the lines around the marker:

- `flags-changed keycode=56 source=... modifiers=0x20000` is Shift. If the
  `input-source-changed` line follows it, the system changed the source while
  Shift was physically down.
- `key-down keycode=... source=...` immediately before a change shows whether
  the reset followed a plain character instead.
- `composer-focus`/`composer-blur` and `context-activate`/`context-deactivate`
  rule the app's own focus handling in or out.

Findings from the installed build (`677a228`, macOS 27.0.1): neither the host nor
GPUI contains a `TISSelectInputSource` call, so the app cannot select a source
itself. Driving the real host with every standard switch gesture
(`Ctrl+Space`, `Ctrl+Option+Space`) and then Shift, with the composer focused
and with `resume_composer_typing` taking the first character from an unfocused
workspace, kept the chosen source in each run. macOS's own input-source
switcher popup (`TUINSCursorUIController showTextInputMenuHUD`) is implicated
instead: Apple's community reports the layout reverting after the initial
character exactly while that popup is displayed. Until a trace shows a jcode
event ordering that precedes the change, do not pin or restore a source from
app code.

System-side workarounds worth trying when the trace shows only the system
change: switch with a short `Ctrl+Space` tap rather than holding the chord, turn
off "Automatically switch to a document's input source" in Keyboard settings,
and remove non-keyboard entries such as Character Palette from Input Sources.

Stop the standalone agent with `launchctl bootout gui/$(id -u)/dev.xenking.input-source-log`. To stop it permanently, also remove `~/Library/LaunchAgents/dev.xenking.input-source-log.plist` and `~/.jcode/bin/input-source-log` after unloading it. The existing log remains until the user removes it.
