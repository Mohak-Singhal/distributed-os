use std::net::TcpStream;
use std::io::{Read, Write};
use tempfile::TempDir;

#[tokio::test]
async fn test_file_server_observability_and_simpleperf() {
    // 1. Create a temp directory and a dummy file
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test_file.txt");
    std::fs::write(&file_path, "Hello from the PDOS file transfer test!").unwrap();

    // 2. Start the Android agent file server on a local test port
    let port = 9091;
    let download_dir = temp_dir.path().to_string_lossy().to_string();
    
    // Spawn server in background
    let server_dir = download_dir.clone();
    tokio::spawn(async move {
        dos_android::file_server::start_server(port, server_dir).await;
    });

    // Wait for server to bind
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // 3. Perform HTTP GET to download the file (this triggers start_sampler and stop_sampler)
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/api/files/test_file.txt", port);
    let res = client.get(&url).send().await.unwrap();
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let body = res.text().await.unwrap();
    assert_eq!(body, "Hello from the PDOS file transfer test!");

    // Wait a brief moment to ensure stop_sampler drops and records simpleperf
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // 4. Fetch the benchmark metrics
    let metrics_url = format!("http://127.0.0.1:{}/api/benchmark-metrics", port);
    let metrics_res = client.get(&metrics_url).send().await.unwrap();
    assert_eq!(metrics_res.status(), reqwest::StatusCode::OK);
    
    let metrics_json: serde_json::Value = metrics_res.json().await.unwrap();
    println!("Metrics response: {:?}", metrics_json);

    // 5. Assertions on simpleperf output & flamegraph
    assert_eq!(metrics_json.get("success").unwrap().as_bool(), Some(true));
    
    // Check that we got kernel profile and it's a non-empty array
    let kernel_profile = metrics_json.get("kernel_profile").unwrap();
    assert!(kernel_profile.is_array());
    let profile_arr = kernel_profile.as_array().unwrap();
    assert!(!profile_arr.is_empty());

    // Check that hot kernel functions (like copy_to_iter or memcpy) are present
    let mut found_copy = false;
    let mut found_memcpy = false;
    for entry in profile_arr {
        let func = entry.get("function").unwrap().as_str().unwrap();
        if func == "copy_to_iter" {
            found_copy = true;
        }
        if func == "memcpy" {
            found_memcpy = true;
        }
    }
    assert!(found_copy, "copy_to_iter not found in profile");
    assert!(found_memcpy, "memcpy not found in profile");

    // Check that we got a flamegraph SVG and it contains the SVG tag
    let flamegraph_svg = metrics_json.get("flamegraph_svg").unwrap().as_str().unwrap();
    assert!(flamegraph_svg.starts_with("<svg"));
    assert!(flamegraph_svg.contains("Kernel Call Graph Flamegraph"));
    assert!(flamegraph_svg.contains("copy_to_iter"));
    assert!(flamegraph_svg.ends_with("</svg>"));

    println!("✓ File server observability and simpleperf profiling tests passed successfully!");
}
