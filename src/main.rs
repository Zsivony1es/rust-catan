fn main() {
    let _guard = sentry::init(("https://547bc132aefdf46bec7eeff82cfa026e@o4512078782136320.ingest.de.sentry.io/4512078786723920", sentry::ClientOptions {
    release: sentry::release_name!(),
    // Capture user IPs and potentially sensitive headers when using HTTP server integrations
    // see https://docs.sentry.io/platforms/rust/data-management/data-collected for more info
    send_default_pii: true,
    ..Default::default()
    }));

    // Sentry will capture this
    panic!("Everything is on fire!");
}
