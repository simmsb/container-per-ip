#[macro_export]
macro_rules! trace_error {
    ($e:expr) => {
        if let Err(err) = $e {
            ::tracing::error!(?err, "{}", concat!("Failed to ", stringify!($e)));
        }
    };
}
