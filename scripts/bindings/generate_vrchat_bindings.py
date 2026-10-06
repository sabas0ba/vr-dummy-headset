"""Generates the VRChat default binding for vrdummy_controller.

The binding is derived from VRChat's own Valve Index binding, which ships with
the game. Inputs that vrdummy_controller does not have are remapped:

  /input/thumbstick            -> /input/joystick (same mode and inputs)
  /input/trackpad touch        -> /input/joystick touch (thumb gesture activator)
  /input/grip mode "grab"      -> /input/grip mode "button" with "click"
  skeleton sections            -> dropped (no skeleton component)

Everything else (buttons, triggers, grip pull, poses, haptics, chords) is kept
with the same action outputs. Action names are checked against VRChat's action
manifest; unknown actions abort the generation.

Usage:
  python -I scripts/bindings/generate_vrchat_bindings.py [vrchat_steamvr_dir] [output]

Defaults: the Steam install of VRChat and
driver/resources/input/vrdummy_controller_vrchat_bindings.json.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

DEFAULT_VRCHAT_DIR = Path(r"C:\Program Files (x86)\Steam\steamapps\common\VRChat\VRChat_Data\StreamingAssets\SteamVR")
REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_OUTPUT = REPO_ROOT / "driver" / "resources" / "input" / "vrdummy_controller_vrchat_bindings.json"

VRCHAT_APP_KEY = "steam.app.438100"
CONTROLLER_TYPE = "vrdummy_controller"

# Input components of vrdummy_controller (docs/protocol.md 3.4.1) by path and mode.
SUPPORTED_MODES = {
    "/input/system": {"button"},
    "/input/a": {"button"},
    "/input/b": {"button"},
    "/input/trigger": {"button", "trigger"},
    "/input/grip": {"button", "trigger"},
    "/input/joystick": {"joystick", "button"},
}


def component(path: str) -> str:
    """'/user/hand/left/input/trigger' -> '/input/trigger'."""
    idx = path.find("/input/")
    return path[idx:] if idx >= 0 else path


def convert_source(src: dict) -> dict | None:
    src = json.loads(json.dumps(src))  # deep copy
    path = src["path"]
    mode = src["mode"]
    if "/input/thumbstick" in path:
        src["path"] = path.replace("/input/thumbstick", "/input/joystick")
    elif "/input/trackpad" in path:
        if mode == "trackpad" and set(src.get("inputs", {})) == {"touch"}:
            src["path"] = path.replace("/input/trackpad", "/input/joystick")
            src["mode"] = "joystick"
        else:
            return None
    elif "/input/grip" in path and mode == "grab":
        src["mode"] = "button"
        src["inputs"] = {"click": src["inputs"]["grab"]}
        src.pop("parameters", None)
    elif "/input/skeleton" in path:
        return None
    comp = component(src["path"])
    if comp not in SUPPORTED_MODES or src["mode"] not in SUPPORTED_MODES[comp]:
        raise ValueError(f"unsupported source after conversion: {src['path']} mode {src['mode']}")
    return src


def convert(index_binding: dict, actions: dict) -> dict:
    known = {a["name"].lower() for a in actions["actions"]}
    known_sets = {s["name"].lower() for s in actions["action_sets"]}
    out = {
        "action_manifest_version": 0,
        "alias_info": {},
        "app_key": VRCHAT_APP_KEY,
        "bindings": {},
        "category": "steamvr_input",
        "controller_type": CONTROLLER_TYPE,
        "description": "Generated from VRChat's Valve Index binding by scripts/bindings/generate_vrchat_bindings.py",
        "interaction_profile": "",
        "name": "VRChat default bindings for vrdummy controller",
        "options": {},
        "simulated_actions": [],
    }
    for set_name, body in index_binding["bindings"].items():
        if set_name.lower() not in known_sets:
            raise ValueError(f"unknown action set {set_name}")
        new_body: dict = {}
        sources = []
        for src in body.get("sources", []):
            converted = convert_source(src)
            if converted is None:
                continue
            for inp in converted["inputs"].values():
                if inp["output"].lower() not in known:
                    raise ValueError(f"unknown action {inp['output']}")
            sources.append(converted)
        new_body["sources"] = sources
        for section in ("poses", "haptics", "chords"):
            if body.get(section):
                items = json.loads(json.dumps(body[section]))
                for item in items:
                    if "path" in item:
                        item["path"] = item["path"].replace("/input/thumbstick", "/input/joystick")
                    if "inputs" in item:  # chords
                        item["inputs"] = [[p.replace("/input/thumbstick", "/input/joystick"), k] for p, k in item["inputs"]]
                    if item["output"].lower() not in known:
                        raise ValueError(f"unknown action {item['output']}")
                new_body[section] = items
        out["bindings"][set_name] = new_body
    return out


def main() -> int:
    vrchat_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_VRCHAT_DIR
    output = Path(sys.argv[2]) if len(sys.argv) > 2 else DEFAULT_OUTPUT
    actions_path = vrchat_dir / "actions.json"
    index_path = vrchat_dir / "bindings_knuckles.json"
    if not actions_path.exists() or not index_path.exists():
        print(f"VRChat SteamVR files not found under {vrchat_dir}", file=sys.stderr)
        return 1
    actions = json.loads(actions_path.read_text(encoding="utf-8"))
    index_binding = json.loads(index_path.read_text(encoding="utf-8"))
    result = convert(index_binding, actions)
    output.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    total = sum(len(b["sources"]) for b in result["bindings"].values())
    print(f"wrote {output} ({len(result['bindings'])} action sets, {total} sources)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
