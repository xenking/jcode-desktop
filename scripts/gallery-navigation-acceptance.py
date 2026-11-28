#!/usr/bin/env python3
"""Drive the 96-image gallery in an isolated native Desktop window."""

import argparse
import json
from pathlib import Path
import subprocess
import sys


def main():
    args = argparse.ArgumentParser(description=__doc__)
    args.add_argument("--run", type=Path, required=True, help="new evidence directory")
    args = args.parse_args()
    helper = Path(__file__).with_name("verify-jcode-desktop.py")
    run = args.run.resolve()

    def call(command, *options):
        result = subprocess.run(
            [sys.executable, str(helper), command, "--run", str(run), *options],
            text=True, stdout=subprocess.PIPE, check=True,
        )
        return json.loads(result.stdout)

    def drive(action, **params):
        return call("drive", "--action", action, "--params", json.dumps(params))

    def click(selector):
        drive("click", selector=selector)

    def key(name):
        drive("press_key", key="fixture:" + name)

    frames = {}

    def check(stage):
        state = drive("state")
        panel = state["panel"]
        nav = state["navigation"]
        state = {**panel["gallery_navigation"], "draft": panel["composer"],
                 "composer_focus": panel["composer_focus"], "session": panel["session_id"],
                 "focused_slot": nav["focused_slot"], "row": nav["active_row"],
                 "cameras": [(row["camera"], row["camera_target"]) for row in nav["rows"]],
                 "panels": nav["visible_panels"]}
        frames[stage] = state
        (run / "gallery-navigation-states.json").write_text(json.dumps(frames, indent=2) + "\n")
        return state

    def screenshot(stage):
        call("screenshot", "--name", f"gallery-{stage}.png")

    call("launch", "--fixture", "gallery-navigation", "--window-width", "1440")
    try:
        call("doctor")
        drive("focus")
        draft = "Retain this gallery draft"
        drive("set_composer", text=draft)
        click("composer:images")
        opened = check("opened")
        assert opened["panels"] == 2 and opened["count"] == 96, opened
        assert opened["composer_focus"] and not opened["focused"], opened
        assert opened["selected"] == 95 and opened["x"] == 0, opened
        key("left")
        composer = check("composer-still-owns-arrows")
        assert composer["open"] and composer["selected"] == 95, composer
        assert composer["caret"] == len(draft) - 1 and composer["draft"] == draft, composer
        key("escape")
        composer = check("composer-still-owns-escape")
        assert composer["open"] and not composer["draft"], composer
        drive("set_composer", text=draft)
        screenshot("opened")

        click("gallery-nav:lines")
        wheel = check("wheel")
        assert -wheel["max_x"] < wheel["x"] < opened["x"], wheel
        for name, offset in [("last", -wheel["max_x"]), ("first", 0)]:
            for suffix in ("move", "edge"):
                click(f"gallery-nav:{name}")
                edge = check(f"{name}-{suffix}")
                assert edge["x"] == offset, edge
                for field in ("transcript", "session", "focused_slot", "row", "cameras", "selected"):
                    assert edge[field] == opened[field], (field, edge)

        click("gallery:thumb:0")
        focused = check("clicked-thumbnail")
        assert focused["focused"] and focused["selected"] == 0, focused
        for _ in range(9):
            key("right")
        moved = check("keyboard-revealed-thumbnail")
        assert moved["focused"] and moved["selected"] == 9 and moved["x"] < 0, moved
        screenshot("focus")
        key("left")
        assert check("left")["selected"] == 8

        click("gallery-nav:gap")
        assert not check("letterbox-does-not-open")["preview"]
        click("gallery-nav:picture")
        assert check("picture-opens-preview")["preview"]
        screenshot("preview")
        key("escape")
        after_preview = check("preview-escape")
        assert not after_preview["preview"] and after_preview["open"], after_preview
        click("gallery-nav:history")
        older = check("historical-row-opens-its-image")
        assert older["preview_label"] == f"[#img_{0:032x}]" and older["selected"] == 8, older
        key("escape")
        history_closed = check("historical-preview-escape")
        assert not history_closed["preview"] and history_closed["draft"] == draft, history_closed
        click("gallery:thumb:7")
        refocused = check("refocused-thumbnail")
        assert refocused["focused"] and refocused["selected"] == 7, refocused
        key("escape")
        closed = check("gallery-escape")
        assert not closed["open"] and closed["composer_focus"] and closed["draft"] == draft, closed
        screenshot("closed")
        print(json.dumps({"passed": True, "evidence": str(run)}))
    finally:
        call("cleanup")


if __name__ == "__main__":
    main()
