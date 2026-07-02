use std::collections::HashMap;
use std::sync::Mutex;

static PAIRING_PIN: std::sync::OnceLock<Mutex<Option<String>>> = std::sync::OnceLock::new();
static TRUSTED_PEERS: std::sync::OnceLock<Mutex<HashMap<String, String>>> = std::sync::OnceLock::new(); // Token -> Device Name

fn get_trusted_peers_path() -> std::path::PathBuf {
    dirs::home_dir().unwrap_or_default().join(".pdos/trusted_peers.json")
}

pub fn load_trusted_peers() {
    let path = get_trusted_peers_path();
    if let Ok(data) = std::fs::read_to_string(&path) {
        if let Ok(peers) = serde_json::from_str::<HashMap<String, String>>(&data) {
            TRUSTED_PEERS.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap().extend(peers);
        }
    } else {
        TRUSTED_PEERS.get_or_init(|| Mutex::new(HashMap::new()));
    }
}

pub fn save_trusted_peers() {
    let path = get_trusted_peers_path();
    if let Some(mutex) = TRUSTED_PEERS.get() {
        if let Ok(peers) = mutex.lock() {
            let _ = std::fs::write(&path, serde_json::to_string(&*peers).unwrap());
        }
    }
}

pub fn generate_pin() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let pin: String = (0..6).map(|_| rng.gen_range(0..10).to_string()).collect();
    
    let pin_mutex = PAIRING_PIN.get_or_init(|| Mutex::new(None));
    *pin_mutex.lock().unwrap() = Some(pin.clone());
    
    // Auto-expire PIN after 60 seconds
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        if let Some(m) = PAIRING_PIN.get() {
            *m.lock().unwrap() = None;
        }
    });
    
    pin
}

pub fn attempt_pair(pin: &str, device_name: &str) -> Option<String> {
    let pin_mutex = PAIRING_PIN.get_or_init(|| Mutex::new(None));
    let mut current_pin = pin_mutex.lock().unwrap();
    
    if let Some(ref valid_pin) = *current_pin {
        if valid_pin == pin {
            // Success! Generate token.
            let token = uuid::Uuid::new_v4().to_string();
            if let Some(peers_mutex) = TRUSTED_PEERS.get() {
                peers_mutex.lock().unwrap().insert(token.clone(), device_name.to_string());
                save_trusted_peers();
            }
            *current_pin = None; // Invalidate PIN
            return Some(token);
        }
    }
    None
}

pub fn is_authenticated(token: &str) -> bool {
    if let Some(peers_mutex) = TRUSTED_PEERS.get() {
        return peers_mutex.lock().unwrap().contains_key(token);
    }
    false
}
