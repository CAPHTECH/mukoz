#!/usr/bin/env python3
"""Memcheck XML (--xml=yes --xml-fd=4) -> normalized events and receipt (DESIGN.md 9.6, 9.7; WP4).

usage: memcheck.py <vg.xml>        prints {"events": [...], "receipt": {...}} as JSON
       (as a module: parse(text) -> (events, receipt); merge(...) joins it with a strace receipt)

Each <error> becomes one "mem_error" event of the instrumented process (the SUT under Valgrind) with
detail.kind (Memcheck's kind), what, auxwhat, the top frame and the allocation stack's top frame (and, for use-after-free, the free stack's top frame).
Errors are reported once per unique location; detail.count is from <errorcounts>.

Receipt:
  start  "before_sut_exec" iff the preamble and the client's argv are present (Memcheck instruments
         from the client's first instruction)
  end    "drained" iff the last <status> is FINISHED and </valgrindoutput> is present
  gaps   xml_truncated (a complete <error> before the cut still counts), error_limit_on (run without
         --error-limit=no: errors may stop being reported), errors_suppressed (a suppression matched:
         whether it was in SUT code is not known), client_sigill (an unhandled instruction looks the same
         as the SUT's own SIGILL [R]), tool_not_memcheck
  capabilities ["memcheck"] only when the tool is memcheck
"""
import json
import re
import sys
import xml.etree.ElementTree as ET

ERROR = re.compile(r"<error>.*?</error>", re.S)


def frame(st):
    f = st.find("frame") if st is not None else None
    if f is None:
        return None
    return {k: f.findtext(k) for k in ("ip", "obj", "fn", "file", "line") if f.findtext(k) is not None}


def parse(text):
    gaps, events = [], []
    tool = re.search(r"<protocoltool>(\w+)</protocoltool>", text)
    tool = tool.group(1) if tool else None
    pid = re.search(r"<pid>(\d+)</pid>", text)
    pid = int(pid.group(1)) if pid else None
    complete = text.rstrip().endswith("</valgrindoutput>")
    states = re.findall(r"<state>(\w+)</state>", text)
    if not complete:
        gaps.append({"reason": "xml_truncated"})
    if tool != "memcheck":
        gaps.append({"reason": "tool_not_memcheck"})
    vargs = re.search(r"<vargv>(.*?)</vargv>", text, re.S)
    if not vargs or "<arg>--error-limit=no</arg>" not in vargs.group(1):
        gaps.append({"reason": "error_limit_on"})
    counts = {}
    for m in re.finditer(r"<errorcounts>(.*?)</errorcounts>", text, re.S):
        for c, u in re.findall(r"<count>(\d+)</count>\s*<unique>(\w+)</unique>", m.group(1)):
            counts[u] = int(c)
    supp = re.search(r"<suppcounts>(.*?)</suppcounts>", text, re.S)
    if supp and re.search(r"<count>[1-9]", supp.group(1)):
        gaps.append({"reason": "errors_suppressed"})
    sig = re.search(r"<fatal_signal>.*?<signame>(\w+)</signame>", text, re.S)
    if sig and sig.group(1) == "SIGILL":
        gaps.append({"reason": "client_sigill"})
    for m in ERROR.finditer(text):
        x = ET.fromstring(m.group(0))
        stacks = x.findall("stack")
        u = x.findtext("unique")
        detail = {"kind": x.findtext("kind"), "what": x.findtext("what") or x.findtext("xwhat/text"),
                  "unique": u, "count": counts.get(u, 1), "frame": frame(stacks[0] if stacks else None)}
        if x.findtext("auxwhat") is not None:
            detail["auxwhat"] = x.findtext("auxwhat")
        if len(stacks) >= 3 and "free'd" in (x.findtext("auxwhat") or "") and any("Block was alloc'd at" in (a.text or "") for a in x.findall("auxwhat")):
            detail["free_frame"] = frame(stacks[1])
            detail["alloc_frame"] = frame(stacks[2])
        elif len(stacks) > 1:
            detail["alloc_frame"] = frame(stacks[1])
        events.append({"subject": {"role": "sut", "pid": pid}, "event_kind": "mem_error", "stage": "completed",
                       "resource_id": None, "result": {"success": True},
                       "attribution": {"case": "exclusive_namespace"}, "evidence_origin": "memcheck",
                       "detail": detail, "raw_evidence_ref": f"vg.xml#unique={u}"})
    started = tool is not None and "<preamble>" in text and "<argv>" in text
    end = "drained" if complete and states and states[-1] == "FINISHED" else "process_exit_unobserved"
    receipt = {"start": "before_sut_exec" if started else None, "end": end, "gaps": gaps, "lost_events": 0,
               "capabilities": ["memcheck"] if tool == "memcheck" else []}
    return events, receipt


def merge(a, b):
    """Join two collectors' receipts for one case. Conservative: any gap or missing end of either
    collector holds every claim of the case (the adapter has one receipt per case)."""
    start = a["start"] if a["start"] == b["start"] else None
    end = a["end"] if a["end"] == b["end"] == "drained" else (a["end"] if a["end"] != "drained" else b["end"])
    return {"start": start, "end": end, "gaps": a["gaps"] + b["gaps"], "lost_events": a["lost_events"] + b["lost_events"],
            "capabilities": sorted(set(a["capabilities"]) | set(b["capabilities"]))}


if __name__ == "__main__":
    ev, rc = parse(open(sys.argv[1], errors="replace").read())
    print(json.dumps({"events": ev, "receipt": rc}, indent=1))
