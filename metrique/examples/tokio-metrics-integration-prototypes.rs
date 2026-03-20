// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use metrique::ServiceMetrics;
use metrique::emf::Emf;
use metrique::writer::{AttachGlobalEntrySinkExt, FormatExt, GlobalEntrySink};
use metrique_util::reporter::CompositeAttachHandle;
use metrique_util::{
    BoxEntrySinkTokioMetricsExt, CompositeAttachHandleTokioMetricsExt, TokioRuntimeMetricsConfig,
    TokioRuntimeMetricsReporter,
};
use metrique_writer::sink::AttachHandle;

const SAMPLING_INTERVAL: Duration = Duration::from_millis(500);

fn create_emf() -> Emf {
    Emf::all_validations("TokioMetricsPrototype".to_string(), vec![vec![]])
}

/// Approach A: Subscribe directly from a sink clone.
///
/// No CompositeAttachHandle — works directly with the global sink via
/// `ServiceMetrics::sink()`. The caller gets a sink clone and passes it to
/// the reporter, producing two independent handles.
///
/// Pros: Can drop a subscription without dropping the sink.
/// Cons: Two handles to manage at call site.
fn approach_a_sink_extension() -> (TokioRuntimeMetricsReporter, AttachHandle) {
    let attach_handle = ServiceMetrics::attach_to_stream(
        create_emf().output_to_makewriter(|| std::io::stdout().lock()),
    );

    // Clones the sink Arc and spawns a background task that appends runtime
    // metrics snapshots at the configured interval.
    let reporter = ServiceMetrics::sink().subscribe_tokio_runtime_metrics(
        TokioRuntimeMetricsConfig::default().with_interval(SAMPLING_INTERVAL),
    );

    // Both must stay alive:
    // - dropping `reporter` aborts the background metrics task
    // - dropping `attach_handle` detaches and flushes the global sink
    //
    // Either handle supports `.forget()` to keep it alive indefinitely
    (reporter, attach_handle)
}

/// Approach B: Composable `CompositeAttachHandle` wrapping attach handle + reporters.
///
/// Uses the `WeakEntrySink` on `AttachHandle` to get a sink clone.
///
/// Single handle manages both the sink and all metric reporters. Additional
/// reporters (e.g. sysinfo) compose via extension traits or `.add_reporter()`
/// (though if we aim for .add_reporter() as user-facing API, we might want to rename it to something like add_subscription()).
///
/// Pros: Single handle, composable.
/// Cons(?): Wrapper type.
#[allow(dead_code)]
fn approach_b_composite_attach_handle() -> CompositeAttachHandle {
    let attach_handle = ServiceMetrics::attach_to_stream(
        create_emf().output_to_makewriter(|| std::io::stdout().lock()),
    );

    CompositeAttachHandle::new(attach_handle).with_tokio_runtime_metrics(
        TokioRuntimeMetricsConfig::default().with_interval(SAMPLING_INTERVAL),
    )
    // Future reporters compose the same way:
    // .with_sysinfo(sysinfo_config)

    // Dropping the returned handle aborts all reporters and detaches the sink.
    // Calling `.forget()` on it keeps everything alive indefinitely.
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();

    // Comment out one of these approches to try the other one. 
    
    // Approach A:
    // Two handles: reporter + attach_handle. Both must stay alive.
    let (_reporter, _attach_handle) = approach_a_sink_extension();
    wait_for_demo_workload().await;

    // Approach B:
    // Single handle
    let _composite = approach_b_composite_attach_handle();
    wait_for_demo_workload().await;

    Ok(())
}

async fn wait_for_demo_workload() {
    tokio::join![do_work(), do_work(), do_work(),];
    tokio::time::sleep(Duration::from_millis(1200)).await;
}

async fn do_work() {
    for _ in 0..25 {
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
