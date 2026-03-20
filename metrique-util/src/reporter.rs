// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Composable attach handle that manages an [`AttachHandle`] alongside
//! background [`MetricReporter`]s (e.g. tokio runtime metrics, sysinfo).

use metrique_writer_core::BoxEntrySink;
use metrique_writer_core::global::AttachHandle;

/// Trait for background metric reporter lifecycle management.
///
/// Implementations control a background task that periodically collects
/// metrics from some source and appends them to a sink.
pub trait MetricReporter: Send {
    /// Abort the background task.
    fn abort(&mut self);

    /// Detach the reporter so it runs indefinitely.
    fn forget(&mut self);
}

/// Composable handle that wraps an [`AttachHandle`] and zero or more [`MetricReporter`]s.
///
/// Dropping this handle drops the attach handle and aborts all reporters.
/// Call [`.forget()`](Self::forget) to keep everything alive indefinitely.
///
/// # Example
///
/// ```rust,ignore
/// use metrique_util::reporter::CompositeAttachHandle;
///
/// let handle = CompositeAttachHandle::new(attach_handle)
///     .with_tokio_runtime_metrics(config);
/// ```
#[must_use = "if dropped, the sink and all reporters will be shut down"]
pub struct CompositeAttachHandle {
    attach_handle: Option<AttachHandle>,
    reporters: Vec<Box<dyn MetricReporter>>,
}

impl CompositeAttachHandle {
    /// Wrap an [`AttachHandle`] into a composite handle.
    pub fn new(attach_handle: AttachHandle) -> Self {
        Self {
            attach_handle: Some(attach_handle),
            reporters: Vec::new(),
        }
    }

    /// Add a metric reporter to this handle.
    pub fn add_reporter(&mut self, reporter: impl MetricReporter + 'static) {
        self.reporters.push(Box::new(reporter));
    }

    /// Keep the attach handle and all reporters alive indefinitely.
    ///
    /// After calling this, coordinated shutdown is no longer possible via this handle.
    pub fn forget(mut self) {
        if let Some(attach_handle) = self.attach_handle.take() {
            attach_handle.forget();
        }
        for mut reporter in self.reporters.drain(..) {
            reporter.forget();
        }
    }

    /// Return a clone of the attached sink, if still attached.
    ///
    /// Delegates to [`AttachHandle::try_sink`].
    pub fn try_sink(&self) -> Option<BoxEntrySink> {
        self.attach_handle.as_ref().and_then(|h| h.try_sink())
    }
}

impl Drop for CompositeAttachHandle {
    fn drop(&mut self) {
        // Abort reporters first, then drop the attach handle.
        for reporter in &mut self.reporters {
            reporter.abort();
        }
        // AttachHandle::drop will detach the sink.
    }
}
