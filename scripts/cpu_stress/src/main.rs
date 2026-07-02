use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: cpu_stress <num_cores> <load_percent> [duration_secs]");
        eprintln!("  num_cores:    number of CPU cores to load (0 = all cores)");
        eprintln!("  load_percent: CPU load percentage per core (1-100)");
        eprintln!("  duration_secs: how long to run (default: 60)");
        std::process::exit(1);
    }

    let num_cores: usize = args[1].parse().expect("invalid num_cores");
    let load_pct: u64 = args[2].parse().expect("invalid load_percent");
    let duration_secs: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(60);

    let total_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);
    let cores_to_use = if num_cores == 0 { total_cores } else { num_cores.min(total_cores) };

    eprintln!(
        "CPU_STRESS: burning {} of {} cores at {}% load for {}s",
        cores_to_use, total_cores, load_pct, duration_secs
    );

    let running = Arc::new(AtomicBool::new(true));
    let mut handles = Vec::with_capacity(cores_to_use);

    for _core in 0..cores_to_use {
        let running = running.clone();
        handles.push(thread::spawn(move || {
            let work = Duration::from_micros(load_pct * 100);
            let idle = Duration::from_micros((100 - load_pct) * 100);
            loop {
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                let start = Instant::now();
                while start.elapsed() < work {
                    std::hint::spin_loop();
                }
                if idle > Duration::ZERO {
                    thread::sleep(idle);
                }
            }
        }));
    }

    thread::sleep(Duration::from_secs(duration_secs));
    running.store(false, Ordering::SeqCst);

    for h in handles {
        let _ = h.join();
    }

    eprintln!("CPU_STRESS: finished");
}
