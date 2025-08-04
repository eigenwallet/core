use arti_client::{TorClient, TorClientConfig};
use clap::Parser;
use monero::Network;
use monero_rpc_pool::{config::Config, create_app_with_receiver, database::parse_network};
use reqwest;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tor_rtcompat::tokio::TokioRustlsRuntime;

#[derive(Parser)]
#[command(name = "stress-test")]
#[command(about = "Stress test the Monero RPC Pool")]
#[command(version)]
struct Args {
    #[arg(short, long, default_value = "60")]
    #[arg(help = "Duration to run the test in seconds")]
    duration: u64,

    #[arg(short, long, default_value = "10")]
    #[arg(help = "Number of concurrent requests")]
    concurrency: usize,

    #[arg(short, long, default_value = "mainnet")]
    #[arg(help = "Network to use (mainnet, stagenet, testnet)")]
    #[arg(value_parser = parse_network)]
    network: Network,

    #[arg(long)]
    #[arg(help = "Enable Tor routing")]
    tor: bool,

    #[arg(short, long)]
    #[arg(help = "Enable verbose logging")]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if args.verbose {
        tracing_subscriber::fmt()
            .with_env_filter("debug")
            .with_target(false)
            .init();
    }

    println!("Stress Testing Monero RPC Pool");
    println!("   Duration: {}s", args.duration);
    println!("   Concurrency: {}", args.concurrency);
    println!("   Network: {}", args.network);
    println!("   Tor: {}", args.tor);
    println!();

    // Setup Tor client if requested
    let tor_client = if args.tor {
        println!("Setting up Tor client...");
        let config = TorClientConfig::default();
        let runtime = TokioRustlsRuntime::current().expect("We are always running with tokio");

        let client = TorClient::with_runtime(runtime)
            .config(config)
            .create_unbootstrapped_async()
            .await?;

        let client = std::sync::Arc::new(client);

        let client_clone = client.clone();
        client_clone
            .bootstrap()
            .await
            .expect("Failed to bootstrap Tor client");

        Some(client)
    } else {
        None
    };

    // Start the pool server
    println!("Starting RPC pool server...");
    let config =
        Config::new_random_port_with_tor_client(std::env::temp_dir(), tor_client, args.network);
    let (app, _status_receiver, _background_handle) = create_app_with_receiver(config).await?;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let pool_url = format!("http://{}", addr);

    // Start the server in the background
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("Server error: {}", e);
        }
    });

    // Give the server a moment to start
    sleep(Duration::from_millis(500)).await;

    let client = reqwest::Client::new();

    let start_time = Instant::now();
    let test_duration = Duration::from_secs(args.duration);

    // Use atomic counters shared between all workers
    let success_count = Arc::new(AtomicU64::new(0));
    let error_count = Arc::new(AtomicU64::new(0));
    let total_response_time_nanos = Arc::new(AtomicU64::new(0));
    let should_stop = Arc::new(AtomicBool::new(false));

    println!("Running for {} seconds...", args.duration);

    // Spawn workers that continuously make requests
    let mut tasks = Vec::new();
    for _ in 0..args.concurrency {
        let client = client.clone();
        let url = format!("{}/get_info", pool_url);
        let success_count = success_count.clone();
        let error_count = error_count.clone();
        let total_response_time_nanos = total_response_time_nanos.clone();
        let should_stop = should_stop.clone();

        tasks.push(tokio::spawn(async move {
            while !should_stop.load(Ordering::Relaxed) {
                let request_start = Instant::now();

                match client.get(&url).send().await {
                    Ok(response) => {
                        if response.status().is_success() {
                            success_count.fetch_add(1, Ordering::Relaxed);
                        } else {
                            error_count.fetch_add(1, Ordering::Relaxed);
                        }
                        let elapsed_nanos = request_start.elapsed().as_nanos() as u64;
                        total_response_time_nanos.fetch_add(elapsed_nanos, Ordering::Relaxed);
                    }
                    Err(_) => {
                        error_count.fetch_add(1, Ordering::Relaxed);
                    }
                }

                // Small delay to prevent overwhelming the server
                sleep(Duration::from_millis(10)).await;
            }
        }));
    }

    // Show progress while workers run and signal stop when duration is reached
    let should_stop_clone = should_stop.clone();
    let progress_task = tokio::spawn(async move {
        while start_time.elapsed() < test_duration {
            let elapsed = start_time.elapsed().as_secs();
            let remaining = args.duration.saturating_sub(elapsed);
            print!("\rRunning... {}s remaining", remaining);
            std::io::Write::flush(&mut std::io::stdout()).unwrap();
            sleep(Duration::from_secs(1)).await;
        }
        // Signal all workers to stop
        should_stop_clone.store(true, Ordering::Relaxed);
    });

    // Wait for the test duration to complete
    let _ = progress_task.await;

    // Wait a moment for workers to see the stop signal and finish their current requests
    sleep(Duration::from_millis(100)).await;

    // Cancel any remaining worker tasks
    for task in &tasks {
        task.abort();
    }

    // Wait for tasks to finish
    for task in tasks {
        let _ = task.await;
    }

    // Final results
    println!("\r                                   "); // Clear progress line
    println!();

    let final_success_count = success_count.load(Ordering::Relaxed);
    let final_error_count = error_count.load(Ordering::Relaxed);
    let final_total_response_time_nanos = total_response_time_nanos.load(Ordering::Relaxed);

    println!("Stress Test Results:");
    println!("   Total successful requests: {}", final_success_count);
    println!("   Total failed requests: {}", final_error_count);
    println!(
        "   Total requests: {}",
        final_success_count + final_error_count
    );

    let total_requests = final_success_count + final_error_count;
    if total_requests > 0 {
        let success_rate = (final_success_count as f64 / total_requests as f64) * 100.0;
        println!("   Success rate: {:.2}%", success_rate);

        let avg_response_time_nanos = final_total_response_time_nanos / total_requests;
        let avg_response_time = Duration::from_nanos(avg_response_time_nanos);
        println!("   Average response time: {:?}", avg_response_time);

        let requests_per_second = total_requests as f64 / args.duration as f64;
        println!("   Requests per second: {:.2}", requests_per_second);
    }

    Ok(())
}
