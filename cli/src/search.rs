use dos_networking::Connection;
use dos_protocol::{builder::search_request, Message};

pub async fn run_search(query: String) -> anyhow::Result<()> {
    let (conn, _cli_id) = crate::net::connect_and_identify().await?;

    let req = search_request(query);
    conn.send(&req).await?;

    // Wait for the response
    while let Ok(Some(msg)) = conn.recv().await {
        if let Message::SearchResponse(resp) = msg {
            println!("Found {} devices:", resp.results.len());
            for r in resp.results {
                println!(
                    "  [{:.1}] {} ({} - {}) ID: {}",
                    r.score, r.name, r.platform, r.status, r.node_id
                );
            }
            break;
        }
    }

    Ok(())
}
