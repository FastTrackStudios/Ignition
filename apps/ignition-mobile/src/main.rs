fn main() {
    // Telemetry before the event loop: the OTLP exporter is built once at
    // process start, so it must exist before the first span. Fully inert
    // unless the build baked an endpoint. Guards are leaked deliberately --
    // `dioxus::launch` never returns, so a Drop-based shutdown is dead code.
    {
        use tracing_subscriber::layer::SubscriberExt as _;
        use tracing_subscriber::util::SubscriberInitExt as _;

        if let Some(guard) = architect_telemetry::init("ignition") {
            std::mem::forget(guard);
        }
        let registry = tracing_subscriber::registry()
            .with(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info".into()),
            )
            .with(tracing_subscriber::fmt::layer())
            .with(architect_telemetry::tracing_layer());
        match architect_telemetry::otel::init("ignition") {
            Some((otel_guard, layers)) => {
                registry.with(layers).init();
                std::mem::forget(otel_guard);
            }
            None => registry.init(),
        }
    }

    dioxus::launch(ignition_mobile::App);
}
