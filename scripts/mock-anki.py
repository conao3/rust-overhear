import argparse
import json
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

parser = argparse.ArgumentParser(description="Local AnkiConnect mock recording createDeck and addNote requests")
parser.add_argument("--port", required=True, type=int)
parser.add_argument("--output", required=True, type=Path)
args = parser.parse_args()

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        output.write(json.dumps(request, ensure_ascii=False) + "\n")
        output.flush()
        action = request.get("action")
        if action == "createDeck":
            result = {"result": 1, "error": None}
        elif action == "addNote":
            self.server.note_id += 1
            result = {"result": self.server.note_id, "error": None}
        else:
            result = {"result": None, "error": f"Unsupported action: {action}"}
        body = json.dumps(result).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass

with args.output.open("x") as output, HTTPServer(("127.0.0.1", args.port), Handler) as server:
    server.note_id = 1612345678900
    print(json.dumps({"endpoint": f"http://127.0.0.1:{server.server_port}"}), flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
