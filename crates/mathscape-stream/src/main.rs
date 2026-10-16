use mathscape_stream::{EpochReport, Gauges, Health, MechanismEvent, Writer};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info};

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,egg=warn")),
        )
        .with_writer(std::io::stderr)
        .init();
    let config = match mathscape_config::load() {
        Ok(c) => c,
        Err(e) => {
            error!("configuration: {e}");
            return ExitCode::from(2);
        }
    };
    let Some(stream) = config.stream else {
        error!("no `stream` section: set stream.seed in mathscape.yaml (or MATHSCAPE_STREAM__SEED); the stream never seeds itself from entropy");
        return ExitCode::from(2);
    };
    let gauges = Arc::new(Gauges::new(stream.seed));
    let _health = match stream.health_addr.as_deref().map(|addr| Health::serve(addr, Arc::clone(&gauges)).map(|h| (addr, h))) {
        None => None,
        Some(Ok((addr, h))) => {
            info!(addr, bound = %h.local_addr(), "health and metrics");
            Some(h)
        }
        Some(Err(e)) => {
            error!("health: {e}");
            return ExitCode::from(1);
        }
    };
    let mut writer = match Writer::open(stream) {
        Ok(w) => w,
        Err(e) => {
            error!("open: {e}");
            return ExitCode::from(1);
        }
    };
    gauges.opened(writer.epoch(), writer.library().len(), writer.resumed_from().is_some());
    match writer.resumed_from() {
        Some(h) => info!(head = %h.to_hex(), epoch = writer.epoch(), library = writer.library().len(), "resumed from HEAD"),
        None => info!(seed = writer.config().seed, "fresh stream from seed"),
    }
    let started = Instant::now();
    let mut certified = 0usize;
    let mut steady = 0usize;
    let mut first_steady: Option<Instant> = None;
    let mut mutations = Vec::new();
    let result = writer.run(|r: &EpochReport| {
        gauges.observe(r);
        certified += r.certified;
        if r.epoch > 0 {
            steady += r.certified;
            first_steady.get_or_insert_with(Instant::now);
        }
        let (cursor, frontier) = r.frontier.unwrap_or_default();
        if let Some(e) = r.emergence {
            info!(
                epoch = r.epoch,
                size = e.size,
                enumerated = e.enumerated,
                screened = e.screened,
                named = e.named,
                laws = e.laws,
                held = e.held,
                certified = r.certified,
                rejected = r.rejected,
                library = r.library,
                snapshot = r.snapshot.map(|h| h.to_hex()).unwrap_or_default(),
                "epoch"
            );
            return;
        }
        info!(
            epoch = r.epoch,
            candidates = r.candidates,
            certified = r.certified,
            rejected = r.rejected,
            library = r.library,
            frontier_cursor = cursor,
            frontier = frontier,
            snapshot = r.snapshot.map(|h| h.to_hex()).unwrap_or_default(),
            "epoch"
        );
        if let Some(event) = r.mechanism {
            mutations.push((r.epoch, event));
            info!(epoch = r.epoch, event = ?event, "mechanism");
        }
    });
    let secs = started.elapsed().as_secs_f64();
    match result {
        Ok(head) => {
            let per_hour = if secs > 0.0 { certified as f64 * 3600.0 / secs } else { 0.0 };
            let steady_secs = first_steady.map_or(0.0, |t| t.elapsed().as_secs_f64());
            let steady_per_hour = if steady_secs > 0.0 { steady as f64 * 3600.0 / steady_secs } else { 0.0 };
            info!(
                head = head.map(|h| h.to_hex()).unwrap_or_default(),
                epochs = writer.epochs_run(),
                certified,
                certified_after_epoch_0 = steady,
                seconds = secs,
                certified_per_hour = per_hour,
                certified_per_hour_after_epoch_0 = steady_per_hour,
                mutations = mutations.iter().filter(|(_, e)| *e == MechanismEvent::Mutated).count(),
                saturations = mutations.iter().filter(|(_, e)| *e == MechanismEvent::Saturated).count(),
                mechanism = writer.mechanism().current.brief(),
                "stream reached its budget"
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            error!("stream: {e}");
            ExitCode::from(1)
        }
    }
}
