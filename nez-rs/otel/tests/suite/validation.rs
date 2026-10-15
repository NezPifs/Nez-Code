use nez_otel::MetricsClient;
use nez_otel::MetricsConfig;
use nez_otel::MetricsError;
use nez_otel::Result;
use opentelemetry_sdk::metrics::InMemoryMetricExporter;

fn build_in_memory_client() -> Result<MetricsClient> {
    let exporter = InMemoryMetricExporter::default();
    let config = MetricsConfig::in_memory("test", "nez-cli", env!("CARGO_PKG_VERSION"), exporter);
    MetricsClient::new(config)
}

// Ensures invalid tag components are rejected during config build.
#[test]
fn invalid_tag_component_is_rejected() -> Result<()> {
    let err = MetricsConfig::in_memory(
        "test",
        "nez-cli",
        env!("CARGO_PKG_VERSION"),
        InMemoryMetricExporter::default(),
    )
    .with_tag("bad key", "value")
    .unwrap_err();
    assert!(matches!(
        err,
        MetricsError::InvalidTagComponent { label, value }
            if label == "tag key" && value == "bad key"
    ));
    Ok(())
}

// Ensures per-metric tag keys are validated.
#[test]
fn counter_rejects_invalid_tag_key() -> Result<()> {
    let metrics = build_in_memory_client()?;
    let err = metrics
        .counter("nez.turns", /*inc*/ 1, &[("bad key", "value")])
        .unwrap_err();
    assert!(matches!(
        err,
        MetricsError::InvalidTagComponent { label, value }
            if label == "tag key" && value == "bad key"
    ));
    metrics.shutdown()?;
    Ok(())
}

// Ensures per-metric tag values are validated.
#[test]
fn histogram_rejects_invalid_tag_value() -> Result<()> {
    let metrics = build_in_memory_client()?;
    let err = metrics
        .histogram(
            "nez.request_latency",
            /*value*/ 3,
            &[("route", "bad value")],
        )
        .unwrap_err();
    assert!(matches!(
        err,
        MetricsError::InvalidTagComponent { label, value }
            if label == "tag value" && value == "bad value"
    ));
    metrics.shutdown()?;
    Ok(())
}

// Ensures invalid metric names are rejected.
#[test]
fn counter_rejects_invalid_metric_name() -> Result<()> {
    let metrics = build_in_memory_client()?;
    let err = metrics.counter("bad name", /*inc*/ 1, &[]).unwrap_err();
    assert!(matches!(
        err,
        MetricsError::InvalidMetricName { name } if name == "bad name"
    ));
    metrics.shutdown()?;
    Ok(())
}

#[test]
fn counter_rejects_negative_increment() -> Result<()> {
    let metrics = build_in_memory_client()?;
    let err = metrics.counter("nez.turns", /*inc*/ -1, &[]).unwrap_err();
    assert!(matches!(
        err,
        MetricsError::NegativeCounterIncrement { name, inc } if name == "nez.turns" && inc == -1
    ));
    metrics.shutdown()?;
    Ok(())
}
