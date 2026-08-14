#!/usr/bin/env python3
"""Minimal GTP engine used by the host integration tests.

Speaks just enough GTP (name/version/protocol_version/boardsize/clear_board/
play/genmove/list_commands/known_command) to exercise the real subprocess
transport (`ProcessGtpTransport` -> `GtpProcessSupervisor`) end to end without
bundling a full engine binary. It is a real child process communicating over
real pipes, so the host tests get genuine process supervision coverage.

Run it manually:  printf '1 name\n\n' | python3 fake-gtp-engine.py
"""

import sys

BOARD_SIZE = 19
OCCUPIED = set()
KNOWN_COMMANDS = {
    "name",
    "version",
    "protocol_version",
    "boardsize",
    "clear_board",
    "play",
    "genmove",
    "list_commands",
    "known_command",
}


def respond(identifier, success, content):
    prefix = "=" if success else "?"
    if identifier is None:
        sys.stdout.write(f"{prefix} {content}\n")
    else:
        sys.stdout.write(f"{prefix}{identifier} {content}\n")
    sys.stdout.write("\n")
    sys.stdout.flush()


def main():
    global BOARD_SIZE
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        parts = line.split()
        identifier = None
        if parts and parts[0].isdigit():
            identifier = int(parts[0])
            parts = parts[1:]
        if not parts:
            continue
        name = parts[0]
        args = parts[1:]

        if name == "name":
            respond(identifier, True, "FakeGTP")
        elif name == "version":
            respond(identifier, True, "1.0.0")
        elif name == "protocol_version":
            respond(identifier, True, "2")
        elif name == "boardsize":
            try:
                BOARD_SIZE = int(args[0])
                OCCUPIED.clear()
                respond(identifier, True, "")
            except (IndexError, ValueError):
                respond(identifier, False, "invalid size")
        elif name == "clear_board":
            OCCUPIED.clear()
            respond(identifier, True, "")
        elif name == "play":
            if len(args) >= 2:
                OCCUPIED.add(args[1])
                respond(identifier, True, "")
            else:
                respond(identifier, False, "play expects a color and a vertex")
        elif name == "genmove":
            respond(identifier, True, "D4")
        elif name == "list_commands":
            respond(identifier, True, "\n".join(sorted(KNOWN_COMMANDS)))
        elif name == "known_command":
            known = bool(args) and args[0] in KNOWN_COMMANDS
            respond(identifier, True, "true" if known else "false")
        else:
            respond(identifier, False, "unknown command")


if __name__ == "__main__":
    main()
