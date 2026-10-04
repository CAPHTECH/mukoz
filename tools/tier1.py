#!/usr/bin/env python3
"""Check the linux-x86_64 Tier 1 acceptance criteria (docs/09 9.8) on this host and write
target/tier1-report.json. Each criterion names the tests or runs that decide it; a criterion
passes only if every one of them ran and passed on this run. Item 10 (the generation-loop trial)
is a recorded experiment (selfcheck/genloop/), not rerun here: it is reported as `recorded`."""
import json, os, re, subprocess, sys, hashlib, platform

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
os.chdir(root)
if platform.system() != "Linux" or platform.machine() != "x86_64":
    sys.exit(f"Tier 1 is linux-x86_64; this host is {platform.system()}-{platform.machine()}")

CRITERIA = {
    "1 normal fixtures accepted": ["correct_variants_are_accepted", "memory_tasks_correct_are_accepted", "fixture_rows_match_their_announced_outcome",
                                   "hello_elf_is_accepted_by_emulated_and_native_process_together", "todo_files_argv_and_exit_status_agree_natively",
                                   "macho_hello_is_checked_on_the_emulator"],
    "2 mutants rejected for the announced property, replayable": ["mutations_are_rejected_with_the_announced_property", "memory_task_mutations_are_rejected",
                                   "native_mutants_are_rejected_for_the_announced_property", "fixture_rows_match_their_announced_outcome",
                                   "shrink_keeps_the_property_and_requires_and_stores_a_replayable_case", "reruns_and_isolated_replays_agree_with_the_batch"],
    "3 no stale verdicts": ["a_changed_artifact_contract_binding_or_suite_never_reuses_the_old_verdict", "i2_replay_on_changed_artifact_is_marked"],
    "4 unsupported, empty, timeout, missing are never success": ["budget_and_unsupported_are_hold_not_accept", "i1_inconclusive_is_hold", "i1_missing_claim_is_hold",
                                   "i1_skipped_cases_are_hold", "i5_vacuous_is_hold", "an_empty_scope_is_hold_never_accept", "requires_excluding_most_cases_is_hold",
                                   "nonterminating_native_cases_stop_after_a_few_and_hold", "missing_or_damaged_evidence_is_an_error",
                                   "fail_fast_rejects_without_accepting_skipped_cases", "a_failed_qualification_holds_every_emulated_result_and_is_not_rerolled",
                                   "unknown_fields_and_oversized_inputs_are_errors_not_verdicts", "the_regression_limit_never_drops_cases_or_verdicts_silently"],
    "5 deterministic reruns": ["reruns_and_isolated_replays_agree_with_the_batch", "splitmix_is_stable", "regression_cases_are_stable_across_runs"],
    "6 native-process claims no unobserved guarantees": ["native_process_alone_claims_no_memory_or_effect_guarantee", "native_alone_cannot_establish_the_memory_claim"],
    "7 summary to finding to counterexample to trace": ["summary_to_finding_to_counterexample_to_trace", "long_lists_are_paged"],
    "8 x86_64 emulated and native-routine agree in the qualified scope": ["differential_agrees_on_add64_and_reports_divergence_on_cpuid", "both_isas_qualify_with_known_answers"],
    "P3 ELF hello on emulated + native-process, mutants rejected, Mach-O native HOLD": ["hello_elf_is_accepted_by_emulated_and_native_process_together",
                                   "native_mutants_are_rejected_for_the_announced_property", "macho_hello_is_checked_on_the_emulator",
                                   "macho_native_is_host_cannot_execute_target", "macho_with_a_dylib_is_an_unresolved_dependency", "broken_macho_headers_are_format_errors",
                                   "a_dynamically_linked_elf_is_an_unresolved_dependency_not_a_violation"],
    "9 self-check stages 1-3 and independence": ["stage3_kernels_hold_only_because_the_checker_is_self", "a_listed_previous_version_is_recorded_and_can_accept"],
}

report = {"host": platform.node(), "criteria": {}}
t = subprocess.run(["cargo", "test", "--release"], capture_output=True, text=True)
results = dict(re.findall(r"^test (?:\S+::)?(\S+) \.\.\. (\w+)$", t.stdout, re.M))
report["cargo_test_exit"] = t.returncode
for name, tests in CRITERIA.items():
    missing = [x for x in tests if x not in results]
    failed = [x for x in tests if results.get(x) not in (None, "ok")]
    report["criteria"][name] = {"tests": tests, "missing": missing, "failed": failed, "pass": not missing and not failed}

# 4 (diagnostics): a build without Capstone gives the same verdict and claims.
mz = "target/release/mukoz"
subprocess.check_call(["cargo", "build", "--release", "-q", "--no-default-features", "--target-dir", "target/nodisasm"])
def run(exe):
    o = subprocess.run([exe, "check", "examples/add64/suite.x86_64.toml", "--artifact", "fixtures/x86_64/add64_mut_32bit.bin", "--store", "target/tier1-store-" + os.path.basename(os.path.dirname(os.path.dirname(exe)))], capture_output=True)
    d = json.loads(o.stdout)["data"]
    for c in d["claims"]:
        c.pop("counterexamples", None)
    return d["assessment"]["admission"], d["claims"]
same = run(mz) == run("target/nodisasm/release/mukoz")
report["criteria"]["4 diagnostics: verdict without Capstone"] = {"pass": same, "compared": "add64_mut_32bit, admission and claims"}

# 9: the self-check run (stage 1 = the test suite above, 2 = native-process CLI, 3 = kernels).
s = subprocess.run([sys.executable, "selfcheck/run.py"], capture_output=True, text=True)
sc = json.load(open("selfcheck/last-run.json"))
report["criteria"]["9 self-check run"] = {"pass": s.returncode == 0, "stages": {k: v["ok"] for k, v in sc["stages"].items()},
                                          "independence": sorted({x.get("independence") for x in sc["stages"]["3"]["suites"] if x.get("independence")} | {sc["stages"]["2"]["independence"]})}
report["criteria"]["10 generation loop"] = {"pass": None, "status": "recorded", "record": "selfcheck/genloop/2026-10-05"}

json.dump(report, open("target/tier1-report.json", "w"), indent=1)
ok = True
for name, r in report["criteria"].items():
    mark = {True: "PASS", False: "FAIL", None: "REC "}[r["pass"]]
    ok &= r["pass"] is not False
    extra = "" if r["pass"] is not False else f"  missing={r.get('missing')} failed={r.get('failed')}"
    print(f"{mark}  {name}{extra}")
sys.exit(0 if ok else 1)
