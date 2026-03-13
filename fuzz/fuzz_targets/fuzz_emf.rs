#![no_main]

mod fuzz_entry;

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;

use metrique_writer_core::format::Format;
use metrique_writer_core::sample::SampledFormat;
use metrique_writer_format_emf::Emf;

use fuzz_entry::FuzzEntry;

/// EMF can produce multiple newline-delimited JSON documents (split entries).
fn assert_valid_json_lines(output: &[u8], context: &str) {
    let mut saw_document = false;
    for line in output.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        saw_document = true;
        let parsed = serde_json::from_slice::<serde_json::Value>(line).unwrap_or_else(|_| {
            panic!(
                "EMF produced invalid JSON ({context}): {}",
                String::from_utf8_lossy(line),
            )
        });
        assert!(
            parsed.is_object(),
            "EMF produced non-object JSON ({context}): {}",
            String::from_utf8_lossy(line),
        );
    }
    assert!(
        saw_document,
        "EMF returned success but emitted no JSON documents ({context})",
    );
}

#[derive(Debug)]
struct FuzzEmfConfig {
    namespace: String,
    default_dimensions: Vec<Vec<String>>,
    extra_namespace: Option<String>,
    log_group_name: Option<String>,
    allow_ignored_dimensions: bool,
    sample_rate_a: f32,
    sample_rate_b: f32,
}

impl<'a> arbitrary::Arbitrary<'a> for FuzzEmfConfig {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let namespace = arbitrary_string(u, 48)?;
        let extra_namespace = if u.arbitrary::<bool>()? {
            Some(arbitrary_string(u, 48)?)
        } else {
            None
        };
        let log_group_name = if u.arbitrary::<bool>()? {
            Some(arbitrary_string(u, 64)?)
        } else {
            None
        };

        // Keep at least one default dimension set to match common EMF setup.
        let set_count = (u.arbitrary::<u8>()? % 4) + 1;
        let mut default_dimensions = Vec::with_capacity(set_count as usize);
        for _ in 0..set_count {
            let dim_count = u.arbitrary::<u8>()? % 5;
            let mut dims = Vec::with_capacity(dim_count as usize);
            for _ in 0..dim_count {
                dims.push(arbitrary_string(u, 32)?);
            }
            default_dimensions.push(dims);
        }

        Ok(Self {
            namespace,
            default_dimensions,
            extra_namespace,
            log_group_name,
            allow_ignored_dimensions: u.arbitrary()?,
            sample_rate_a: arbitrary_sample_rate(u)?,
            sample_rate_b: arbitrary_sample_rate(u)?,
        })
    }
}

fn arbitrary_string<'a>(u: &mut Unstructured<'a>, max_len: usize) -> arbitrary::Result<String> {
    let len = (u.arbitrary::<u8>()? as usize).min(max_len);
    let mut s = String::with_capacity(len);
    for _ in 0..len {
        s.push(u.arbitrary::<char>()?);
    }
    Ok(s)
}

fn arbitrary_sample_rate<'a>(u: &mut Unstructured<'a>) -> arbitrary::Result<f32> {
    let selector: u8 = u.arbitrary()?;
    Ok(match selector % 10 {
        0 => f32::NAN,
        1 => 0.0,
        2 => -1.0,
        3 => f32::INFINITY,
        4 => 1.0,
        5 => 0.5,
        6 => 0.001,
        7 => 1e-30,
        _ => f32::from_bits(u.arbitrary()?),
    })
}

fn build_emf(config: &FuzzEmfConfig) -> Emf {
    let mut builder = Emf::builder(config.namespace.clone(), config.default_dimensions.clone())
        .allow_ignored_dimensions(config.allow_ignored_dimensions);
    if let Some(extra) = &config.extra_namespace {
        builder = builder.add_namespace(extra.clone());
    }
    if let Some(log_group_name) = &config.log_group_name {
        builder = builder.log_group_name(log_group_name.clone());
    }
    builder.build()
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Ok((entry_a, entry_b, config)) = u.arbitrary::<(FuzzEntry, FuzzEntry, FuzzEmfConfig)>()
    else {
        return;
    };

    // Regular EMF path.
    // Baseline invariant only: successful formatting must emit parseable JSON objects.
    // Deeper semantic invariants can be added in future, but this structural guard should
    // always remain.
    let mut format = build_emf(&config);
    let mut output = Vec::new();

    let result = format.format(&entry_a, &mut output);

    if let Ok(()) = result {
        assert_valid_json_lines(&output, "first call");
    }

    // Test formatter state reuse with a different entry.
    output.clear();
    let result = format.format(&entry_b, &mut output);
    if let Ok(()) = result {
        assert_valid_json_lines(&output, "state reuse call");
    }

    // Sampled EMF path.
    let mut sampled = build_emf(&config).with_sampling();
    output.clear();
    let result = sampled.format_with_sample_rate(&entry_a, &mut output, config.sample_rate_a);
    if let Ok(()) = result {
        assert_valid_json_lines(&output, "sampled first call");
    }
    output.clear();
    let result = sampled.format_with_sample_rate(&entry_b, &mut output, config.sample_rate_b);
    if let Ok(()) = result {
        assert_valid_json_lines(&output, "sampled state reuse call");
    }
});
