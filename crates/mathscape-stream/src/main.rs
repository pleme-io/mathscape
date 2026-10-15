use mathscape_stream::{EpochReport, Writer};
use std::process::ExitCode;
use std::time::Instant;
use tracing::{error, info};

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
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
    let mut writer = match Writer::open(stream) {
        Ok(w) => w,
        Err(e) => {
            error!("open: {e}");
            return ExitCode::from(1);
        }
    };
    match writer.resumed_from() {
        Some(h) => info!(head = %h.to_hex(), epoch = writer.epoch(), library = writer.library().len(), "resumed from HEAD"),
        None => info!(seed = writer.config().seed, "fresh stream from seed"),
    }
    let started = Instant::now();
    let mut certified = 0usize;
    let result = writer.run(|r: &EpochReport| {
        certified += r.certified;
        info!(
            epoch = r.epoch,
            candidates = r.candidates,
            certified = r.certified,
            rejected = r.rejected,
            library = r.library,
            snapshot = r.snapshot.map(|h| h.to_hex()).unwrap_or_default(),
            "epoch"
        );
    });
    let secs = started.elapsed().as_secs_f64();
    match result {
        Ok(head) => {
            let per_hour = if secs > 0.0 { certified as f64 * 3600.0 / secs } else { 0.0 };
            info!(
                head = head.map(|h| h.to_hex()).unwrap_or_default(),
                epochs = writer.epochs_run(),
                certified,
                seconds = secs,
                certified_per_hour = per_hour,
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
