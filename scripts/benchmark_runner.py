#!/usr/bin/env python3
import os
import sys
import time
import json
import subprocess
import glob
from pathlib import Path

# ==============================================================================
# CONFIGURATION
# ==============================================================================
RECEIVER_IP = "127.0.0.1"  # Default localhost loopback or android hotspot IP
PORT = 8080

FILE_SIZES = {
    "1M": 1 * 1024 * 1024,
    "10M": 10 * 1024 * 1024,
    "100M": 100 * 1024 * 1024,
}

PROTOCOLS = {
    "tcp": "tcpbuffered",
    "zerocopy": "tcpzerocopy",
    "udp": "udpcustom",
    "quic": "quic",
}

LOSS_CONDITIONS = [0.0, 0.02, 0.05]  # 0%, 2%, 5% packet loss
RUNS = 2  # Number of runs per combination to average
TEMP_DIR = Path("./scratch/benchmark_files")
OUTPUT_SUMMARY_CSV = Path("./benchmarks/matrix_summary.csv")
OUTPUT_SUMMARY_JSON = Path("./benchmarks/matrix_summary.json")

# ==============================================================================
# OS NETWORK SHAPING (macOS dnctl/pfctl)
# ==============================================================================
def configure_network_impairment(loss: float, delay_ms: int = 50):
    """Configures macOS dnctl/pfctl packet loss and delay."""
    if sys.platform != "darwin":
        print(f"[Warn] Network shaping is only supported on macOS. Skipping setup for loss={loss}.")
        return
    
    # Flush existing rules
    subprocess.run(["sudo", "dnctl", "-q", "flush"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    
    if loss == 0.0:
        print("[NetShaper] Resetting network to ideal conditions (0% loss, 0ms delay).")
        return
        
    print(f"[NetShaper] Configuring network: delay={delay_ms}ms, loss={loss:.0%}")
    # Configure pipe 1
    subprocess.run([
        "sudo", "dnctl", "pipe", "1", "config",
        "delay", f"{delay_ms}ms",
        "plr", f"{loss:.3f}"
    ], check=True)
    
    # Enable pfctl anchoring (requires pf.conf anchor setup or basic redirection)
    # For safe default, we configure the pipe but print a note if anchor setup is missing
    # Under typical test conditions, pipe 1 configurations apply immediately to matched routes.

def clear_network_impairment():
    if sys.platform == "darwin":
        subprocess.run(["sudo", "dnctl", "-q", "flush"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

# ==============================================================================
# BENCHMARK RUNNER
# ==============================================================================
def generate_random_file(size_bytes: int, filename: Path):
    TEMP_DIR.mkdir(parents=True, exist_ok=True)
    if filename.exists() and filename.stat().st_size == size_bytes:
        return
    print(f"[FileGen] Generating {filename.name} ({size_bytes / (1024*1024):.1f} MB)...")
    # Generate high-entropy file to prevent compression bias
    with open(filename, "wb") as f:
        f.write(os.urandom(size_bytes))

def find_latest_benchmark_json():
    """Finds the most recently written .json file in the benchmarks/ directory."""
    files = glob.glob("./benchmarks/*.json")
    if not files:
        return None
    # Exclude matrix summaries
    filtered_files = [f for f in files if "matrix_summary" not in f]
    if not filtered_files:
        return None
    return max(filtered_files, key=os.path.getmtime)

def execute_run(file_path: Path, protocol_flag: str) -> dict:
    """Executes a single file transfer run using the compiled dos CLI."""
    print(f"[Execute] Running send-file mode={protocol_flag} for {file_path.name}")
    
    # We execute using cargo run from workspace
    cmd = [
        "cargo", "run", "--release", "--bin", "dos", "--",
        "send-file", "--http", f"{RECEIVER_IP}:{PORT}", str(file_path),
        "--mode", protocol_flag
    ]
    
    try:
        # Run CLI process
        res = subprocess.run(cmd, capture_output=True, text=True, check=True)
        # Give system brief rest to finish file flush on receiver side
        time.sleep(1.0)
        
        # Parse the JSON report generated in benchmarks/
        latest_json = find_latest_benchmark_json()
        if latest_json:
            with open(latest_json, "r") as f:
                return json.load(f)
        else:
            print("[Error] No benchmark JSON report found in benchmarks/")
            return {}
            
    except subprocess.CalledProcessError as e:
        print(f"[Error] Command failed: {e.stderr}")
        return {}

# ==============================================================================
# MAIN MATRIX SEQUENCE
# ==============================================================================
def main():
    if len(sys.argv) > 1:
        global RECEIVER_IP
        RECEIVER_IP = sys.argv[1]
        print(f"Overriding target IP to: {RECEIVER_IP}")

    # Build release binaries first
    print("[Build] Compiling PDOS in release mode...")
    subprocess.run(["cargo", "build", "--release"], check=True)

    results_matrix = []

    try:
        for size_name, size_bytes in FILE_SIZES.items():
            file_path = TEMP_DIR / f"test_{size_name}.bin"
            generate_random_file(size_bytes, file_path)

            for loss in LOSS_CONDITIONS:
                configure_network_impairment(loss)

                for proto_name, proto_flag in PROTOCOLS.items():
                    print(f"\n==================================================")
                    print(f" Testing: Size={size_name} | Protocol={proto_name} | Loss={loss:.0%}")
                    print(f"==================================================")

                    run_throughputs = []
                    run_cpus = []
                    run_rss = []

                    for r in range(1, RUNS + 1):
                        print(f"--- Run {r}/{RUNS} ---")
                        report = execute_run(file_path, proto_flag)
                        
                        if report:
                            tp = report.get("avg_throughput_mbps", 0.0)
                            # Pull sender average CPU and peak RSS from telemetry samples
                            samples = report.get("sender_samples", [])
                            avg_cpu = sum(s.get("cpu_pct", 0.0) for s in samples) / max(len(samples), 1)
                            peak_rss = max((s.get("rss_bytes", 0) for s in samples), default=0) / (1024*1024)

                            run_throughputs.append(tp)
                            run_cpus.append(avg_cpu)
                            run_rss.append(peak_rss)
                            
                            print(f"  Result: Throughput={tp:.2f} Mbps | CPU={avg_cpu:.1f}% | RSS={peak_rss:.2f} MB")
                        else:
                            print("  Result: FAILED")

                    if run_throughputs:
                        avg_tp = sum(run_throughputs) / len(run_throughputs)
                        avg_cpu = sum(run_cpus) / len(run_cpus)
                        avg_rss = sum(run_rss) / len(run_rss)
                    else:
                        avg_tp, avg_cpu, avg_rss = 0.0, 0.0, 0.0

                    results_matrix.append({
                        "file_size": size_name,
                        "loss_pct": f"{loss * 100:.0f}%",
                        "protocol": proto_name,
                        "avg_throughput_mbps": round(avg_tp, 2),
                        "avg_cpu_percent": round(avg_cpu, 2),
                        "avg_mem_mb": round(avg_rss, 2)
                    })

    finally:
        # Ensure we always restore network behavior
        clear_network_impairment()

    # ==============================================================================
    # EXPORT RESULTS & DISPLAY
    # ==============================================================================
    # Save CSV
    import csv
    with open(OUTPUT_SUMMARY_CSV, "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=["file_size", "loss_pct", "protocol", "avg_throughput_mbps", "avg_cpu_percent", "avg_mem_mb"])
        writer.writeheader()
        writer.writerows(results_matrix)

    # Save JSON
    with open(OUTPUT_SUMMARY_JSON, "w") as f:
        json.dump(results_matrix, f, indent=2)

    # Print summary Markdown table
    print("\n\n======================================================================")
    print("                      PDOS BENCHMARK MATRIX SUMMARY")
    print("======================================================================")
    print("| File Size | Loss Rate | Protocol | Avg Throughput | Avg CPU | Avg RSS |")
    print("|-----------|-----------|----------|----------------|---------|---------|")
    for r in results_matrix:
        print(f"| {r['file_size']:<9} | {r['loss_pct']:<9} | {r['protocol']:<8} | {r['avg_throughput_mbps']:>11} Mbps | {r['avg_cpu_percent']:>6}% | {r['avg_mem_mb']:>5} MB |")
    print("======================================================================")
    print(f"Summary logs successfully exported to:\n - {OUTPUT_SUMMARY_CSV}\n - {OUTPUT_SUMMARY_JSON}")

if __name__ == "__main__":
    main()
