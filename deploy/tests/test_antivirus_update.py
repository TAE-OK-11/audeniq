import contextlib
import importlib.util
import pathlib
import socket
import ssl
import subprocess
import tempfile
import threading
import unittest
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("updater", ROOT / "update-antivirus.py")
updater = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(updater)


@contextlib.contextmanager
def serving(handler):
    with updater.http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler) as server:
        server.daemon_threads = True
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield server
        finally:
            server.shutdown()
            thread.join()


def headers(connection):
    data = b""
    while not data.endswith(b"\r\n\r\n"):
        byte = connection.recv(1)
        if not byte:
            raise AssertionError("incomplete HTTP response")
        data += byte
    return data


class AntivirusUpdateTests(unittest.TestCase):
    def test_plaintext_never_opens_external_connection(self):
        with serving(updater.Tunnel) as server, mock.patch.object(updater.socket, "create_connection") as dial:
            for method in ("GET", "HEAD", "POST"):
                with socket.socket() as client:
                    client.settimeout(3)
                    client.connect(server.server_address)
                    client.sendall(f"{method} http://database.clamav.net/daily.cvd HTTP/1.1\r\nHost: database.clamav.net\r\n\r\n".encode())
                    self.assertIn(b" 403 ", headers(client))
            dial.assert_not_called()

    def test_tunnels_cannot_target_other_hosts_or_plaintext_ports(self):
        with serving(updater.Tunnel) as server, mock.patch.object(updater.socket, "create_connection") as dial:
            for target in ("database.clamav.net:80", "attacker.example:443", "127.0.0.1:5432", "database.clamav.net:443@attacker.example"):
                with socket.socket() as client:
                    client.settimeout(3)
                    client.connect(server.server_address)
                    client.sendall(f"CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n".encode())
                    self.assertIn(b" 403 ", headers(client))
            dial.assert_not_called()

    def test_verified_tls_tunnel_and_http_redirect_rejection(self):
        # Exercise actual certificate validation and libcurl redirect behavior.
        class Redirect(updater.http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(302)
                self.send_header("Location", "http://database.clamav.net/daily.cvd")
                self.send_header("Content-Length", "0")
                self.end_headers()

            def log_message(self, *_args):
                pass

        with tempfile.TemporaryDirectory() as folder:
            cert, key = pathlib.Path(folder) / "cert.pem", pathlib.Path(folder) / "key.pem"
            subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                            "-keyout", str(key), "-out", str(cert), "-subj", "/CN=database.clamav.net",
                            "-addext", "subjectAltName=DNS:database.clamav.net"],
                           check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            tls.minimum_version = ssl.TLSVersion.TLSv1_2
            tls.load_cert_chain(cert, key)
            with serving(Redirect) as origin, serving(updater.Tunnel) as proxy:
                origin.socket = tls.wrap_socket(origin.socket, server_side=True)
                original_dial = socket.create_connection

                def dial(address, **kwargs):
                    self.assertEqual(address, ("database.clamav.net", 443))
                    return original_dial(origin.server_address, **kwargs)

                with mock.patch.object(updater.socket, "create_connection", side_effect=dial) as calls:
                    env = {k: v for k, v in updater.os.environ.items() if not k.lower().endswith("_proxy")}
                    result = subprocess.run([
                        "curl", "--silent", "--show-error", "--fail", "--max-time", "10", "--location",
                        "--proxy", f"http://127.0.0.1:{proxy.server_port}", "--cacert", str(cert),
                        "https://database.clamav.net/daily.cvd",
                    ], env=env, capture_output=True, timeout=15)
                    self.assertEqual(result.returncode, 22, result.stderr)
                    self.assertIn(b"403", result.stderr)
                    calls.assert_called_once()


if __name__ == "__main__":
    unittest.main()
