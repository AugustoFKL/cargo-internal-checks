use tracing_subscriber::{
    FmtSubscriber,
    fmt::{format::FmtSpan, time::ChronoLocal},
};

/// Creates and configures the logging context.
pub(crate) fn setup() -> anyhow::Result<()> {
    let crate_name = env!("CARGO_CRATE_NAME");

    let subscriber = FmtSubscriber::builder()
        .with_env_filter(format!("{crate_name}=info"))
        .with_timer(ChronoLocal::new("%Y-%m-%d %H:%M:%S".to_owned()))
        .with_span_events(FmtSpan::FULL)
        .with_target(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;
    Ok(())
}
