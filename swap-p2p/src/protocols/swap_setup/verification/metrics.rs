use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::{
    counter::Counter, family::Family, gauge::Gauge, histogram::Histogram,
};
use prometheus_client::registry::Registry;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct Outcome {
    outcome: &'static str,
}

/// Shared handles: registration and actor updates refer to the same metrics.
/// Labels have a fixed vocabulary; no peer, swap or job identifiers are exported.
#[derive(Clone, Debug)]
pub(super) struct Metrics {
    pub running: Gauge,
    pub queued: Gauge,
    capacity: Gauge,
    completed: Family<Outcome, Counter>,
    rejected: Family<Outcome, Counter>,
    pub queue_wait: Histogram,
    pub duration: Histogram,
}

impl Default for Metrics {
    fn default() -> Self {
        let capacity = Gauge::default();
        capacity.set(super::MAX_CONCURRENT_JOBS as i64);
        Self {
            running: Gauge::default(),
            queued: Gauge::default(),
            capacity,
            completed: Family::default(),
            rejected: Family::default(),
            queue_wait: Histogram::new(prometheus_client::metrics::histogram::exponential_buckets(
                0.001, 2.0, 18,
            )),
            duration: Histogram::new(prometheus_client::metrics::histogram::exponential_buckets(
                0.001, 2.0, 18,
            )),
        }
    }
}

impl Metrics {
    pub fn register(&self, registry: &mut Registry) {
        let registry = registry.sub_registry_with_prefix("lock_proof_verification");
        registry.register(
            "running",
            "Jobs occupying a verification worker",
            self.running.clone(),
        );
        registry.register(
            "queued",
            "Jobs waiting in the actor queues",
            self.queued.clone(),
        );
        registry.register(
            "capacity",
            "Configured verification worker limit",
            self.capacity.clone(),
        );
        registry.register(
            "completed",
            "Completed or cancelled jobs by outcome",
            self.completed.clone(),
        );
        registry.register(
            "rejected",
            "Rejected requests by reason",
            self.rejected.clone(),
        );
        registry.register(
            "queue_wait_seconds",
            "Time from submission until a worker starts",
            self.queue_wait.clone(),
        );
        registry.register(
            "duration_seconds",
            "Time occupying a worker, including cancellation drain",
            self.duration.clone(),
        );
    }

    pub fn completed(&self, outcome: &'static str) {
        self.completed.get_or_create(&Outcome { outcome }).inc();
    }
    pub fn rejected(&self, outcome: &'static str) {
        self.rejected.get_or_create(&Outcome { outcome }).inc();
    }
}
