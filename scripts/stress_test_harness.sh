#!/bin/bash
set -o pipefail

# ============================================================================
# PDOS FILE TRANSFER STRESS TEST HARNESS
# ============================================================================
# Usage:
#   ./scripts/stress_test_harness.sh [receiver_ip] [dashboard_port]
#
#   receiver_ip:    IP of the device running the dashboard (default: 127.0.0.1)
#   dashboard_port: Port of the dashboard (default: 8080)
#
# Prerequisites:
#   - cargo (Rust toolchain)
#   - macOS (for dnctl network shaping) OR Linux with tc/netem
#   - stress-ng (optional, for CPU stress on Linux)
#   - The cpu_stress tool in scripts/cpu_stress/
#
# Output:
#   All test results go to: ./stress_test_output/
#   - metrics_*.csv        Per-second metrics logs
#   - scenario_*.json      Per-scenario results
#   - summary.json         Full test summary
# ============================================================================

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_ROOT" || exit 1

RECEIVER_IP="${1:-127.0.0.1}"
DASHBOARD_PORT="${2:-8080}"
OUTPUT_DIR="$PROJECT_ROOT/stress_test_output"
CPU_STRESS_BIN="$PROJECT_ROOT/scripts/cpu_stress/target/release/cpu_stress"
TEST_FILES_DIR="$PROJECT_ROOT/scratch/stress_test_files"
DASHBOARD_LOG="/tmp/pdos_dashboard_stress.log"
METRICS_DIR="$OUTPUT_DIR/metrics"

mkdir -p "$OUTPUT_DIR" "$TEST_FILES_DIR" "$METRICS_DIR"

# ---- Colors ----
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

pass=0
fail=0

log()  { echo -e "${BLUE}[$(date +%H:%M:%S)]${NC} $*"; }
pass_log() { echo -e "  ${GREEN}PASS:${NC} $*"; ((pass++)); }
fail_log() { echo -e "  ${RED}FAIL:${NC} $*"; ((fail++)); }
header() { echo; echo "============================================================"; echo "  $*"; echo "============================================================"; }

# ---- Warmup / Cache Management ----
warmup_and_prepare() {
    local size_mb="$1"
    log "Preparing system for benchmark..."

    # 1. Attempt to drop page cache for cold-cache measurements
    if [[ "$OSTYPE" == "darwin"* ]]; then
        sudo purge 2>/dev/null || log "${YELLOW}purge failed — SIP may be enabled. Results may reflect warm cache.${NC}"
    elif [[ -f /proc/sys/vm/drop_caches ]]; then
        echo 3 | sudo tee /proc/sys/vm/drop_caches > /dev/null 2>&1 || true
    fi

    # 2. Generate a timestamped unique file for this test (avoids page cache hits from prior runs)
    local unique_name="test_${size_mb}MB_$(date +%s).bin"
    local unique_path="$TEST_FILES_DIR/$unique_name"
    log "Generating unique ${size_mb}MB test file: $unique_name"
    dd if=/dev/urandom of="$unique_path" bs=1m count="$size_mb" 2>/dev/null
    echo "$unique_path"
}

# ---- File Generation ----
generate_file() {
    local size_mb="$1"
    local path="$TEST_FILES_DIR/test_${size_mb}MB.bin"
    if [[ -f "$path" && $(stat -f%z "$path" 2>/dev/null || stat -c%s "$path" 2>/dev/null) -eq $((size_mb * 1048576)) ]]; then
        return
    fi
    log "Generating ${size_mb}MB test file..."
    dd if=/dev/urandom of="$path" bs=1m count="$size_mb" 2>/dev/null
}

# ---- Network Shaping (macOS dnctl) ----
reset_network() {
    if [[ "$OSTYPE" == "darwin"* ]]; then
        sudo dnctl -q flush 2>/dev/null || true
    elif command -v tc &>/dev/null; then
        sudo tc qdisc del dev "$(ip route get 8.8.8.8 | awk '{print $5; exit}')" root 2>/dev/null || true
    fi
}

apply_network_loss() {
    local loss_pct="$1"
    reset_network
    if [[ "$OSTYPE" == "darwin"* ]]; then
        if [[ "$loss_pct" != "0" ]]; then
            log "Applying ${loss_pct}% packet loss via dnctl..."
            sudo dnctl pipe 1 config plr "$(echo "scale=3; $loss_pct / 100" | bc)"
        fi
    elif command -v tc &>/dev/null; then
        local iface
        iface=$(ip route get 8.8.8.8 | awk '{print $5; exit}')
        if [[ "$loss_pct" != "0" ]]; then
            log "Applying ${loss_pct}% packet loss on $iface via tc..."
            sudo tc qdisc add dev "$iface" root netem loss "$loss_pct"%
        fi
    else
        log "${YELLOW}No network shaping available, skipping${NC}"
    fi
}

apply_network_delay() {
    local delay_ms="$1"
    reset_network
    if [[ "$OSTYPE" == "darwin"* ]]; then
        log "Applying ${delay_ms}ms delay via dnctl..."
        sudo dnctl pipe 1 config delay "${delay_ms}ms"
    elif command -v tc &>/dev/null; then
        local iface
        iface=$(ip route get 8.8.8.8 | awk '{print $5; exit}')
        sudo tc qdisc add dev "$iface" root netem delay "${delay_ms}ms"
    fi
}

apply_bandwidth_limit() {
    local mbps="$1"
    reset_network
    if [[ "$OSTYPE" == "darwin"* ]]; then
        log "Applying ${mbps}Mbps bandwidth limit via dnctl..."
        sudo dnctl pipe 1 config bw "${mbps}Mbit/s"
    elif command -v tc &>/dev/null; then
        local iface
        iface=$(ip route get 8.8.8.8 | awk '{print $5; exit}')
        sudo tc qdisc add dev "$iface" root tbf rate "${mbps}mbps" burst 32kbit latency 400ms
    fi
}

# ---- CPU Stress ----
start_cpu_stress() {
    local cores="$1"
    local load="$2"
    local duration="$3"
    log "Starting CPU stress: ${cores} cores at ${load}% for ${duration}s..."

    if [[ -f "$CPU_STRESS_BIN" ]]; then
        "$CPU_STRESS_BIN" "$cores" "$load" "$duration" &
        CPU_STRESS_PID=$!
        return
    fi

    if command -v stress-ng &>/dev/null; then
        stress-ng --cpu "$cores" --cpu-load "$load" --timeout "${duration}s" &
        CPU_STRESS_PID=$!
        return
    fi

    # Fallback: simple CPU spinner with background yes processes
    local c=0
    while [[ $c -lt "$cores" ]]; do
        yes > /dev/null &
        CPU_BG_PIDS+=($!)
        ((c++))
    done
    CPU_STRESS_PID=$$
    log "${YELLOW}WARNING: fallback CPU stress (no fine-grained control)${NC}"
}

stop_cpu_stress() {
    if [[ -n "${CPU_STRESS_PID:-}" ]]; then
        kill "$CPU_STRESS_PID" 2>/dev/null || true
        wait "$CPU_STRESS_PID" 2>/dev/null || true
        unset CPU_STRESS_PID
    fi
    for pid in "${CPU_BG_PIDS[@]:-}"; do
        kill "$pid" 2>/dev/null || true
    done
    CPU_BG_PIDS=()
}

# ---- Disk Bottleneck ----
start_disk_bottleneck() {
    log "Starting disk bottleneck (background dd writer)..."
    # Write 100MB/s continuously to slow the disk
    dd if=/dev/zero of=/tmp/disk_stress.dat bs=1m count=1000 2>/dev/null &
    DISK_STRESS_PID=$!
}

stop_disk_bottleneck() {
    if [[ -n "${DISK_STRESS_PID:-}" ]]; then
        kill "$DISK_STRESS_PID" 2>/dev/null || true
        wait "$DISK_STRESS_PID" 2>/dev/null || true
        rm -f /tmp/disk_stress.dat
        unset DISK_STRESS_PID
    fi
}

# ---- Metrics Collection ----
start_metrics_collection() {
    local scenario_name="$1"
    local metrics_file="$METRICS_DIR/metrics_${scenario_name}.csv"

    {
        echo "timestamp,elapsed_sec,cpu_pct,mem_mb,net_tx_mbps,net_rx_mbps,rss_mb,threads,thermal"
        local start_time
        start_time=$(date +%s)
        while true; do
            if [[ -f "/tmp/metrics_stop_${scenario_name}" ]]; then
                rm -f "/tmp/metrics_stop_${scenario_name}"
                break
            fi
            local now
            now=$(date +%s)
            local elapsed=$((now - start_time))
            local ts
            ts=$(date +%Y-%m-%dT%H:%M:%S)

            # CPU and memory via ps
            local cpu mem rss threads
            cpu=$(ps -p $$ -o %cpu= 2>/dev/null | tr -d ' ' || echo "0")
            mem=$(ps -p $$ -o %mem= 2>/dev/null | tr -d ' ' || echo "0")
            rss=$(ps -p $$ -o rss= 2>/dev/null | tr -d ' ' || echo "0")
            threads=$(ps -M -p $$ 2>/dev/null | wc -l | tr -d ' ' || echo "0")

            # Network via netstat (macOS)
            local tx_mbps=0 rx_mbps=0
            if [[ "$OSTYPE" == "darwin"* ]]; then
                local net_stats
                net_stats=$(netstat -ib -I en0 2>/dev/null | tail -1)
                if [[ -n "$net_stats" ]]; then
                    local packets_in bytes_in packets_out bytes_out
                    packets_in=$(echo "$net_stats" | awk '{print $5}')
                    bytes_in=$(echo "$net_stats" | awk '{print $7}')
                    packets_out=$(echo "$net_stats" | awk '{print $6}')
                    bytes_out=$(echo "$net_stats" | awk '{print $8}')
                fi
            fi

            # Thermal via pmset
            local thermal="nominal"
            if [[ "$OSTYPE" == "darwin"* ]]; then
                local therm_out
                therm_out=$(pmset -g therm 2>/dev/null)
                if echo "$therm_out" | grep -q "CPU_Scheduler_Limit"; then
                    thermal="throttled"
                fi
            fi

            echo "$ts,$elapsed,$cpu,$mem,$tx_mbps,$rx_mbps,$rss,$threads,$thermal" >> "$metrics_file"

            # Also collect system-level stats using iostat and vm_stat
            if [[ "$OSTYPE" == "darwin"* ]]; then
                iostat -d disk0 1 1 2>/dev/null | tail -1 >> "${metrics_file%.csv}_disk.csv" 2>/dev/null
                vm_stat 2>/dev/null | head -20 >> "${metrics_file%.csv}_vmstat.txt" 2>/dev/null
            fi

            sleep 1
        done
    } &
    METRICS_PID=$!
    log "Metrics collection started (PID $METRICS_PID)..."
}

stop_metrics_collection() {
    local scenario_name="$1"
    touch "/tmp/metrics_stop_${scenario_name}"
    if [[ -n "${METRICS_PID:-}" ]]; then
        wait "$METRICS_PID" 2>/dev/null || true
        unset METRICS_PID
    fi
    log "Metrics collection stopped."
}

# ---- Dashboard (Receiver) Management ----
start_dashboard() {
    local port="$1"
    log "Starting dashboard receiver on port $port..."
    # Kill any existing dashboard
    lsof -ti:"$port" 2>/dev/null | xargs kill -9 2>/dev/null || true
    sleep 1

    cd "$PROJECT_ROOT" || exit 1
    cargo run --release --bin dos -- dashboard "$port" > "$DASHBOARD_LOG" 2>&1 &
    DASHBOARD_PID=$!
    log "Dashboard PID: $DASHBOARD_PID"

    # Wait for dashboard to be ready
    local i=0
    while [[ $i -lt 30 ]]; do
        if lsof -ti:"$port" 2>/dev/null | grep -q .; then
            log "Dashboard ready on http://${RECEIVER_IP}:${port}"
            return 0
        fi
        sleep 1
        ((i++))
    done
    log "${RED}Dashboard failed to start${NC}"
    return 1
}

stop_dashboard() {
    if [[ -n "${DASHBOARD_PID:-}" ]]; then
        kill "$DASHBOARD_PID" 2>/dev/null || true
        wait "$DASHBOARD_PID" 2>/dev/null || true
        unset DASHBOARD_PID
    fi
    lsof -ti:"$DASHBOARD_PORT" 2>/dev/null | xargs kill -9 2>/dev/null || true
}

# ---- File Transfer Run ----
run_transfer() {
    local file_path="$1"
    local mode="$2"
    local scenario_name="$3"
    local log_file="$OUTPUT_DIR/transfer_${scenario_name}.log"
    local json_output="$OUTPUT_DIR/scenario_${scenario_name}.json"

    log "Running transfer: $file_path (mode: $mode)"

    # The dos CLI outputs benchmark JSON to cli/benchmarks/
    # Capture stdout and also look for the generated JSON
    cd "$PROJECT_ROOT" || exit 1

    # Clean up any previous benchmark JSON for this transfer
    rm -f "$PROJECT_ROOT/benchmarks/benchmark_"*.json 2>/dev/null

    # Run the transfer
    cargo run --release --bin dos -- send-file --http "${RECEIVER_IP}:${DASHBOARD_PORT}" "$file_path" --mode "$mode" > "$log_file" 2>&1
    local exit_code=$?

    # Find the latest benchmark JSON
    local latest_json
    latest_json=$(ls -t "$PROJECT_ROOT/benchmarks/"benchmark_*.json 2>/dev/null | head -1)

    if [[ -n "$latest_json" ]]; then
        cp "$latest_json" "$json_output"
        log "Benchmark JSON saved to $json_output"
    else
        log "${YELLOW}No benchmark JSON generated${NC}"
        echo '{"error": "no_benchmark_json"}' > "$json_output"
    fi

    return $exit_code
}

# ---- Test a file transfer and validate ----
test_scenario() {
    local scenario_name="$1"
    local desc="$2"
    local file_size_mb="$3"
    local mode="$4"

    header "$desc"

    # Generate test file
    generate_file "$file_size_mb"
    local file_path="$TEST_FILES_DIR/test_${file_size_mb}MB.bin"

    # Start metrics collection
    start_metrics_collection "$scenario_name"

    # Run the transfer
    local transfer_start
    transfer_start=$(date +%s%N)
    run_transfer "$file_path" "$mode" "$scenario_name"
    local transfer_exit=$?
    local transfer_end
    transfer_end=$(date +%s%N)

    # Stop metrics
    stop_metrics_collection "$scenario_name"

    # Calculate elapsed
    local elapsed_ms=$(( (transfer_end - transfer_start) / 1000000 ))
    local elapsed_sec
    elapsed_sec=$(echo "scale=2; $elapsed_ms / 1000" | bc)

    # Parse benchmark JSON for validation
    local json_file="$OUTPUT_DIR/scenario_${scenario_name}.json"
    local throughput=0
    local cpu=0
    local mem=0

    if [[ -f "$json_file" ]]; then
        throughput=$(python3 -c "
import json
with open('$json_file') as f:
    d = json.load(f)
print(d.get('avg_throughput_mbps', d.get('average_speed_mbps', 0)))
" 2>/dev/null || echo "0")
        cpu=$(python3 -c "
import json
with open('$json_file') as f:
    d = json.load(f)
print(d.get('average_cpu_pct', d.get('peak_cpu_pct', 0)))
" 2>/dev/null || echo "0")
        mem=$(python3 -c "
import json
with open('$json_file') as f:
    d = json.load(f)
print(d.get('average_ram_mb', 0))
" 2>/dev/null || echo "0")
    fi

    if [[ $transfer_exit -eq 0 ]]; then
        pass_log "Transfer completed: ${throughput} Mbps avg, ${cpu}% CPU, ${mem} MB RAM (${elapsed_sec}s)"
    else
        fail_log "Transfer failed (exit code $transfer_exit)"
    fi

    # Save scenario metadata
    cat >> "$OUTPUT_DIR/scenario_${scenario_name}.meta" <<METAEOF
SCENARIO=$scenario_name
DESC=$desc
FILE_SIZE_MB=$file_size_mb
MODE=$mode
ELAPSED_SEC=$elapsed_sec
THROUGHPUT=$throughput
CPU=$cpu
MEM=$mem
EXIT_CODE=$transfer_exit
METAEOF
}

# ---- Stress Tests ----
stress_cpu() {
    local file_size_mb="$1"
    local mode="$2"
    local scenario_name="cpu_stress_${file_size_mb}MB"

    header "SCENARIO 2: CPU Stress Test (${file_size_mb}MB, mode=$mode)"

    local total_cores
    total_cores=$(sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4)
    local load_cores=$(( total_cores > 2 ? total_cores - 2 : 1 ))

    start_cpu_stress "$load_cores" 90 600  # 10 min max
    sleep 3  # let stress ramp up

    test_scenario "$scenario_name" "Transfer under ${load_cores}-core 90% CPU load" "$file_size_mb" "$mode"

    stop_cpu_stress
}

stress_packet_loss() {
    local file_size_mb="$1"
    local loss_pct="$2"
    local mode="$3"
    local scenario_name="loss_${loss_pct}pct_${file_size_mb}MB"

    header "SCENARIO 4: Packet Loss ${loss_pct}% (${file_size_mb}MB, mode=$mode)"

    apply_network_loss "$loss_pct"
    sleep 1

    test_scenario "$scenario_name" "Transfer under ${loss_pct}% packet loss" "$file_size_mb" "$mode"

    reset_network
}

stress_bandwidth() {
    local file_size_mb="$1"
    local bw_limit="$2"
    local mode="$3"
    local scenario_name="bw_${bw_limit}mbps_${file_size_mb}MB"

    header "SCENARIO 5: Bandwidth Limit ${bw_limit} Mbps (${file_size_mb}MB, mode=$mode)"

    apply_bandwidth_limit "$bw_limit"
    sleep 1

    test_scenario "$scenario_name" "Transfer under ${bw_limit} Mbps bandwidth cap" "$file_size_mb" "$mode"

    reset_network
}

stress_disk() {
    local file_size_mb="$1"
    local mode="$2"
    local scenario_name="disk_bottleneck_${file_size_mb}MB"

    header "SCENARIO 6: Disk Bottleneck Test (${file_size_mb}MB, mode=$mode)"

    start_disk_bottleneck
    sleep 2

    test_scenario "$scenario_name" "Transfer under disk write contention" "$file_size_mb" "$mode"

    stop_disk_bottleneck
}

stress_intermittent() {
    local file_size_mb="$1"
    local mode="$2"
    local scenario_name="intermittent_${file_size_mb}MB"

    header "SCENARIO 7: Intermittent Disconnect Simulation (${file_size_mb}MB, mode=$mode)"

    # Start metrics
    start_metrics_collection "$scenario_name"

    generate_file "$file_size_mb"
    local file_path="$TEST_FILES_DIR/test_${file_size_mb}MB.bin"

    # Run transfer but periodically drop connection
    local transfer_start
    transfer_start=$(date +%s%N)

    # Start transfer in background
    cd "$PROJECT_ROOT" || exit 1
    rm -f "$PROJECT_ROOT/benchmarks/"benchmark_*.json 2>/dev/null
    cargo run --release --bin dos -- send-file --http "${RECEIVER_IP}:${DASHBOARD_PORT}" "$file_path" --mode "$mode" > "$OUTPUT_DIR/transfer_${scenario_name}.log" 2>&1 &
    TRANSFER_PID=$!

    # Simulate disconnects: kill and restart dashboard every ~3 seconds
    local cycles=0
    while kill -0 "$TRANSFER_PID" 2>/dev/null; do
        if [[ $cycles -gt 0 ]]; then
            log "Simulating disconnect (cycle $cycles)..."
            # Kill dashboard to force disconnect
            lsof -ti:"$DASHBOARD_PORT" 2>/dev/null | xargs kill -9 2>/dev/null || true
            sleep 2
            # Restart dashboard
            start_dashboard "$DASHBOARD_PORT"
            sleep 2
        fi
        sleep 3
        ((cycles++))
        if [[ $cycles -gt 5 ]]; then
            break
        fi
    done

    wait "$TRANSFER_PID" 2>/dev/null
    local transfer_exit=$?
    local transfer_end
    transfer_end=$(date +%s%N)

    stop_metrics_collection "$scenario_name"

    local elapsed_ms=$(( (transfer_end - transfer_start) / 1000000 ))
    local elapsed_sec
    elapsed_sec=$(echo "scale=2; $elapsed_ms / 1000" | bc)

    # Find benchmark JSON
    local json_file="$OUTPUT_DIR/scenario_${scenario_name}.json"
    local latest_json
    latest_json=$(ls -t "$PROJECT_ROOT/benchmarks/"benchmark_*.json 2>/dev/null | head -1)
    if [[ -n "$latest_json" ]]; then
        cp "$latest_json" "$json_file"
    else
        echo '{"error": "no_benchmark_json"}' > "$json_file"
    fi

    if [[ $transfer_exit -eq 0 ]]; then
        pass_log "Transfer completed with ${cycles} disconnect cycles (${elapsed_sec}s)"
    else
        fail_log "Transfer interrupted after ${cycles} disconnect cycles (exit=$transfer_exit)"
    fi

    cat >> "$OUTPUT_DIR/scenario_${scenario_name}.meta" <<METAEOF
SCENARIO=$scenario_name
DESC=Intermittent disconnect ($cycles cycles)
FILE_SIZE_MB=$file_size_mb
MODE=$mode
ELAPSED_SEC=$elapsed_sec
DISCONNECT_CYCLES=$cycles
EXIT_CODE=$transfer_exit
METAEOF
}

verify_dashboard_on_port() {
    local port="$1"
    if lsof -ti:"$port" 2>/dev/null | grep -q .; then
        return 0
    fi
    return 1
}

# ============================================================================
# MAIN
# ============================================================================
main() {
    log "=== PDOS File Transfer Stress Test Harness ==="
    log "Receiver: ${RECEIVER_IP}:${DASHBOARD_PORT}"
    log "Output:   $OUTPUT_DIR"
    log ""

    # Check prerequisites
    if ! cargo build --release --bin dos 2>&1 | tail -5; then
        log "${RED}Failed to build dos binary${NC}"
        exit 1
    fi
    log "dos binary built successfully"

    # Verify cpu_stress binary
    if [[ ! -f "$CPU_STRESS_BIN" ]]; then
        log "${YELLOW}cpu_stress not built. Building...${NC}"
        (cd "$PROJECT_ROOT/scripts/cpu_stress" && cargo build --release) || true
    fi

    # Generate test files
    header "Generating Test Files"
    generate_file 100
    generate_file 500
    # Generate 1GB if enough space
    local avail_gb
    avail_gb=$(df -g "$TEST_FILES_DIR" 2>/dev/null | awk 'NR==2{print $4}')
    if [[ -n "$avail_gb" && "$avail_gb" -gt 5 ]]; then
        generate_file 1024
    else
        log "${YELLOW}Skipping 1GB file (only ${avail_gb}GB available)${NC}"
    fi
    log "Test files generated in $TEST_FILES_DIR"
    ls -lh "$TEST_FILES_DIR/"

    # Start dashboard
    header "Starting Receiver Dashboard"
    start_dashboard "$DASHBOARD_PORT" || {
        log "${RED}Cannot start dashboard. Is port $DASHBOARD_PORT in use?${NC}"
        exit 1
    }

    # Cleanup on exit
    trap 'log "Cleaning up..."; stop_dashboard; reset_network; stop_cpu_stress; stop_disk_bottleneck; rm -f /tmp/metrics_stop_*' EXIT INT TERM

    # ---- Run All Scenarios ----
    local scenario_index=0
    local all_meta=()

    # SCENARIO 1: Baseline
    header "===== SCENARIO 1: Baseline Transfer ====="
    test_scenario "baseline_100MB" "Normal transfer (baseline, 100MB)" 100 "tcp"
    all_meta+=("baseline_100MB")

    # SCENARIO 2: CPU Stress
    stress_cpu 100 "zerocopy"
    all_meta+=("cpu_stress_100MB")

    # SCENARIO 3: Thermal (CPU frequency capping simulation)
    header "===== SCENARIO 3: Thermal Throttling Simulation ====="
    if [[ "$OSTYPE" == "darwin"* ]]; then
        log "Applying thermal throttle simulation (60% CPU frequency scaling)..."
        # macOS: we simulate by capping available CPU
        local half_cores
        half_cores=$(( $(sysctl -n hw.ncpu) / 2 ))
        start_cpu_stress "$half_cores" 100 600
        sleep 3
        test_scenario "thermal_100MB" "Thermal throttling simulation (50% cores busy)" 100 "tcp"
        stop_cpu_stress
    else
        # Linux: throttle with cpufreq
        local gov
        gov=$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null || echo "unknown")
        log "Setting CPU governor to powersave..."
        echo powersave | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor 2>/dev/null || true
        test_scenario "thermal_100MB" "Thermal throttling simulation (powersave governor)" 100 "tcp"
        echo "$gov" | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor 2>/dev/null || true
    fi
    all_meta+=("thermal_100MB")

    # SCENARIO 4: Packet Loss
    stress_packet_loss 100 1 "tcp"
    all_meta+=("loss_1pct_100MB")
    stress_packet_loss 100 5 "tcp"
    all_meta+=("loss_5pct_100MB")

    # SCENARIO 5: Bandwidth Drop
    stress_bandwidth 100 100 "tcp"
    all_meta+=("bw_100mbps_100MB")
    stress_bandwidth 100 10 "tcp"
    all_meta+=("bw_10mbps_100MB")

    # SCENARIO 6: Disk Bottleneck
    stress_disk 100 "tcp"
    all_meta+=("disk_bottleneck_100MB")

    # SCENARIO 7: Intermittent Disconnect
    stress_intermittent 100 "tcp"
    all_meta+=("intermittent_100MB")

    # ---- Summary ----
    header "TEST SUMMARY"
    echo "Passed: $pass | Failed: $fail | Total: $((pass + fail))"
    echo ""

    for meta in "${all_meta[@]}"; do
        local meta_file="$OUTPUT_DIR/scenario_${meta}.meta"
        if [[ -f "$meta_file" ]]; then
            echo "--- $meta ---"
            cat "$meta_file"
            echo ""
        fi
    done

    if [[ $fail -gt 0 ]]; then
        log "${RED}Some tests FAILED${NC}"
    else
        log "${GREEN}All tests PASSED${NC}"
    fi

    # Generate summary JSON
    python3 -c "
import json, os, glob

results = []
meta_dir = '$OUTPUT_DIR'
for mf in glob.glob(os.path.join(meta_dir, 'scenario_*.meta')):
    meta = {}
    with open(mf) as f:
        for line in f:
            if '=' in line:
                k, v = line.strip().split('=', 1)
                meta[k] = v
    results.append(meta)

summary = {
    'test_date': '$(date -u +%Y-%m-%dT%H:%M:%SZ)',
    'receiver': '${RECEIVER_IP}:${DASHBOARD_PORT}',
    'total_tests': $((pass + fail)),
    'passed': $pass,
    'failed': $fail,
    'scenarios': results
}
with open(os.path.join(meta_dir, 'summary.json'), 'w') as f:
    json.dump(summary, f, indent=2)
print('Summary written to', os.path.join(meta_dir, 'summary.json'))
"

    log "All results in: $OUTPUT_DIR"
    log "Done."
}

main "$@"
