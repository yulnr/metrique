//! Shared fuzz entry types used by all formatter fuzz targets.

use std::borrow::Cow;
use std::time::{Duration, SystemTime};

use arbitrary::{Arbitrary, Unstructured};

use metrique_writer_core::{
    Entry, EntryWriter, MetricFlags, Observation, Unit, ValueWriter,
    config::{AllowSplitEntries, EntryDimensions},
    unit::{NegativeScale, PositiveScale},
};
use metrique_writer_format_emf::{HighStorageResolution, NoMetric};

const OMIT_ENTRY_DIMENSIONS_PERCENT: u8 = 55;
const REUSE_EXISTING_NAME_PERCENT: u8 = 70;
const REPLACE_EXISTING_POOL_NAME_PERCENT: u8 = 20;

/// A single field in our fuzzed entry.
#[derive(Debug)]
pub enum FuzzField {
    /// A string property like `writer.value("key", &"some string")`
    StringProperty { name: String, value: String },
    /// A metric with one or more observations
    Metric {
        name: String,
        observations: Vec<FuzzObservation>,
        dimensions: Vec<(String, String)>,
        unit: FuzzUnit,
        flag_mode: FuzzMetricFlagMode,
    },
}

#[derive(Debug)]
pub enum FuzzObservation {
    Unsigned(u64),
    Floating(f64),
    Repeated { total: f64, occurrences: u64 },
}

impl FuzzObservation {
    fn to_observation(&self) -> Observation {
        match *self {
            FuzzObservation::Unsigned(v) => Observation::Unsigned(v),
            FuzzObservation::Floating(v) => Observation::Floating(v),
            FuzzObservation::Repeated { total, occurrences } => {
                Observation::Repeated { total, occurrences }
            }
        }
    }
}

impl<'a> Arbitrary<'a> for FuzzObservation {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let tag: u8 = u.arbitrary()?;
        match tag % 4 {
            0 => Ok(FuzzObservation::Unsigned(u.arbitrary()?)),
            1 => Ok(FuzzObservation::Floating(arbitrary_f64(u)?)),
            2 => Ok(FuzzObservation::Repeated {
                total: arbitrary_f64(u)?,
                occurrences: u.arbitrary()?,
            }),
            _ => Ok(FuzzObservation::Repeated {
                total: arbitrary_f64(u)?,
                // Keep this edge case frequent: repeated with 0 count.
                occurrences: if u.arbitrary::<bool>()? {
                    0
                } else {
                    u.arbitrary()?
                },
            }),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum FuzzUnit {
    None,
    Count,
    Percent,
    SecondMicro,
    SecondMilli,
    Second,
    Byte,
    KiloByte,
    MegaByte,
    Bit,
    KiloBit,
}

impl FuzzUnit {
    fn to_unit(&self) -> Unit {
        match self {
            FuzzUnit::None => Unit::None,
            FuzzUnit::Count => Unit::Count,
            FuzzUnit::Percent => Unit::Percent,
            FuzzUnit::SecondMicro => Unit::Second(NegativeScale::Micro),
            FuzzUnit::SecondMilli => Unit::Second(NegativeScale::Milli),
            FuzzUnit::Second => Unit::Second(NegativeScale::One),
            FuzzUnit::Byte => Unit::Byte(PositiveScale::One),
            FuzzUnit::KiloByte => Unit::Byte(PositiveScale::Kilo),
            FuzzUnit::MegaByte => Unit::Byte(PositiveScale::Mega),
            FuzzUnit::Bit => Unit::Bit(PositiveScale::One),
            FuzzUnit::KiloBit => Unit::Bit(PositiveScale::Kilo),
        }
    }
}

impl<'a> Arbitrary<'a> for FuzzUnit {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let tag: u8 = u.arbitrary()?;
        Ok(match tag % 11 {
            0 => FuzzUnit::None,
            1 => FuzzUnit::Count,
            2 => FuzzUnit::Percent,
            3 => FuzzUnit::SecondMicro,
            4 => FuzzUnit::SecondMilli,
            5 => FuzzUnit::Second,
            6 => FuzzUnit::Byte,
            7 => FuzzUnit::KiloByte,
            8 => FuzzUnit::MegaByte,
            9 => FuzzUnit::Bit,
            _ => FuzzUnit::KiloBit,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub enum FuzzMetricFlagMode {
    None,
    HighStorageResolution,
    NoMetric,
    HighThenNoMetric,
    NoMetricThenHigh,
}

impl<'a> Arbitrary<'a> for FuzzMetricFlagMode {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let tag: u8 = u.arbitrary()?;
        Ok(match tag % 5 {
            0 => FuzzMetricFlagMode::None,
            1 => FuzzMetricFlagMode::HighStorageResolution,
            2 => FuzzMetricFlagMode::NoMetric,
            3 => FuzzMetricFlagMode::HighThenNoMetric,
            _ => FuzzMetricFlagMode::NoMetricThenHigh,
        })
    }
}

#[derive(Debug)]
pub struct FuzzTimestamp {
    pub before_epoch: bool,
    pub secs: u64,
}

impl FuzzTimestamp {
    fn to_system_time(&self) -> SystemTime {
        // Keep values bounded to avoid pathological durations.
        let secs = self.secs % (365 * 500 * 24 * 3600);
        let duration = Duration::from_secs(secs);
        if self.before_epoch {
            SystemTime::UNIX_EPOCH
                .checked_sub(duration)
                .unwrap_or(SystemTime::UNIX_EPOCH)
        } else {
            SystemTime::UNIX_EPOCH + duration
        }
    }
}

impl<'a> Arbitrary<'a> for FuzzTimestamp {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self {
            before_epoch: u.arbitrary()?,
            secs: u.arbitrary()?,
        })
    }
}

/// Fuzzed entry that exercises the full `EntryWriter` interface.
#[derive(Debug)]
pub struct FuzzEntry {
    pub timestamps: Vec<FuzzTimestamp>,
    pub allow_split_entries: bool,
    pub entry_dimensions: Option<EntryDimensions>,
    pub fields: Vec<FuzzField>,
}

impl Entry for FuzzEntry {
    fn write<'a>(&'a self, writer: &mut impl EntryWriter<'a>) {
        if self.allow_split_entries {
            writer.config(&const { AllowSplitEntries::new() });
        }
        if let Some(entry_dimensions) = &self.entry_dimensions {
            writer.config(entry_dimensions);
        }
        for timestamp in &self.timestamps {
            writer.timestamp(timestamp.to_system_time());
        }
        for field in &self.fields {
            match field {
                FuzzField::StringProperty { name, value } => {
                    writer.value(name.as_str(), &value.as_str());
                }
                FuzzField::Metric {
                    name,
                    observations,
                    dimensions,
                    unit,
                    flag_mode,
                } => {
                    let metric = FuzzMetricValue {
                        observations,
                        dimensions,
                        unit: *unit,
                    };
                    match flag_mode {
                        FuzzMetricFlagMode::None => writer.value(name.as_str(), &metric),
                        FuzzMetricFlagMode::HighStorageResolution => {
                            writer.value(name.as_str(), &HighStorageResolution::from(metric));
                        }
                        FuzzMetricFlagMode::NoMetric => {
                            writer.value(name.as_str(), &NoMetric::from(metric));
                        }
                        FuzzMetricFlagMode::HighThenNoMetric => {
                            writer.value(
                                name.as_str(),
                                &NoMetric::from(HighStorageResolution::from(metric)),
                            );
                        }
                        FuzzMetricFlagMode::NoMetricThenHigh => {
                            writer.value(
                                name.as_str(),
                                &HighStorageResolution::from(NoMetric::from(metric)),
                            );
                        }
                    }
                }
            }
        }
    }
}

struct FuzzMetricValue<'a> {
    observations: &'a [FuzzObservation],
    dimensions: &'a [(String, String)],
    unit: FuzzUnit,
}

impl metrique_writer_core::value::Value for FuzzMetricValue<'_> {
    fn write(&self, writer: impl ValueWriter) {
        writer.metric(
            self.observations
                .iter()
                .map(FuzzObservation::to_observation),
            self.unit.to_unit(),
            self.dimensions
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
            MetricFlags::empty(),
        );
    }
}

impl<'a> Arbitrary<'a> for FuzzEntry {
    fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
        let timestamp_count = match u.arbitrary::<u8>()? % 8 {
            0 => 0,
            1 | 2 | 3 | 4 => 1,
            5 => 2,
            6 => 3,
            _ => 4,
        };
        let timestamps: Vec<FuzzTimestamp> = (0..timestamp_count)
            .map(|_| u.arbitrary())
            .collect::<Result<_, _>>()?;
        let allow_split_entries: bool = u.arbitrary()?;

        let entry_dimensions = if chance_percent(u, OMIT_ENTRY_DIMENSIONS_PERCENT)? {
            None
        } else {
            Some(arbitrary_entry_dimensions(u)?)
        };

        // 0-24 fields per entry, with occasional larger cases.
        let field_count = match u.arbitrary::<u8>()? % 8 {
            0 => 0,
            1 => 1,
            2 => 2,
            3 => 4,
            4 => 8,
            5 => 12,
            6 => 16,
            _ => 24,
        };

        let mut name_pool = Vec::new();
        let fields: Vec<FuzzField> = (0..field_count)
            .map(|_| arbitrary_field(u, &mut name_pool))
            .collect::<Result<_, _>>()?;

        Ok(FuzzEntry {
            timestamps,
            allow_split_entries,
            entry_dimensions,
            fields,
        })
    }
}

fn arbitrary_field<'a>(
    u: &mut Unstructured<'a>,
    name_pool: &mut Vec<String>,
) -> arbitrary::Result<FuzzField> {
    let is_string: bool = u.arbitrary()?;
    let reuse_name = !name_pool.is_empty() && chance_percent(u, REUSE_EXISTING_NAME_PERCENT)?;
    let name = if reuse_name {
        let idx = choose_index(u, name_pool.len())?;
        name_pool[idx].clone()
    } else {
        let name = arbitrary_string(u, 96)?;
        if !name_pool.is_empty() && chance_percent(u, REPLACE_EXISTING_POOL_NAME_PERCENT)? {
            // Keep pool bounded and still churn names.
            let idx = choose_index(u, name_pool.len())?;
            name_pool[idx] = name.clone();
        } else if name_pool.len() < 16 {
            name_pool.push(name.clone());
        }
        name
    };

    if is_string {
        let value = arbitrary_string(u, 192)?;
        Ok(FuzzField::StringProperty { name, value })
    } else {
        let obs_count = match u.arbitrary::<u8>()? % 8 {
            0 => 0,
            1 => 1,
            2 => 2,
            3 => 3,
            4 => 4,
            5 => 8,
            6 => 16,
            _ => 24,
        };
        let observations: Vec<FuzzObservation> = (0..obs_count)
            .map(|_| u.arbitrary())
            .collect::<Result<_, _>>()?;

        let dim_count = match u.arbitrary::<u8>()? % 6 {
            0 => 0,
            1 => 1,
            2 => 2,
            3 => 3,
            4 => 6,
            _ => 12,
        };
        let dimensions: Vec<(String, String)> = (0..dim_count)
            .map(|_| Ok((arbitrary_string(u, 48)?, arbitrary_string(u, 64)?)))
            .collect::<Result<_, _>>()?;

        Ok(FuzzField::Metric {
            name,
            observations,
            dimensions,
            unit: u.arbitrary()?,
            flag_mode: u.arbitrary()?,
        })
    }
}

fn arbitrary_entry_dimensions<'a>(u: &mut Unstructured<'a>) -> arbitrary::Result<EntryDimensions> {
    let set_count = match u.arbitrary::<u8>()? % 6 {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 4,
        _ => 6,
    };
    let mut sets: Vec<Cow<'static, [Cow<'static, str>]>> = Vec::with_capacity(set_count);
    for _ in 0..set_count {
        let dim_count = match u.arbitrary::<u8>()? % 6 {
            0 => 0,
            1 => 1,
            2 => 2,
            3 => 3,
            4 => 4,
            _ => 6,
        };
        let mut dims: Vec<Cow<'static, str>> = Vec::with_capacity(dim_count);
        for _ in 0..dim_count {
            dims.push(Cow::Owned(arbitrary_string(u, 48)?));
        }
        sets.push(Cow::Owned(dims));
    }
    Ok(EntryDimensions::new(Cow::Owned(sets)))
}

fn arbitrary_string<'a>(u: &mut Unstructured<'a>, max_len: usize) -> arbitrary::Result<String> {
    let len = (u.arbitrary::<u8>()? as usize).min(max_len);
    let mut s = String::with_capacity(len);
    for _ in 0..len {
        s.push(arbitrary_char(u)?);
    }
    Ok(s)
}

fn arbitrary_char<'a>(u: &mut Unstructured<'a>) -> arbitrary::Result<char> {
    const JSON_ESCAPES: [char; 6] = ['"', '\\', '\n', '\r', '\t', '\u{08}'];
    const DELIMS: [char; 6] = ['{', '}', '[', ']', ':', ','];
    let bucket: u8 = u.arbitrary()?;
    match bucket % 10 {
        0 => {
            let idx = (u.arbitrary::<u8>()? as usize) % JSON_ESCAPES.len();
            Ok(JSON_ESCAPES[idx])
        }
        1 => {
            let control = u.int_in_range(0..=0x1f)?;
            Ok(char::from(control))
        }
        2 => {
            let idx = (u.arbitrary::<u8>()? as usize) % DELIMS.len();
            Ok(DELIMS[idx])
        }
        3 => Ok(if u.arbitrary::<bool>()? { ' ' } else { '\n' }),
        _ => u.arbitrary::<char>(),
    }
}

fn arbitrary_f64<'a>(u: &mut Unstructured<'a>) -> arbitrary::Result<f64> {
    let choice: u8 = u.arbitrary()?;
    Ok(match choice % 12 {
        0 => f64::NAN,
        1 => f64::INFINITY,
        2 => f64::NEG_INFINITY,
        3 => -0.0,
        4 => 0.0,
        5 => f64::MAX,
        6 => f64::MIN,
        7 => f64::MIN_POSITIVE,
        8 => f64::from_bits(1),
        _ => f64::from_bits(u.arbitrary()?),
    })
}

fn chance_percent<'a>(u: &mut Unstructured<'a>, percent: u8) -> arbitrary::Result<bool> {
    debug_assert!(percent <= 100);
    if percent == 0 {
        return Ok(false);
    }
    if percent == 100 {
        return Ok(true);
    }
    let roll: u8 = u.int_in_range(0..=99)?;
    Ok(roll < percent)
}

fn choose_index<'a>(u: &mut Unstructured<'a>, len: usize) -> arbitrary::Result<usize> {
    debug_assert!(len > 0);
    u.int_in_range(0..=len - 1)
}
