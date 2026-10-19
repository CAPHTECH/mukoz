#!/usr/bin/env python3
"""A0 client: send one case's requests to the SUT in order and record every response byte.

usage: client.py <contract.json> <case-id> <out.json>      (run inside the test-network netns)
The target is sent exactly as written in the contract (no normalization). Exit 0 iff every request
got a parseable response.
"""
import base64
import json
import socket
import sys
from pathlib import Path

SUT = ("10.77.0.2", 8000)


def one(target, headers):
    raw = f"GET {target} HTTP/1.0\r\nHost: sut\r\n" + "".join(f"{k}: {v}\r\n" for k, v in headers.items()) + "\r\n"
    with socket.create_connection(SUT, timeout=5) as s:
        s.sendall(raw.encode())
        data = b""
        while chunk := s.recv(65536):
            data += chunk
    head, _, body = data.partition(b"\r\n\r\n")
    lines = head.decode("latin-1").split("\r\n")
    status = int(lines[0].split(" ")[1])
    hdrs = dict(l.split(": ", 1) for l in lines[1:] if ": " in l)
    return {"status": status, "headers": hdrs, "body_b64": base64.b64encode(body).decode()}


def main(contract, case_id, out):
    case = next(c for c in json.loads(Path(contract).read_text())["cases"] if c["id"] == case_id)
    res, ok = [], True
    for r in case["requests"]:
        try:
            res.append(one(r["target"], r.get("headers", {})))
        except Exception as e:   # recorded, not hidden: a failed request fails the case
            res.append({"error": repr(e)})
            ok = False
    Path(out).write_text(json.dumps({"case": case_id, "responses": res}, indent=1))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(*sys.argv[1:]))
