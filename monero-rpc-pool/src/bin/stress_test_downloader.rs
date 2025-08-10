use arti_client::{TorClient, TorClientConfig};
use clap::Parser;
use cuprate_epee_encoding::{epee_object, from_bytes, to_bytes};
use futures::stream::{self, StreamExt};
use monero::Network;
use monero_rpc_pool::{
    config::Config,
    create_app_with_receiver,
    database::{network_to_string, parse_network},
};
use reqwest;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tor_rtcompat::tokio::TokioRustlsRuntime;

#[derive(Parser)]
#[command(name = "stress-test-downloader")]
#[command(about = "Download blocks 0-1000 via get_block JSON-RPC")]
#[command(version)]
struct Args {
    #[arg(short, long, default_value = "mainnet")]
    #[arg(help = "Network to use (mainnet, stagenet, testnet)")]
    #[arg(value_parser = parse_network)]
    network: Network,

    #[arg(short, long, default_value = "true")]
    #[arg(help = "Enable verbose logging")]
    verbose: bool,

    #[arg(short, long, default_value = "5")]
    #[arg(help = "Number of concurrent batch downloads")]
    concurrency: usize,

    #[arg(long)]
    #[arg(help = "Enable Tor routing")]
    tor: bool,

    #[arg(long, default_value = "100000")]
    #[arg(help = "Maximum block height to download (inclusive)")]
    max_height: u64,

    #[arg(long, default_value = "1000")]
    #[arg(help = "Number of blocks per batch")]
    batch_size: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct GetBlocksByHeightRequest {
    heights: Vec<u64>,
}

epee_object!(
    GetBlocksByHeightRequest,
    heights: Vec<u64>,
);

#[derive(Clone, Debug, PartialEq)]
struct BlockEntry {
    block: Vec<u8>,
    txs: Vec<Vec<u8>>,
}

epee_object!(
    BlockEntry,
    block: Vec<u8>,
    txs: Vec<Vec<u8>>,
);

#[derive(Clone, Debug, PartialEq)]
struct GetBlocksByHeightResponse {
    status: String,
    untrusted: bool,
    blocks: Vec<BlockEntry>,
}

epee_object!(
    GetBlocksByHeightResponse,
    status: String,
    untrusted: bool,
    blocks: Vec<BlockEntry>,
);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if args.verbose {
        tracing_subscriber::fmt()
            .with_env_filter("monero_rpc_pool=debug")
            .with_target(true)
            .init();
    }

    println!("Block Downloader Test");
    println!("   Network: {}", network_to_string(&args.network));
    println!("   Concurrency: {}", args.concurrency);
    println!("   Tor: {}", args.tor);
    println!("   Max height: {}", args.max_height);
    println!("   Batch size: {}", args.batch_size);
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

    let client = Arc::new(
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10 * 60 + 30)) // used in wallet2
            .build()
            .expect("Failed to build reqwest client"),
    );
    let bin_rpc_url = format!("{}/get_blocks_by_height.bin", pool_url);

    println!(
        "Downloading blocks 0-{} using binary format...",
        args.max_height
    );

    // Create all batch ranges
    let batch_ranges: Vec<(u64, u64)> = (0..=args.max_height)
        .step_by(args.batch_size as usize)
        .map(|batch_start| {
            let batch_end = std::cmp::min(batch_start + args.batch_size - 1, args.max_height);
            (batch_start, batch_end)
        })
        .collect();

    // Statistics tracking
    let success_count = Arc::new(AtomicU64::new(0));
    let error_count = Arc::new(AtomicU64::new(0));
    let total_bytes = Arc::new(AtomicU64::new(0));
    let start_time = std::time::Instant::now();

    // Process batches concurrently
    let _results = stream::iter(batch_ranges)
        .map(|(batch_start, batch_end)| {
            let client = Arc::clone(&client);
            let bin_rpc_url = bin_rpc_url.clone();
            let success_count = Arc::clone(&success_count);
            let error_count = Arc::clone(&error_count);
            let total_bytes = Arc::clone(&total_bytes);
            let start_time = start_time;

            async move {
                let heights: Vec<u64> = (batch_start..=batch_end).collect();
                let request = GetBlocksByHeightRequest { heights: heights.clone() };

                // Serialize request to binary format
                let request_bytes = match to_bytes(request) {
                    Ok(bytes) => bytes.to_vec(),
                    Err(e) => {
                        println!("Failed to serialize request for batch {}-{}: {}", batch_start, batch_end, e);
                        error_count.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                };

                match client
                    .post(&bin_rpc_url)
                    .header("Content-Type", "application/octet-stream")
                    .body(request_bytes)
                    .send()
                    .await
                {
                    Ok(response) => {
                        if response.status().is_success() {
                            match response.bytes().await {
                                Ok(response_bytes) => {
                                    match from_bytes::<GetBlocksByHeightResponse, _>(&mut response_bytes.as_ref()) {
                                        Ok(parsed_response) => {
                                            if parsed_response.status == "OK" {
                                                let batch_blocks = parsed_response.blocks.len();
                                                let batch_bytes: usize = parsed_response.blocks.iter().map(|b| b.block.len()).sum();
                                                let batch_txs: usize = parsed_response.blocks.iter().map(|b| b.txs.len()).sum();

                                                success_count.fetch_add(1, Ordering::Relaxed);
                                                total_bytes.fetch_add(batch_bytes as u64, Ordering::Relaxed);

                                                let elapsed = start_time.elapsed();
                                                let total_bytes_so_far = total_bytes.load(Ordering::Relaxed);
                                                let throughput_mbps = if elapsed.as_secs() > 0 {
                                                    (total_bytes_so_far as f64 / (1024.0 * 1024.0)) / elapsed.as_secs_f64()
                                                } else { 0.0 };

                                                println!("Batch {}-{}: {} blocks, {} bytes, {} txs | Total: {:.2} MB/s",
                                                    batch_start, batch_end, batch_blocks, batch_bytes, batch_txs, throughput_mbps);
                                            } else {
                                                println!("Batch {}-{}: RPC Error: {}", batch_start, batch_end, parsed_response.status);
                                                error_count.fetch_add(1, Ordering::Relaxed);
                                            }
                                        }
                                        Err(e) => {
                                            println!("Batch {}-{}: Failed to parse binary response: {}", batch_start, batch_end, e);
                                            error_count.fetch_add(1, Ordering::Relaxed);
                                        }
                                    }
                                }
                                Err(e) => {
                                    println!("Batch {}-{}: Failed to get response bytes: {}", batch_start, batch_end, e);
                                    error_count.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        } else {
                            println!("Batch {}-{}: HTTP Error: {}", batch_start, batch_end, response.status());
                            error_count.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(e) => {
                        println!("Batch {}-{}: Request failed: {}", batch_start, batch_end, e);
                        error_count.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        })
        .buffer_unordered(args.concurrency)
        .collect::<Vec<_>>()
        .await;

    // Final statistics
    let final_success_count = success_count.load(Ordering::Relaxed);
    let final_error_count = error_count.load(Ordering::Relaxed);
    let final_total_bytes = total_bytes.load(Ordering::Relaxed);
    let total_elapsed = start_time.elapsed();

    println!();
    println!("=== DOWNLOAD SUMMARY ===");
    println!("Successful batches: {}", final_success_count);
    println!("Failed batches: {}", final_error_count);
    println!("Total batches: {}", final_success_count + final_error_count);
    println!(
        "Total bytes downloaded: {} ({:.2} MB)",
        final_total_bytes,
        final_total_bytes as f64 / (1024.0 * 1024.0)
    );
    println!("Total time: {:.2}s", total_elapsed.as_secs_f64());

    let success_rate = if final_success_count + final_error_count > 0 {
        (final_success_count as f64 / (final_success_count + final_error_count) as f64) * 100.0
    } else {
        0.0
    };
    println!("Success rate: {:.2}%", success_rate);

    let avg_throughput = if total_elapsed.as_secs() > 0 {
        (final_total_bytes as f64 / (1024.0 * 1024.0)) / total_elapsed.as_secs_f64()
    } else {
        0.0
    };
    println!("Average throughput: {:.2} MB/s", avg_throughput);

    Ok(())
}
