#!/usr/bin/env python3
"""Send repeatable physical input to the development VM through its QMP socket.

Run on the PVE host: python3 vm-input.py --vm-id 700 --width 1962 --height 627
JSON events are read from stdin: move(x,y), button(down), key(key,down), wait(seconds).
Coordinates are guest desktop logical coordinates. QEMU/tablet mapping must be
verified against the guest; this helper does not assume mapping is accurate.
"""
import argparse
import json
import socket
import sys
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--vm-id", type=int, default=700)
    parser.add_argument("--width", type=int, required=True)
    parser.add_argument("--height", type=int, required=True)
    args = parser.parse_args()
    if not 100 <= args.vm_id <= 999999999 or args.width < 2 or args.height < 2:
        parser.error("invalid VM ID or desktop dimensions")
    events = json.load(sys.stdin)
    if not isinstance(events, list) or len(events) > 1000:
        parser.error("expected an array of at most 1000 events")
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(5)
        connection.connect(f"/run/qemu-server/{args.vm_id}.qmp")
        stream = connection.makefile("rwb", buffering=0)
        greeting = json.loads(stream.readline())
        if "QMP" not in greeting:
            raise RuntimeError("not a QMP server")
        sequence = 0

        def command(name, arguments=None):
            nonlocal sequence
            sequence += 1
            request = {"execute": name, "id": sequence}
            if arguments is not None:
                request["arguments"] = arguments
            stream.write((json.dumps(request) + "\n").encode())
            while True:
                response = json.loads(stream.readline())
                if response.get("id") == sequence:
                    if "error" in response:
                        raise RuntimeError(response["error"])
                    return response.get("return")

        command("qmp_capabilities")
        pressed = {}
        try:
            for event in events:
                kind = event["type"]
                if kind == "capture":
                    # QMP reads the display without opening a guest app or changing focus.
                    path = event["path"]
                    if not path.startswith('/tmp/waytoys-') or not path.endswith('.ppm'):
                        raise ValueError('capture path must be /tmp/waytoys-*.ppm')
                    command('screendump', {'filename': path, 'device': event.get('device', 'vga'), 'head': event.get('head', 0)})
                    continue
                if kind == "wait":
                    seconds = float(event["seconds"])
                    if not 0 <= seconds <= 10:
                        raise ValueError("wait must be between 0 and 10 seconds")
                    time.sleep(seconds)
                    continue
                if kind == "move":
                    x, y = float(event["x"]), float(event["y"])
                    if not 0 <= x < args.width or not 0 <= y < args.height:
                        raise ValueError("pointer outside desktop")
                    inputs = [
                        {"type": "abs", "data": {"axis": "x", "value": round(x / (args.width - 1) * 32767)}},
                        {"type": "abs", "data": {"axis": "y", "value": round(y / (args.height - 1) * 32767)}},
                    ]
                elif kind == "button":
                    inputs = [{"type": "btn", "data": {"button": event.get("button", "left"), "down": bool(event["down"])}}]
                elif kind == "key":
                    inputs = [{"type": "key", "data": {"key": {"type": "qcode", "data": event["key"]}, "down": bool(event["down"])}}]
                else:
                    raise ValueError(f"unknown event: {kind}")
                command("input-send-event", {"events": inputs})
                if kind in ("button", "key"):
                    token = (kind, event.get("button", "left") if kind == "button" else event["key"])
                    if event["down"]:
                        pressed[token] = inputs[0]
                    else:
                        pressed.pop(token, None)
                time.sleep(0.04)
        finally:
            # Avoid leaving a held button/modifier in the guest after a test error.
            for event in reversed(list(pressed.values())):
                event["data"]["down"] = False
                try:
                    command("input-send-event", {"events": [event]})
                except (OSError, ValueError, RuntimeError):
                    pass
    print(json.dumps({"vm_id": args.vm_id, "events_sent": len(events)}))


if __name__ == "__main__":
    main()
