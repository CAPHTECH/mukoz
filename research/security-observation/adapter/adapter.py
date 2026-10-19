#!/usr/bin/env python3
"""Convert normalized observation events into Mukoz external claims (WP0.5b, DESIGN.md 16.1, 7.5).

usage: adapter.py --binding B --observations O --subject-context HEX
                  --independence {independent,previous_version,self,undeclared} -o OUT

Writes OUT (schema mukoz.external_claims/0, docs/07) and OUT.receipt.json (input digests).
Exit 0 on success, 2 on malformed input (nothing is written).

The evaluation is computed only from trusted-origin events, the per-case receipt and the binding.
Nothing in the observations is copied into an evaluation, so SUT output cannot promote itself.
"""
import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

SCHEMA_OUT = "mukoz.external_claims/0"
SCHEMA_OBS = "secobs.observations/0"
SCHEMA_BINDING = "secobs.binding/0"
VERSION = "0.1.0"
SAT, VIO, INC, NOT = "SATISFIED_IN_SCOPE", "VIOLATED", "INCONCLUSIVE", "NOT_EVALUATED"

# Only these origins can count as evidence. "sut_output" (stdout, reports, logs of the SUT) never does.
TRUSTED_ORIGINS = {"os_observer", "memcheck"}
# Attribution modes under which an event belongs to the case without doubt.
CERTAIN_CASE = {"exclusive_process_group", "exclusive_namespace"}
EVENT_KINDS = {"file_open", "file_read", "file_write", "file_transfer", "net_connect", "process_exec", "mem_error",
               "mem_region_read", "unsupported_mechanism"}
MEM_INVALID = {"InvalidRead", "InvalidWrite", "InvalidFree", "MismatchedFree", "InvalidJump"}

# predicate -> (capability the receipt must list, event kinds the predicate looks at)
PREDICATES = {
    "fs.no_forbidden_open": ("fs", {"file_open"}),
    "fs.no_forbidden_read": ("fs", {"file_read", "file_transfer"}),
    "fs.no_forbidden_write": ("fs", {"file_write", "file_transfer"}),
    "net.no_forbidden_attempt": ("net", {"net_connect"}),
    "net.no_forbidden_connect": ("net", {"net_connect"}),
    "proc.allowed_exec_only": ("proc", {"process_exec"}),
    "mem.heap.addressable": ("memcheck", {"mem_error"}),
    # Reads of a contract's logical regions inside one valid allocation (F12, MEM.REGION.READ_AUTHORIZED).
    # Memcheck cannot see them, and no SEC-OBS-01 collector provides "region": the claim stays NOT_EVALUATED.
    "mem.region.read_authorized": ("region", {"mem_region_read"}),
}


class Invalid(Exception):
    pass


def need(cond, msg):
    if not cond:
        raise Invalid(msg)


def keys(obj, required, optional=(), where=""):
    need(isinstance(obj, dict), f"{where}: not an object")
    extra = set(obj) - set(required) - set(optional)
    need(not extra, f"{where}: unknown fields {sorted(extra)}")
    missing = set(required) - set(obj)
    need(not missing, f"{where}: missing fields {sorted(missing)}")
    return obj


def strings(v, where):
    need(isinstance(v, list) and all(isinstance(s, str) and s for s in v), f"{where}: not a list of strings")
    return v


def load_json(path):
    def no_dup(pairs):
        d = {}
        for k, v in pairs:
            need(k not in d, f"{path}: repeated key {k}")
            d[k] = v
        return d
    raw = Path(path).read_bytes()
    try:
        return raw, json.loads(raw, object_pairs_hook=no_dup)
    except ValueError as e:
        raise Invalid(f"{path}: {e}")


def check_binding(b):
    keys(b, ["schema", "resources", "allow", "net_allowed", "exec_allowed", "properties"], ["fills"], "binding")
    need(b["schema"] == SCHEMA_BINDING, "binding: schema")
    strings(b["resources"], "binding.resources")
    for i, a in enumerate(b["allow"]):
        keys(a, ["principal", "resource", "ops"], (), f"binding.allow[{i}]")
        need(a["resource"] in b["resources"], f"binding.allow[{i}]: resource not in catalog")
        need(set(strings(a["ops"], f"binding.allow[{i}].ops")) <= {"read", "write"}, f"binding.allow[{i}]: ops")
    strings(b["net_allowed"], "binding.net_allowed")
    strings(b["exec_allowed"], "binding.exec_allowed")
    need(isinstance(b["properties"], dict) and b["properties"], "binding.properties: empty")
    for p, pred in b["properties"].items():
        need(re.fullmatch(r"external\.[a-z0-9_.]+", p), f"binding.properties: bad id {p}")
        need(pred in PREDICATES, f"binding.properties: unknown predicate {pred}")
    for p, parts in b.get("fills", {}).items():
        need(not p.startswith("external."), f"binding.fills: {p} is not an existing Mukoz property")
        need(strings(parts, f"binding.fills.{p}") and set(parts) <= set(b["properties"]), f"binding.fills.{p}: undeclared part")


def check_event(e, where):
    keys(e, ["subject", "event_kind", "stage", "resource_id", "result", "attribution", "evidence_origin"],
         ["phase", "dest_resource_id", "access", "mechanism", "op", "detail", "raw_evidence_ref"], where)
    keys(e["subject"], ["role"], ["pid", "tid"], f"{where}.subject")
    keys(e["result"], ["success"], ["bytes", "errno"], f"{where}.result")
    keys(e["attribution"], ["case"], ["resource", "policy"], f"{where}.attribution")
    need(isinstance(e["result"]["success"], bool), f"{where}: result.success not bool")
    b = e["result"].get("bytes")
    need(b is None or (isinstance(b, int) and not isinstance(b, bool)), f"{where}: result.bytes not int")
    need(e["resource_id"] is None or (isinstance(e["resource_id"], str) and e["resource_id"]), f"{where}: resource_id")
    need(e["stage"] in {"attempted", "completed", "failed"}, f"{where}: stage")
    need(isinstance(e["event_kind"], str) and isinstance(e["evidence_origin"], str), f"{where}: kind/origin")


def check_observations(o):
    keys(o, ["schema", "binding_sha256", "cases"], ["collectors"], "observations")
    need(o["schema"] == SCHEMA_OBS, "observations: schema")
    need(isinstance(o["cases"], list), "observations.cases: not a list")
    seen = set()
    for i, c in enumerate(o["cases"]):
        w = f"observations.cases[{i}]"
        keys(c, ["case_id", "principal", "requests_sent", "receipt", "events"], (), w)
        need(isinstance(c["case_id"], str) and c["case_id"] and c["case_id"] not in seen, f"{w}: case_id empty or repeated")
        seen.add(c["case_id"])
        need(isinstance(c["requests_sent"], int) and c["requests_sent"] >= 0, f"{w}: requests_sent")
        r = keys(c["receipt"], ["start", "end", "gaps", "lost_events", "capabilities"], (), f"{w}.receipt")
        need(isinstance(r["gaps"], list) and isinstance(r["lost_events"], int), f"{w}.receipt: gaps/lost_events")
        strings(r["capabilities"], f"{w}.receipt.capabilities")
        need(isinstance(c["events"], list), f"{w}.events: not a list")
        for j, e in enumerate(c["events"]):
            check_event(e, f"{w}.events[{j}]")


def allowed(binding, principal, resource, op):
    return any(a["principal"] == principal and a["resource"] == resource and op in a["ops"] for a in binding["allow"])


def forbidden(binding, principal, resource, op):
    # Resources outside the catalog (shared libraries, /proc, ...) are outside this policy.
    return resource in binding["resources"] and not allowed(binding, principal, resource, op)


def witness(pred, e, binding, principal):
    """True if e is a counterexample to pred, None if it would need resolution to decide, else False."""
    k, ok, res = e["event_kind"], e["result"]["success"], e["resource_id"]
    n = e["result"].get("bytes") or 0
    if k == "unsupported_mechanism":
        # A path the observation does not follow (mmap, io_uring, fd passing, rename, ...). It cannot be a
        # counterexample, but it leaves undecided every predicate it could bear on (9.2).
        if res is None:
            return None
        if not pred.startswith("fs."):
            return False
        ops = [e["op"]] if e.get("op") in ("read", "write") else ["read", "write"]
        return None if any(forbidden(binding, principal, res, op) for op in ops) else False
    if k not in PREDICATES[pred][1]:
        return False
    if pred == "fs.no_forbidden_open":
        if not ok:
            return False
        # An open whose access mode is unknown is checked against both operations.
        ops = {"read": ["read"], "write": ["write"], "rw": ["read", "write"], "none": []}.get(e.get("access"), ["read", "write"])
        return None if res is None else any(forbidden(binding, principal, res, op) for op in ops)
    if pred == "fs.no_forbidden_read":
        if not (ok and n > 0):
            return False
        return None if res is None else forbidden(binding, principal, res, "read")
    if pred == "fs.no_forbidden_write":
        dst = e.get("dest_resource_id") if k == "file_transfer" else res
        if not (ok and n > 0):
            return False
        return None if dst is None else forbidden(binding, principal, dst, "write")
    if pred == "net.no_forbidden_attempt":
        return None if res is None else res not in binding["net_allowed"]
    if pred == "net.no_forbidden_connect":
        if e["stage"] == "attempted":
            # A non-blocking connect (EINPROGRESS) whose outcome the observation does not follow: to a
            # forbidden destination it leaves the predicate undecided, never satisfied.
            return None if res is None or res not in binding["net_allowed"] else False
        if not ok:
            return False
        return None if res is None else res not in binding["net_allowed"]
    if pred == "proc.allowed_exec_only":
        if not ok:
            return False
        return None if res is None else res not in binding["exec_allowed"]
    if pred == "mem.heap.addressable":
        kind = (e.get("detail") or {}).get("kind")
        return None if kind is None else kind in MEM_INVALID
    if pred == "mem.region.read_authorized":
        auth = (e.get("detail") or {}).get("authorized")
        return None if not isinstance(auth, bool) else not auth
    raise AssertionError(pred)


def gaps(case, binding_ok):
    """Reasons a case's observation cannot support SATISFIED, in a fixed order."""
    r = case["receipt"]
    out = []
    if not binding_ok:
        out.append("BINDING_STALE")
    if r["start"] != "before_sut_exec":
        out.append("MONITORING_START_MISSING")
    if r["end"] != "drained":
        out.append("MONITORING_END_MISSING")
    if r["gaps"] or r["lost_events"]:
        out.append("MONITORING_INCOMPLETE")
    if case["requests_sent"] == 0:
        out.append("VACUOUS_CASE")
    return out


def evaluate(pred, case, binding, binding_ok):
    if PREDICATES[pred][0] not in case["receipt"]["capabilities"]:
        return NOT, "REQUIRED_CAPABILITY_UNAVAILABLE"
    if not binding_ok:
        # The resource catalog and policy are not the ones the observation was made under: no verdict either way.
        return INC, "BINDING_STALE"
    unresolved = unsupported = mechanism = outcome = False
    for e in case["events"]:
        if e["evidence_origin"] not in TRUSTED_ORIGINS or e["subject"]["role"] != "sut":
            continue
        if e["event_kind"] not in EVENT_KINDS:
            unsupported = True
            continue
        w = witness(pred, e, binding, case["principal"])
        if e["event_kind"] == "unsupported_mechanism":
            mechanism |= w is None
            continue
        if w and e["attribution"]["case"] in CERTAIN_CASE:
            # A sound counterexample stands even if the log is incomplete before or after it (10.2).
            return VIO, None
        if w is None and e["stage"] == "attempted":
            outcome = True
        elif w is None or (w and e["attribution"]["case"] not in CERTAIN_CASE):
            unresolved = True
    reasons = gaps(case, binding_ok)
    if unsupported:
        reasons.append("EVENT_UNSUPPORTED")
    if mechanism:
        reasons.append("UNSUPPORTED_MECHANISM")
    if outcome:
        reasons.append("OUTCOME_UNOBSERVED")
    if unresolved:
        reasons.append("ATTRIBUTION_UNRESOLVED")
    if reasons:
        return INC, reasons[0]
    return SAT, None


def conjoin(results):
    """Fill an existing Mukoz property from several external results."""
    if any(ev == VIO for ev, _ in results):
        return VIO, None
    if all(ev == SAT for ev, _ in results):
        return SAT, None
    for want in (INC, NOT):
        for ev, reason in results:
            if ev == want:
                return want, reason
    raise AssertionError(results)


def convert(binding_raw, binding, obs_raw, obs, context, independence):
    check_binding(binding)
    check_observations(obs)
    need(re.fullmatch(r"[0-9a-f]{64}", context), "subject context: not 64 lowercase hex")
    binding_sha = hashlib.sha256(binding_raw).hexdigest()
    binding_ok = obs["binding_sha256"] == binding_sha
    props = binding["properties"]
    claims = []
    for case in obs["cases"]:
        got = {p: evaluate(pred, case, binding, binding_ok) for p, pred in props.items()}
        for p, (ev, reason) in got.items():
            claims.append({"case_id": case["case_id"], "property": p, "eval": ev, "reason": reason})
        for p, parts in binding.get("fills", {}).items():
            ev, reason = conjoin([got[q] for q in parts])
            claims.append({"case_id": case["case_id"], "property": p, "eval": ev, "reason": reason})
    out = {
        "schema": SCHEMA_OUT,
        "subject_context": context,
        "source": {"collector": "secobs-adapter", "version": VERSION},
        "independence": None if independence == "undeclared" else independence,
        "properties": list(props),
        "claims": claims,
    }
    receipt = {
        "adapter_version": VERSION,
        "binding_sha256": binding_sha,
        "binding_matches_observations": binding_ok,
        "observations_sha256": hashlib.sha256(obs_raw).hexdigest(),
        "collectors": obs.get("collectors", []),
        "cases": len(obs["cases"]),
    }
    return out, receipt


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--binding", required=True)
    ap.add_argument("--observations", required=True)
    ap.add_argument("--subject-context", required=True)
    ap.add_argument("--independence", required=True, choices=["independent", "previous_version", "self", "undeclared"])
    ap.add_argument("-o", "--out", required=True)
    a = ap.parse_args(argv)
    try:
        braw, b = load_json(a.binding)
        oraw, o = load_json(a.observations)
        out, receipt = convert(braw, b, oraw, o, a.subject_context, a.independence)
    except (Invalid, OSError) as e:
        print(f"adapter: invalid input: {e}", file=sys.stderr)
        return 2
    Path(a.out).write_text(json.dumps(out, indent=1) + "\n")
    Path(a.out + ".receipt.json").write_text(json.dumps(receipt, indent=1) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
