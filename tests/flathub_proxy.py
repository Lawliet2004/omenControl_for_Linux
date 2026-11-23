"""Short-lived HTTPS CONNECT relay for Flathub downloads in a restricted VM.

Only dl.flathub.org:443 is reachable. TLS and repository signature verification
remain end-to-end in Flatpak. No listener is exposed beyond host loopback.
"""
import select
import socket
import socketserver
import threading
import time


class FlathubProxy:
    def __init__(self):
        self.requests = []
        self.lock = threading.Lock()
        owner = self

        class Handler(socketserver.StreamRequestHandler):
            timeout = 30

            def handle(self):
                line = self.rfile.readline(8193)
                if not line:
                    return
                if line not in [b'CONNECT dl.flathub.org:443 HTTP/1.0\r\n', b'CONNECT dl.flathub.org:443 HTTP/1.1\r\n']:
                    self.wfile.write(b'HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n')
                    return
                consumed = len(line)
                while True:
                    header = self.rfile.readline(8193)
                    consumed += len(header)
                    if consumed > 32768 or not header:
                        return
                    if header == b'\r\n':
                        break
                with owner.lock:
                    owner.requests.append({'host': 'dl.flathub.org', 'port': 443})
                try:
                    with socket.create_connection(('dl.flathub.org', 443), timeout=20) as upstream:
                        self.wfile.write(b'HTTP/1.1 200 Connection Established\r\n\r\n')
                        self.wfile.flush()
                        upstream.setblocking(False)
                        self.connection.setblocking(False)
                        pending = {upstream: bytearray(), self.connection: bytearray()}
                        peer = {upstream: self.connection, self.connection: upstream}
                        ended = set()
                        deadline = time.monotonic() + 1800
                        while time.monotonic() < deadline:
                            reads = [s for s in peer if s not in ended and len(pending[peer[s]]) < 262144]
                            writes = [s for s in peer if pending[s]]
                            ready, writable, failed = select.select(reads, writes, list(peer), 15)
                            if failed:
                                return
                            for stream in ready:
                                data = stream.recv(65536)
                                if not data:
                                    ended.add(stream)
                                else:
                                    pending[peer[stream]].extend(data)
                            for stream in writable:
                                sent = stream.send(pending[stream])
                                del pending[stream][:sent]
                            if any(not pending[peer[stream]] for stream in ended):
                                return
                except (OSError, TimeoutError):
                    return

        class Server(socketserver.ThreadingTCPServer):
            daemon_threads = True
            allow_reuse_address = False

        self.server = Server(('127.0.0.1', 0), Handler)
        self.port = self.server.server_address[1]
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *_):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
