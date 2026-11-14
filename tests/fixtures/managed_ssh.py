#!/usr/bin/env python3
"""Local SSH control-protocol stand-in for tunnel ownership tests."""
import json
import os
import selectors
import socket
import sys

args = sys.argv[1:]
destination_arg = next((arg for arg in args if "@" in arg), "")


def option(name):
    return args[args.index(name) + 1]


path = option("-S")
if "-O" in args:
    try:
        with socket.socket(socket.AF_UNIX) as control:
            control.connect(path)
            control.sendall(json.dumps({
                "operation": option("-O"),
                "target": option("-L") if "-L" in args else "",
            }).encode())
            status = control.recv(1)
        sys.exit(0 if status == b"0" else 1)
    except OSError:
        sys.exit(1)

if "reject-auth" in destination_arg:
    print("Permission denied (publickey).", file=sys.stderr)
    sys.exit(255)

selector = selectors.DefaultSelector()
master = socket.socket(socket.AF_UNIX)
master.bind(path)
master.listen()
selector.register(master, selectors.EVENT_READ, None)
listeners = []
attempts = 0
running = True
while running:
    for key, _ in selector.select():
        connection, _ = key.fileobj.accept()
        with connection:
            if key.fileobj is not master:
                connection.sendall(key.data.encode())
                continue
            request = json.loads(connection.recv(8192))
            operation = request["operation"]
            status = b"0"
            if operation == "forward":
                attempts += 1
                host, port, destination, remote_port = request["target"].split(":")
                listener = socket.socket()
                try:
                    if "reject-forward" in destination_arg or (
                        "race-once" in destination_arg and attempts == 1
                    ):
                        raise OSError("simulated bind collision")
                    listener.bind((host, int(port)))
                    listener.listen()
                    selector.register(listener, selectors.EVENT_READ, destination)
                    listeners.append(listener)
                except OSError:
                    listener.close()
                    status = b"1"
            elif operation == "exit":
                running = False
            connection.sendall(status)

for listener in listeners:
    listener.close()
master.close()
os.unlink(path)
