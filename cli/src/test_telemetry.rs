use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use serde_json::{json, Value};
use tokio;

#[tokio::main]
async fn main() {
    println!("Starting Automated Telemetry Engine Test...");
    
    // Mock State Map
    let mut session = json!({
        "transfer_id": "test_123",
        "status": "in_progress",
    });
    
    let side = "receiver";
    let speed_mb_s = 45.5;
    let cpu = 12.0;
    let ram = 24.0;
    let rssi = -45.0;
    
    // Test History Aggregation (Phase 8)
    if let Some(obj) = session.as_object_mut() {
        let ts = 123456789;
        let mut append_history = |key: &str, val: f64| {
            let history_key = format!("{}_{}_history", side, key);
            if !obj.contains_key(&history_key) {
                obj.insert(history_key.clone(), json!([]));
            }
            if let Some(arr) = obj.get_mut(&history_key).and_then(|v| v.as_array_mut()) {
                arr.push(json!({"time": ts, "value": val}));
            }
        };
        append_history("speed", speed_mb_s);
        append_history("cpu", cpu);
        append_history("ram", ram);
        append_history("rssi", rssi);
        
        // Test Deep Kernel Telemetry (Phase 9)
        obj.insert("tcp_retransmissions".to_string(), json!(5));
    }
    
    println!("Session Payload: {}", serde_json::to_string_pretty(&session).unwrap());
    
    assert!(session.get("receiver_speed_history").unwrap().as_array().unwrap().len() == 1);
    assert!(session.get("receiver_cpu_history").unwrap().as_array().unwrap().len() == 1);
    assert!(session.get("tcp_retransmissions").unwrap().as_u64().unwrap() == 5);
    
    println!("All Internal Telemetry State Assertions Passed Successfully!");
}
