//! Log capture and a leak assertion shared by every redaction test.

use std::io::Write;
use std::sync::{Arc, Mutex, PoisonError};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::format::FmtSpan;

/// Everything the current thread logs while the guard from [`LogCapture::install`] lives.
/// Tests using it should run in their own test binary: tracing's callsite-interest cache
/// is process-wide.
#[derive(Clone, Default)]
pub struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl LogCapture {
    /// Installs a thread-default subscriber at `TRACE` that writes into the capture. Span
    /// closes are logged too, so a span's fields are captured even when no event fires in it.
    #[must_use]
    pub fn install() -> (Self, tracing::subscriber::DefaultGuard) {
        let capture = Self::default();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_span_events(FmtSpan::CLOSE)
            .with_writer(capture.clone())
            .finish();
        (capture, tracing::subscriber::set_default(subscriber))
    }

    /// What was logged so far.
    #[must_use]
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap_or_else(PoisonError::into_inner)).into_owned()
    }
}

impl Write for LogCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl MakeWriter<'_> for LogCapture {
    type Writer = Self;
    fn make_writer(&self) -> Self::Writer {
        self.clone()
    }
}

/// Panics if any `(label, text)` output contains any secret, in clear or base64 form.
///
/// # Panics
///
/// When a leak is found; the message names the output and only the secret's first four
/// characters.
pub fn assert_no_leak(secrets: &[&str], outputs: &[(&str, &str)]) {
    for secret in secrets {
        let forms = [
            (*secret).to_owned(),
            STANDARD.encode(secret),
            URL_SAFE_NO_PAD.encode(secret),
        ];
        for (label, text) in outputs {
            for form in &forms {
                assert!(
                    !text.contains(form.as_str()),
                    "{label} leaks the secret starting {:?}",
                    secret.get(..4).unwrap_or_default()
                );
            }
        }
    }
}
