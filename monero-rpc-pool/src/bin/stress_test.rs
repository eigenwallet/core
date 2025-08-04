use monero_rpc_pool::{config::Config, create_app_with_receiver};
use reqwest;
use std::time::{Duration, Instant};
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let duration_secs = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);

    let concurrency = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    println!("Stress Testing Monero RPC Pool");
    println!("   Duration: {}s", duration_secs);
    println!("   Concurrency: {}", concurrency);
    println!();

    // Start the pool server
    println!("Starting RPC pool server...");
    let config = Config::new_random_port(std::env::temp_dir(), "mainnet".to_string());
    let (app, _status_receiver, _background_handle) = create_app_with_receiver(config).await?;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let pool_url = format!("http://{}", addr);

    println!("   Server running at: {}", pool_url);

    // Spawn the server
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Give the server a moment to start
    sleep(Duration::from_millis(500)).await;

    let client = reqwest::Client::new();

    let start_time = Instant::now();
    let test_duration = Duration::from_secs(duration_secs);
    let mut success_count = 0;
    let mut error_count = 0;
    let mut total_response_time = Duration::ZERO;

    println!("Running for {} seconds...", duration_secs);

    // Spawn workers that continuously make requests
    let mut tasks = Vec::new();
    for _ in 0..concurrency {
        let client = client.clone();
        let url = format!("{}/get_info", pool_url);
        let test_duration = test_duration.clone();
        let start_time = start_time.clone();

        tasks.push(tokio::spawn(async move {
            let mut worker_success = 0;
            let mut worker_errors = 0;
            let mut worker_response_time = Duration::ZERO;

            while start_time.elapsed() < test_duration {
                let request_start = Instant::now();
                let result = client.get(&url).send().await;
                let request_duration = request_start.elapsed();

                match result {
                    Ok(response) if response.status().is_success() => {
                        worker_success += 1;
                        worker_response_time += request_duration;
                    }
                    Ok(_) | Err(_) => {
                        worker_errors += 1;
                    }
                }

                // Small delay to avoid overwhelming
                sleep(Duration::from_millis(10)).await;
            }

            (worker_success, worker_errors, worker_response_time)
        }));
    }

    // Show progress while workers run
    let progress_task = tokio::spawn(async move {
        while start_time.elapsed() < test_duration {
            let elapsed = start_time.elapsed().as_secs();
            let remaining = duration_secs.saturating_sub(elapsed);
            print!("\rRunning... {}s remaining", remaining);
            std::io::Write::flush(&mut std::io::stdout()).unwrap();
            sleep(Duration::from_secs(1)).await;
        }
    });

    // Wait for all workers to complete
    for task in tasks {
        let (worker_success, worker_errors, worker_response_time) = task.await?;
        success_count += worker_success;
        error_count += worker_errors;
        total_response_time += worker_response_time;
    }

    progress_task.abort();

    let total_duration = start_time.elapsed();
    println!("\n");

    // Results
    println!("\nBenchmark Results");
    println!("================");
    println!("Total time: {:.2}s", total_duration.as_secs_f64());
    println!("Successful requests: {}", success_count);
    println!("Failed requests: {}", error_count);
    let total_requests = success_count + error_count;
    println!(
        "Success rate: {:.1}%",
        (success_count as f64 / total_requests as f64) * 100.0
    );

    if success_count > 0 {
        let avg_response_time = total_response_time / success_count;
        println!(
            "Average response time: {:.2}ms",
            avg_response_time.as_millis()
        );
    }

    let requests_per_second = total_requests as f64 / total_duration.as_secs_f64();
    println!("Requests per second: {:.1}", requests_per_second);

    if error_count > 0 {
        println!("\n{} requests failed", error_count);
    } else {
        println!("\nAll requests successful!");
    }

    Ok(())
}
