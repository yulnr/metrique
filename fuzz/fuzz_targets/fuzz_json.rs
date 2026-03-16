#![no_main]

mod fuzz_entry;

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;

use metrique_writer_core::format::Format;
use metrique_writer_format_json::Json;

use fuzz_entry::FuzzEntry;

fuzz_target!(|data: &[u8]| {
    let Ok((entry_a, entry_b)) =
        Unstructured::new(data).arbitrary::<(FuzzEntry, FuzzEntry)>()
    else {
        return;
    };

    let mut format = Json::new();
    let mut output = Vec::new();

    // Format the entry, we don't care if it returns a validation error,
    // but it must never panic.
    let result = format.format(&entry_a, &mut output);

    if let Ok(()) = result {
        // Baseline invariant: successful formatting must produce structurally valid JSON.
        // We intentionally keep this target focused on framing validity; semantic checks can
        // be layered in later without weakening this guarantee.
        assert!(
            serde_json::from_slice::<serde_json::Value>(&output).is_ok(),
            "Formatter produced invalid JSON: {}",
            String::from_utf8_lossy(&output),
        );
    }

    // Format a different entry through the same formatter to test state reuse.
    output.clear();
    let result = format.format(&entry_b, &mut output);
    if let Ok(()) = result {
        assert!(
            serde_json::from_slice::<serde_json::Value>(&output).is_ok(),
            "Formatter produced invalid JSON after state reuse: {}",
            String::from_utf8_lossy(&output),
        );
    }
});
