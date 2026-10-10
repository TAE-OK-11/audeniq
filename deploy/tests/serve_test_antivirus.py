#!/usr/bin/env python3
"""CI-only VERSION adapter for a real clamd with a tiny custom signature DB.

Unsigned test NDBs have no daily.cvd timestamp. Only VERSION is synthesized;
INSTREAM and all verdicts are relayed unchanged to the real ClamAV engine.
This script is never shipped in the runtime image or production deployment.
"""
import datetime
import pathlib
import socket
import socketserver
import sys


class Handler(socketserver.StreamRequestHandler):
    def handle(self):
        self.connection.settimeout(130)
        command = self.rfile.read(len(b"zVERSION\0"))
        if command == b"zVERSION\0":
            date = datetime.datetime.now(datetime.timezone.utc).strftime("%a %b %d %H:%M:%S %Y")
            self.wfile.write(f"ClamAV CI-custom-signatures/1/{date}\0".encode())
            return
        if command != b"zINSTREAM":
            return
        command += self.rfile.read(1)
        if command != b"zINSTREAM\0":
            return
        with socket.socket(socket.AF_UNIX) as upstream:
            upstream.settimeout(130)
            upstream.connect(sys.argv[2])
            upstream.sendall(command)
            total = 0
            while True:
                length = self.rfile.read(4)
                if len(length) != 4:
                    return
                n = int.from_bytes(length, "big")
                total += n
                if n > 65536 or total > 536870912:
                    return
                data = self.rfile.read(n)
                if len(data) != n:
                    return
                upstream.sendall(length + data)
                if not n:
                    break
            while True:
                reply = upstream.recv(1024)
                if not reply:
                    return
                self.wfile.write(reply)
                if b"\0" in reply:
                    return


if __name__ == "__main__":
    path = pathlib.Path(sys.argv[1])
    if path.exists():
        path.unlink()
    with socketserver.ThreadingUnixStreamServer(str(path), Handler) as server:
        path.chmod(0o600)
        server.serve_forever()
