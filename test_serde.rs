use serde::Serialize;
#[derive(Serialize)]
#[serde(tag = "type", content = "data")]
enum AgentEvent {
    Connected { relay_url: String },
}
fn main() {
    let e = AgentEvent::Connected { relay_url: "ws://127.0.0.1".into() };
    println!("{}", serde_json::to_string(&e).unwrap());
}
