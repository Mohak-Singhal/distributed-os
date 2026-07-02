#!/usr/bin/env python3
"""
PDOS Stress Test Result Validator

Usage:
    python3 scripts/validate_results.py [output_dir]

Reads benchmark JSON + meta files from stress_test_output/ and produces
structured validation results with PASS/FAIL per scenario.

Validation Rules:
  - Throughput drop > 30% vs baseline -> FAIL
  - CPU spike > 90% -> WARN
  - Transfer failure under stress -> FAIL
  - Retransmissions > 5% of total packets -> FAIL
  - Excessive reconnect count -> FAIL
"""

import json
import os
import sys
import glob
import math

PASS = "PASS"
FAIL = "FAIL"
WARN = "WARN"

THROUGHPUT_DROP_THRESHOLD = 0.30   # 30% drop vs baseline
CPU_SPIKE_THRESHOLD = 90.0         # 90% CPU
RETRANS_THRESHOLD = 0.05           # 5% retransmission ratio


def load_benchmark_json(path: str) -> dict:
    try:
        with open(path) as f:
            return json.load(f)
    except (FileNotFoundError, json.JSONDecodeError) as e:
        return {"error": str(e)}


def load_meta(path: str) -> dict:
    meta = {}
    try:
        with open(path) as f:
            for line in f:
                line = line.strip()
                if "=" in line and not line.startswith("#"):
                    k, v = line.split("=", 1)
                    meta[k] = v
    except FileNotFoundError:
        pass
    return meta


def get_baseline_throughput(output_dir: str) -> float:
    """Find baseline throughput as reference."""
    meta_path = os.path.join(output_dir, "scenario_baseline_100MB.meta")
    json_path = os.path.join(output_dir, "scenario_baseline_100MB.json")
    if os.path.exists(meta_path):
        meta = load_meta(meta_path)
        tp = meta.get("THROUGHPUT", "0")
        try:
            return float(tp)
        except ValueError:
            pass
    if os.path.exists(json_path):
        data = load_benchmark_json(json_path)
        tp = data.get("avg_throughput_mbps", data.get("average_speed_mbps", 0))
        return float(tp)
    return 0.0


def get_throughput_from_json(json_path: str) -> float:
    data = load_benchmark_json(json_path)
    if "error" in data:
        return 0.0
    tp = data.get("avg_throughput_mbps", data.get("average_speed_mbps", 0))
    try:
        return float(tp)
    except (TypeError, ValueError):
        return 0.0


def get_retransmissions(json_path: str) -> int:
    data = load_benchmark_json(json_path)
    if "error" in data:
        return 0
    # Try different field names
    retrans = data.get("retransmissions", data.get("network", {}).get("retransmissions", 0))
    try:
        return int(retrans)
    except (TypeError, ValueError):
        return 0


def get_peak_cpu(json_path: str) -> float:
    data = load_benchmark_json(json_path)
    if "error" in data:
        return 0.0
    cpu = data.get("peak_cpu_pct", data.get("resources", {}).get("peak_cpu_pct", 0))
    try:
        return float(cpu)
    except (TypeError, ValueError):
        return 0.0


def get_reconnects(json_path: str) -> int:
    data = load_benchmark_json(json_path)
    if "error" in data:
        return 0
    rc = data.get("reconnects", data.get("network", {}).get("reconnects", 0))
    try:
        return int(rc)
    except (TypeError, ValueError):
        return 0


def get_packet_loss(json_path: str) -> float:
    data = load_benchmark_json(json_path)
    if "error" in data:
        return 0.0
    loss = data.get("packet_loss_pct", data.get("network", {}).get("packet_loss_pct", 0))
    try:
        return float(loss)
    except (TypeError, ValueError):
        return 0.0


def validate_scenario(
    scenario_name: str, meta: dict, json_path: str, baseline_tp: float
) -> list:
    """Validate a single test scenario. Returns list of (status, reason) tuples."""
    results = []

    # 1. Check if transfer failed
    exit_code = meta.get("EXIT_CODE", "0")
    if exit_code != "0":
        results.append((FAIL, f"Transfer failed (exit code {exit_code})"))

    # 2. Throughput comparison vs baseline
    tp = get_throughput_from_json(json_path)
    if baseline_tp > 0 and tp > 0:
        drop_pct = (baseline_tp - tp) / baseline_tp
        if drop_pct > THROUGHPUT_DROP_THRESHOLD:
            results.append(
                (
                    FAIL,
                    f"Throughput dropped {drop_pct*100:.0f}% ({tp:.1f} vs baseline {baseline_tp:.1f} Mbps)",
                )
            )
        elif drop_pct > THROUGHPUT_DROP_THRESHOLD * 0.5:
            results.append(
                (
                    WARN,
                    f"Throughput dropped {drop_pct*100:.0f}% ({tp:.1f} vs baseline {baseline_tp:.1f} Mbps)",
                )
            )
        else:
            results.append((PASS, f"Throughput: {tp:.1f} Mbps"))
    elif tp > 0:
        results.append((PASS, f"Throughput: {tp:.1f} Mbps (no baseline)"))
    else:
        results.append((WARN, "No throughput data available"))

    # 3. CPU spike detection
    cpu = get_peak_cpu(json_path)
    if cpu > CPU_SPIKE_THRESHOLD:
        results.append((WARN, f"CPU spike detected: {cpu:.1f}%"))
    elif cpu > 0:
        results.append((PASS, f"Peak CPU: {cpu:.1f}%"))

    # 4. Retransmission detection
    retrans = get_retransmissions(json_path)
    if retrans > 0:
        results.append((PASS, f"Retransmissions: {retrans}"))
        # If we have speed samples, compute retrans ratio
        data = load_benchmark_json(json_path)
        speed_samples = data.get("speed_samples", data.get("sender_samples", []))
        if speed_samples and len(speed_samples) > 1:
            # Rough estimate of total packets
            total_packets = len(speed_samples) * 10  # approx
            if total_packets > 0 and retrans / total_packets > RETRANS_THRESHOLD:
                results.append(
                    (
                        FAIL,
                        f"Retransmission ratio {retrans}/{total_packets} > {RETRANS_THRESHOLD*100:.0f}%",
                    )
                )

    # 5. Reconnect detection
    reconnects = get_reconnects(json_path)
    if reconnects > 3:
        results.append((FAIL, f"Excessive reconnects: {reconnects}"))
    elif reconnects > 0:
        results.append((WARN, f"Reconnects: {reconnects}"))
    else:
        results.append((PASS, "No reconnections"))

    # 6. Packet loss
    loss = get_packet_loss(json_path)
    if loss > 5.0:
        results.append((WARN, f"Packet loss: {loss:.1f}%"))
    elif loss > 0:
        results.append((PASS, f"Packet loss: {loss:.1f}%"))

    return results


def main():
    output_dir = sys.argv[1] if len(sys.argv) > 1 else "stress_test_output"
    output_dir = os.path.abspath(output_dir)

    if not os.path.isdir(output_dir):
        print(f"Error: output directory not found: {output_dir}")
        print(f"Usage: {sys.argv[0]} [output_dir]")
        sys.exit(1)

    print("=" * 70)
    print("  PDOS STRESS TEST VALIDATION REPORT")
    print("=" * 70)
    print(f"  Results directory: {output_dir}")
    print()

    # Load baseline throughput
    baseline_tp = get_baseline_throughput(output_dir)
    if baseline_tp > 0:
        print(f"  Baseline throughput: {baseline_tp:.1f} Mbps")
    else:
        print(f"  {WARN}: No baseline data found")
    print()

    # Find all scenario meta files
    meta_files = sorted(glob.glob(os.path.join(output_dir, "scenario_*.meta")))
    if not meta_files:
        print(f"  Error: no scenario meta files found in {output_dir}")
        sys.exit(1)

    all_results = []
    total_pass = 0
    total_fail = 0
    total_warn = 0

    for mf in meta_files:
        scenario_name = os.path.basename(mf).replace("scenario_", "").replace(".meta", "")
        meta = load_meta(mf)
        json_path = os.path.join(
            output_dir, f"scenario_{scenario_name}.json"
        )

        desc = meta.get("DESC", scenario_name)
        print(f"  [{scenario_name}]")
        print(f"    Description: {desc}")

        validations = validate_scenario(scenario_name, meta, json_path, baseline_tp)

        scenario_pass = 0
        scenario_fail = 0
        scenario_warn = 0

        for status, reason in validations:
            status_str = status.ljust(5)
            if status == FAIL:
                scenario_fail += 1
                print(f"    {status_str} {reason}")
            elif status == WARN:
                scenario_warn += 1
                print(f"    {status_str} {reason}")
            else:
                scenario_pass += 1
                print(f"    {status_str} {reason}")

        # Overall scenario result
        if scenario_fail > 0:
            overall = FAIL
        elif scenario_warn > 0:
            overall = WARN
        else:
            overall = PASS

        total_pass += scenario_pass
        total_fail += scenario_fail
        total_warn += scenario_warn

        all_results.append(
            {
                "scenario": scenario_name,
                "description": desc,
                "overall": overall,
                "checks_passed": scenario_pass,
                "checks_failed": scenario_fail,
                "checks_warned": scenario_warn,
                "details": [{"status": s, "reason": r} for s, r in validations],
            }
        )

        print(f"    Overall: {overall}")
        print()

    # Summary
    print("=" * 70)
    print("  SUMMARY")
    print("=" * 70)
    print(f"  Passed checks: {total_pass}")
    print(f"  Warning checks: {total_warn}")
    print(f"  Failed checks: {total_fail}")
    print(f"  Total scenarios: {len(meta_files)}")

    failing_scenarios = [r for r in all_results if r["overall"] == FAIL]
    if failing_scenarios:
        print()
        print("  FAILED SCENARIOS:")
        for fs in failing_scenarios:
            print(f"    - [{fs['scenario']}] {fs['description']}")
            for d in fs["details"]:
                if d["status"] == FAIL:
                    print(f"      * {d['reason']}")

    print()

    # Write validation report
    report = {
        "baseline_throughput_mbps": baseline_tp,
        "total_checks_passed": total_pass,
        "total_checks_failed": total_fail,
        "total_checks_warned": total_warn,
        "scenarios": all_results,
    }
    report_path = os.path.join(output_dir, "validation_report.json")
    with open(report_path, "w") as f:
        json.dump(report, f, indent=2)
    print(f"  Validation report written to {report_path}")
    print()

    if total_fail > 0:
        sys.exit(1)


if __name__ == "__main__":
    main()
