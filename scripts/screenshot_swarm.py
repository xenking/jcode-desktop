"""Native swarm acceptance, called only by screenshot.py's private Xvfb harness.

Run: python3 scripts/screenshot.py target/swarm-native.png --swarm-interact --no-build

The three-agent fixture renders as compact rows nested under its coordinator in
the sidebar and the composer bar carries the live worker count instead of the
old "Waiting for N workers" text strip. OCR checks the nested rows and the
absence of the removed strip and expansion controls; native clicks check that a
worker row opens that worker's chat and that the lead row stays one panel.
No GPUI selectors, control API, live display, network, or fixture mutation.
Per-step PNG/OCR/navigation and a JSON report survive temporary-root cleanup.
"""
import json
import shutil

from default_directory_acceptance import NativeUI, assert_focused, panels
from model_picker_acceptance import phrase_bounds


LEAD = "screenshot-fixture"
WORKERS = ("API reviewer", "Sidebar implementation", "Test runner")
WORKER = "swarm-fixture-0"
# Removed by the compact redesign: the text strip and its expansion controls.
GONE = ("Show all", "Show fewer", "Open lead", "Waiting for", "reported",
        "Swarm", "need attention")


def navigation(root):
    text = (root / "state").read_text()
    lines = [line[len("navigation="):] for line in text.splitlines()
             if line.startswith("navigation=")]
    assert len(lines) == 1, "Missing independent navigation fixture evidence"
    return json.loads(lines[0])


def live_panels(state):
    return [panel for panel in panels(state) if not panel.get("closing")]


def assert_sessions(state, expected, focused):
    live = live_panels(state)
    actual = [panel["session"] for panel in live]
    assert sorted(actual) == sorted(expected), (actual, expected)
    assert not any(state.get(key) for key in ("camera_motion", "tab_motion", "row_motion")), "Navigation still moving"
    if focused:
        matches = [panel for panel in live if panel["session"] == focused]
        assert len(matches) == 1, (focused, state)
        assert_focused(state, matches[0]["id"])
        return matches[0]["id"]
    return None


def has_phrase(words, phrase):
    try:
        phrase_bounds(words, phrase)
        return True
    except AssertionError:
        return False


def verify(output, env, root):
    ui = NativeUI(output, env, root)
    assert env.get("JCODE_DESKTOP_SCREENSHOT_SWARM") == "1", "Swarm fixture required"
    report = {"passed": False, "checks": [], "scope": "native X11 + rendered OCR + independent navigation"}
    stage = "initial"

    def frame(label, check):
        # Move away so tooltips cannot satisfy absence or title checks.
        ui.native("mousemove", 1100, 700)

        def wrapped(image):
            assert image.size == (1440, 1000), "Default screenshot dimensions required"
            words = ui.words(image, (0, 0, 224, 1000), label, psm=6)
            return check(image, words)

        return ui.wait_frame(label, wrapped)

    def check_nested(image, words):
        title = phrase_bounds(words, "Review markdown")
        for phrase in GONE:
            assert not has_phrase(words, phrase), f"{phrase!r} unexpectedly visible"
        for worker in WORKERS:
            row = phrase_bounds(words, worker)
            assert row[0] > title[0], f"{worker!r} does not start right of the coordinator"
            assert row[1] > title[1], f"{worker!r} does not sit below the coordinator row"
        return image, title

    def locate_worker(image, words):
        return image, phrase_bounds(words, WORKERS[0])

    def passed(check):
        report["checks"].append(check)
        print("Swarm native check: " + check, flush=True)

    try:
        image, title = frame(stage, check_nested)
        assert_sessions(navigation(root), [LEAD], LEAD)
        passed("workers render as compact rows nested under the coordinator, with no text strip or expansion controls")

        # A worker row opens that worker's chat.
        stage = "worker-click"
        _, worker_row = frame(stage, locate_worker)
        ui.click(worker_row)
        state = navigation(root)
        live = live_panels(state)
        assert sorted(panel["session"] for panel in live) == sorted([LEAD, WORKER]), live
        focused = [panel for panel in live if panel["focused"]]
        assert len(focused) == 1 and focused[0]["session"] == WORKER, state
        passed("clicking a nested worker row opens that worker's chat")

        # The lead row still focuses its own panel, and its title stays one line.
        stage = "lead-click"
        ui.click(title)
        state = navigation(root)
        live = live_panels(state)
        assert sorted(panel["session"] for panel in live) == sorted([LEAD, WORKER]), live
        assert_sessions(state, [LEAD, WORKER], LEAD)
        image, after_title = frame(stage, check_nested)
        assert after_title[0] == title[0] and after_title[2] == title[2], "Lead title width changed"
        assert after_title[3] - after_title[1] == title[3] - title[1], "Lead title height changed"
        shutil.copyfile(ui.artifact(stage + ".png"), output)
        report["passed"] = True
        passed("native lead click keeps the compact single-line sidebar title")
    except Exception as error:
        report.update(failed_stage=stage, error=str(error))
        raise
    finally:
        report["last_stage"] = stage
        for source, suffix in ((root / "state", "swarm-final-state.txt"),
                               (root / "app.log", "swarm-app.log"),
                               (root / "logs/jcode-desktop/jcode-desktop.log", "swarm-desktop.log")):
            if source.exists():
                shutil.copyfile(source, ui.artifact(suffix))
        output.with_suffix(".swarm.json").write_text(json.dumps(report, indent=2) + "\n")
