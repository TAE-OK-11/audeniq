#!/usr/bin/env python3
"""Run FreshClam through a loopback proxy that permits only official TLS tunnels.

FreshClam/libcurl may follow a redirect to HTTP. Reject that request locally,
before it can open an external plaintext connection. Certificate verification
and TLS remain end to end in FreshClam; the proxy never decrypts the tunnel.
"""
import http.server
import os
import select
import signal
import socket
import subprocess
import threading

MIRROR = "database.clamav.net"
PORT = 18080


class Tunnel(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args):
        pass

    def reject(self):
        self.send_response(403)
        self.send_header("Content-Length", "0")
        self.send_header("Connection", "close")
        self.end_headers()
        self.close_connection = True

    do_GET = do_HEAD = do_POST = do_PUT = do_DELETE = reject

    def do_CONNECT(self):
        self.close_connection = True
        if self.path != f"{MIRROR}:443":
            self.reject()
            return
        try:
            with socket.create_connection((MIRROR, 443), timeout=10) as remote:
                self.send_response(200, "Connection established")
                self.end_headers()
                self.wfile.flush()
                self.connection.settimeout(30)
                remote.settimeout(30)
                sockets = (self.connection, remote)
                remaining = 1024 * 1024 * 1024
                while remaining > 0:
                    readable, _, _ = select.select(sockets, [], [], 180)
                    if not readable:
                        return
                    for source in readable:
                        data = source.recv(min(65536, remaining))
                        if not data:
                            return
                        remaining -= len(data)
                        (remote if source is self.connection else self.connection).sendall(data)
        except OSError:
            return


def main():
    with http.server.ThreadingHTTPServer(("127.0.0.1", PORT), Tunnel) as server:
        server.daemon_threads = True
        threading.Thread(target=server.serve_forever, daemon=True).start()
        # NO_PROXY must not bypass the explicit local transport policy.
        env = {k: v for k, v in os.environ.items()
               if k.lower() not in {"http_proxy", "https_proxy", "all_proxy", "no_proxy"}}
        child = subprocess.Popen([
            "freshclam", "--daemon", "--foreground",
            "--config-file=/usr/local/share/audeniq/freshclam.conf",
        ], env=env)

        def stop(signum, _frame):
            child.send_signal(signum)

        signal.signal(signal.SIGTERM, stop)
        signal.signal(signal.SIGINT, stop)
        try:
            return child.wait()
        finally:
            server.shutdown()


if __name__ == "__main__":
    raise SystemExit(main())
