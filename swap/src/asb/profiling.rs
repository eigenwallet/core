//! pprof CPU and jemalloc heap profiles for the ASB, served to Grafana Pyroscope.

use anyhow::{Context, Result, ensure};
use protobuf::Message;
use std::time::Duration;

const CPU_SAMPLE_FREQUENCY: i32 = 100;

pub async fn heap_pprof() -> Result<Vec<u8>> {
    let prof_ctl = jemalloc_pprof::PROF_CTL
        .as_ref()
        .context("jemalloc heap profiling is unavailable on this build")?;
    let mut prof_ctl = prof_ctl.lock().await;

    ensure!(
        prof_ctl.activated(),
        "jemalloc heap profiling is inactive; start the ASB with MALLOC_CONF=prof:true,prof_active:true"
    );

    prof_ctl
        .dump_pprof()
        .context("Failed to dump jemalloc heap profile")
}

pub async fn cpu_pprof(window: Duration) -> Result<Vec<u8>> {
    let guard = pprof::ProfilerGuardBuilder::default()
        .frequency(CPU_SAMPLE_FREQUENCY)
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()
        .context("Failed to start CPU profiler")?;

    tokio::time::sleep(window).await;

    let profile = guard
        .report()
        .build()
        .context("Failed to build CPU profile report")?
        .pprof()
        .context("Failed to encode CPU profile as pprof")?;

    profile
        .write_to_bytes()
        .context("Failed to serialize CPU pprof profile")
}
