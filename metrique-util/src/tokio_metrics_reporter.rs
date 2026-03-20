// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::{fmt, time::Duration};

use metrique_writer_core::{BoxEntrySink, EntrySink};
use tokio::runtime::Handle;
use tokio::task::JoinHandle;
use tokio_metrics::RuntimeMonitor;

use crate::reporter::{CompositeAttachHandle, MetricReporter};

const DEFAULT_METRIC_SAMPLING_INTERVAL: Duration = Duration::from_secs(30);

/// Configuration for Tokio runtime metrics bridge subscriptions.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct TokioRuntimeMetricsConfig {
    /// Sampling interval used by the reporter loop.
    pub interval: Duration,
}

impl Default for TokioRuntimeMetricsConfig {
    fn default() -> Self {
        Self {
            interval: DEFAULT_METRIC_SAMPLING_INTERVAL,
        }
    }
}

impl TokioRuntimeMetricsConfig {
    /// Return a config with a custom sampling interval.
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }
}

/// Handle for a Tokio runtime metrics subscription.
///
/// Keep this alive while metrics should continue being collected and appended.
#[must_use = "if unused the reporter task will be aborted immediately"]
pub struct TokioRuntimeMetricsReporter {
    reporter_task: Option<JoinHandle<()>>,
}

impl fmt::Debug for TokioRuntimeMetricsReporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokioRuntimeMetricsReporter").finish()
    }
}

impl TokioRuntimeMetricsReporter {
    fn new(reporter_task: JoinHandle<()>) -> Self {
        Self {
            reporter_task: Some(reporter_task),
        }
    }
}

impl MetricReporter for TokioRuntimeMetricsReporter {
    fn abort(&mut self) {
        if let Some(task) = self.reporter_task.take() {
            task.abort();
        }
    }

    fn forget(&mut self) {
        let _ = self.reporter_task.take();
    }
}

impl Drop for TokioRuntimeMetricsReporter {
    fn drop(&mut self) {
        self.abort();
    }
}

/// Extension methods for subscribing Tokio runtime metrics reporting on a sink.
pub trait BoxEntrySinkTokioMetricsExt {
    /// Start runtime metrics reporting with custom configuration.
    fn subscribe_tokio_runtime_metrics(
        self,
        config: TokioRuntimeMetricsConfig,
    ) -> TokioRuntimeMetricsReporter;
}

impl BoxEntrySinkTokioMetricsExt for BoxEntrySink {
    fn subscribe_tokio_runtime_metrics(
        self,
        config: TokioRuntimeMetricsConfig,
    ) -> TokioRuntimeMetricsReporter {
        let interval = config.interval;
        let reporter_task = tokio::spawn(async move {
            tracing::debug!("tokio runtime metrics reporter started");
            let handle = Handle::current();
            let monitor = RuntimeMonitor::new(&handle);
            for snapshot in monitor.intervals() {
                // Take histogram counts before moving snapshot into append.
                // Bucket ranges come from the runtime handle at format time.
                #[cfg(tokio_unstable)]
                let (snapshot, histogram_counts) = {
                    let mut snapshot = snapshot;
                    let counts = std::mem::take(&mut snapshot.poll_time_histogram);
                    (snapshot, counts)
                };
                self.append(snapshot);
                #[cfg(tokio_unstable)]
                emit_poll_time_histogram(&self, histogram_counts, handle.metrics());
                tokio::time::sleep(interval).await;
            }
            tracing::debug!("tokio runtime metrics reporter stopped");
        });
        TokioRuntimeMetricsReporter::new(reporter_task)
    }
}

/// Emit `poll_time_histogram` bucket counts as a metrique distribution metric,
/// pairing each bucket's count with its range from the runtime handle.
#[cfg(tokio_unstable)]
fn emit_poll_time_histogram(
    sink: &BoxEntrySink,
    counts: Vec<u64>,
    rt: tokio::runtime::RuntimeMetrics,
) {
    use metrique_writer_core::value::MetricFlags;
    use metrique_writer_core::{Entry, EntryWriter, Observation, Unit, unit::NegativeScale};

    // Prototype note: Emitted as a separate entry alongside RuntimeMetrics because
    // `poll_time_histogram` uses #[entry(ignore)] on RuntimeMetrics — the raw
    // Vec<u64> counts need bucket ranges from the runtime handle to be
    // meaningful, which Entry::write() doesn't have access to.
    //
    // If a single entry is preferred, this could be wrapped into a struct
    // that flattens RuntimeMetrics and adds the enriched histogram field.
    struct PollTimeHistogramEntry {
        counts: Vec<u64>,
        rt: tokio::runtime::RuntimeMetrics,
    }

    impl Entry for PollTimeHistogramEntry {
        fn write<'a>(&'a self, writer: &mut impl EntryWriter<'a>) {
            writer.value("poll_time_histogram", self);
        }
    }

    impl metrique_writer_core::Value for PollTimeHistogramEntry {
        fn write(&self, writer: impl metrique_writer_core::ValueWriter) {
            writer.metric(
                self.counts
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| **c > 0)
                    .map(|(i, &count)| {
                        let range = self.rt.poll_time_histogram_bucket_range(i);
                        let midpoint_us =
                            (range.start.as_micros() + range.end.as_micros()) as f64 / 2.0;
                        Observation::Repeated {
                            total: midpoint_us * count as f64,
                            occurrences: count,
                        }
                    }),
                Unit::Second(NegativeScale::Micro),
                [],
                MetricFlags::empty(),
            );
        }
    }

    sink.append(PollTimeHistogramEntry { counts, rt });
}

/// Extension methods for composing Tokio runtime metrics with a [`CompositeAttachHandle`].
pub trait CompositeAttachHandleTokioMetricsExt {
    /// Subscribe to Tokio runtime metrics, adding the subscription to this handle.
    ///
    /// # Panics
    /// Panics if the underlying sink has been detached (e.g. the `AttachHandle` was
    /// dropped elsewhere before this call).
    fn with_tokio_runtime_metrics(self, config: TokioRuntimeMetricsConfig) -> Self;
}

impl CompositeAttachHandleTokioMetricsExt for CompositeAttachHandle {
    fn with_tokio_runtime_metrics(mut self, config: TokioRuntimeMetricsConfig) -> Self {
        let sink = self
            .try_sink()
            .expect("sink must still be attached before subscribing tokio runtime metrics");
        let sub = sink.subscribe_tokio_runtime_metrics(config);
        self.add_reporter(sub);
        self
    }
}
