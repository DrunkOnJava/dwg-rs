#!/usr/bin/env python3
"""Black-box acceptance for the asynchronous byte source (#206), never cargo test.
Runs the built stream_walk through byte_source::AsyncSource and an HTTP range
worker against a local range server, and compares it with the file-backed run.
Usage: verify_source.py <stream_walk binary>"""
import hashlib
import http.server
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request

FILES = ('line_R14.dwg', 'line_2000.dwg')


class Server(http.server.ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self):
        super().__init__(('127.0.0.1', 0), Handler)
        self.files, self.log, self.counts, self.lock = {}, [], {}, threading.Lock()

    def handle_error(self, request, client_address):
        # The fault cases close connections on purpose; anything else is a
        # server bug and is reported.
        if not isinstance(sys.exc_info()[1], (ConnectionResetError, BrokenPipeError)):
            super().handle_error(request, client_address)


class Handler(http.server.BaseHTTPRequestHandler):
    """GET /<run>/<fault>/<k>/<name>: Range requests only; the k-th request of
    a run gets the fault (ok, badrange, short, ignore: 200 with the whole file,
    oversize: a 206 longer than asked for)."""
    protocol_version = 'HTTP/1.1'
    disable_nagle_algorithm = True

    def log_message(self, *args):
        pass

    def do_GET(self):
        run, fault, k, name = self.path.lstrip('/').split('/')
        data = self.server.files[name]
        first, last = (int(v) for v in self.headers['Range'].removeprefix('bytes=').split('-'))
        with self.server.lock:
            index = self.server.counts.get(run, 0)
            self.server.counts[run] = index + 1
        if first >= len(data):
            self.server.log.append((run, first, 0))
            self.send_response(416)
            self.send_header('Content-Range', f'bytes */{len(data)}')
            self.send_header('Content-Length', '0')
            self.end_headers()
            return
        if fault == 'ignore' and index == int(k):
            self.server.log.append((run, 0, len(data)))
            self.send_response(200)
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        if fault == 'oversize' and index == int(k):
            last += 64
        body = data[first:min(last, len(data) - 1) + 1]
        self.server.log.append((run, first, len(body)))
        claimed = first + 1 if fault == 'badrange' and index == int(k) else first
        self.send_response(206)
        self.send_header('Content-Range', f'bytes {claimed}-{claimed + len(body) - 1}/{len(data)}')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        if fault == 'short' and index == int(k):
            self.wfile.write(body[:-1])
            self.wfile.flush()
            self.close_connection = True
            return
        self.wfile.write(body)


def offsets(ranges):
    return [at for first, n in ranges for at in range(first, first + n)]


def fetch(url, size, digest):
    for attempt in range(3):
        try:
            with urllib.request.urlopen(url, timeout=30) as src:
                data = src.read(size + 1)
            break
        except OSError:
            if attempt == 2:
                raise
            time.sleep(5 * (attempt + 1))
    assert len(data) == size and hashlib.sha256(data).hexdigest() == digest, url
    return data


def main():
    walk = str(Path(sys.argv[1]).resolve())
    server = Server()
    threading.Thread(target=server.serve_forever, daemon=True).start()
    port = server.server_address[1]
    receipts, runs = [], iter(range(1 << 30))

    corpus = [('synthetic_2000.dwg', Path('tests/fixtures/canonical/synthetic_2000.dwg').read_bytes())]
    manifest = json.loads(Path('tools/streaming/walk-corpus.json').read_text())
    for item in manifest['files']:
        if item['name'] in FILES:
            url = f"{manifest['source'].replace('github.com', 'raw.githubusercontent.com')}/{manifest['commit']}/{item['name']}"
            corpus.append((item['name'], fetch(url, item['bytes'], item['sha256'])))

    with tempfile.TemporaryDirectory(prefix='dwg-source-') as tmp:
        tmp = Path(tmp)
        trace = tmp / 'trace.jsonl'

        def local(name, data, *args):
            path = tmp / name
            path.write_bytes(data)
            result = subprocess.run([walk, str(path), *map(str, args), '--trace', trace], capture_output=True, timeout=600)
            reads = [json.loads(line) for line in trace.read_text().splitlines()]
            return result.returncode, result.stdout.decode(), result.stderr.decode(), [(r['offset'], r['bytes']) for r in reads]

        def remote(name, data, *args, fault='ok', k=0, length=None):
            run = f'r{next(runs)}'
            server.files[name] = data
            url = f'http://127.0.0.1:{port}/{run}/{fault}/{k}/{name}'
            result = subprocess.run([walk, name, '--url', url, '--length', str(length or len(data)), *map(str, args), '--trace', trace],
                                    capture_output=True, timeout=600)
            served = [(first, n) for r, first, n in server.log if r == run]
            delivered = [(r['offset'], r['bytes']) for r in map(json.loads, trace.read_text().splitlines())]
            return result.returncode, result.stdout.decode(), result.stderr.decode(), served, delivered

        for name, data in corpus:
            fetched = []
            for schedule in ((), ('--chunks', '1,7,256,3')):
                base = local(name, data, *schedule)
                assert base[0] == 0, (name, base[2])
                code, out, err, served, delivered = remote(name, data, *schedule)
                assert code == 0, (name, schedule, err)
                assert out == base[1], (name, schedule, 'remote output differs')
                # Every fetched byte is delivered once, in order, and nothing
                # unfetched is delivered.
                assert offsets(served) == offsets(delivered), (name, schedule, 'fetched and delivered bytes differ')
                if not schedule:
                    # One size for every poll: each fetch is exactly the grammar's demand.
                    assert served == delivered == base[3], (name, 'server ranges are not the grammar demands')
                counters = json.loads(err.strip().splitlines()[-1])
                assert counters['fetches'] == len(served) and counters['pending_polls'] >= len(served) > 0, counters
                fetched.append({'schedule': ' '.join(schedule) or '256', 'fetches': len(served),
                                'pending_polls': counters['pending_polls']})
            receipts.append({'file': name, 'sha256': hashlib.sha256(data).hexdigest(), 'runs': fetched,
                             'equal_to_file_source': True, 'fetched_bytes_delivered_once': True})

        # Fresh-process resume across the asynchronous transport.
        name, data = corpus[2]
        identity = hashlib.sha256(data).hexdigest()
        checkpoint = tmp / 'state.bin'
        held_saved = 0
        resumed = []
        # A mixed schedule leaves fetched but undelivered bytes in the adapter
        # at most stops; they travel in <checkpoint>.source, not over the wire
        # again. Three sizes, so a fetch posted at 256 bytes is delivered in
        # 1- and 7-byte feeds (a two-size cycle locks into posting at 1).
        for schedule in ((), ('--chunks', '256,1,7')):
            full = remote(name, data, *schedule, '--identity', identity)
            feeds = len(full[4])
            stops = sorted({1, 2, 3, 50, feeds // 2, feeds - 1})
            if schedule:
                stops = sorted({*range(1, 7), *range(50, 54), *range(feeds // 2, feeds // 2 + 4), feeds - 1})
            for stop in stops:
                first = remote(name, data, *schedule, '--identity', identity, '--checkpoint-out', checkpoint, '--stop-after', stop)
                assert first[0] == 0, first[2]
                held_saved += (tmp / 'state.bin.source').read_text().split()[1] != '-'
                second = remote(name, data, *schedule, '--identity', identity, '--resume', checkpoint)
                assert second[0] == 0, second[2]
                assert first[1].splitlines()[:-1] + second[1].splitlines() == full[1].splitlines(), (stop, 'output not exactly once')
                assert offsets(first[3]) + offsets(second[3]) == offsets(full[3]), (schedule, stop, 'a byte was fetched twice or skipped')
                assert offsets(first[4]) + offsets(second[4]) == offsets(full[4]), (schedule, stop, 'a byte was delivered twice or skipped')
            resumed.append({'schedule': ' '.join(schedule) or '256', 'stops': stops})
        assert held_saved >= 5, ('too few stops carried held bytes', held_saved)
        receipts.append({'file': name, 'fresh_process_resume': resumed, 'stops_with_held_bytes': held_saved,
                         'refetched_bytes': 0})

        # Faults: each ends in a typed error with only a valid prefix printed.
        expected = local(name, data)[1].splitlines()
        cut = len(data) - 1000
        faults = [
            ('server says 416 past a short file', dict(data=data[:cut], length=len(data)), 'Truncated'),
            ('Content-Range for another offset', dict(fault='badrange', k=40), 'answer is not for the posted request'),
            ('body cut short', dict(fault='short', k=40), 'failed to fill whole buffer'),
            ('server ignores Range (200, whole file)', dict(fault='ignore', k=40), 'HTTP status 200'),
            ('206 longer than the request', dict(fault='oversize', k=40), '206 body of'),
        ]
        labels = []
        for label, options, error in faults:
            body = options.pop('data', data)
            code, out, err, served, _ = remote(name, body, **options)
            assert code != 0 and error in err, (label, err)
            printed = out.splitlines()
            assert printed == expected[:len(printed)], (label, 'output is not a valid prefix')
            labels.append(label)
        receipts.append({'file': name, 'faults_rejected': labels})

    server.shutdown()
    report = {'scope': 'byte_source::AsyncSource driven by an HTTP/1.1 range worker against a local server, '
                       'through the R13-R2000 container walk. No TLS, browser worker or remote-host measurement.',
              'binary_sha256': hashlib.sha256(Path(walk).read_bytes()).hexdigest(), 'cases': receipts, 'status': 'passed'}
    Path('source-evidence.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
