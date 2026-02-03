/// Errors for date/time conversions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateTimeError {
    /// Value exceeds the representable range.
    OutOfRange,
    /// Timezone/DST metadata is not supported.
    TimezoneNotSupported,
}

/// Duration with nanosecond precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Duration {
    nanos: i64,
}

impl Duration {
    pub const ZERO: Self = Self { nanos: 0 };

    #[must_use]
    pub fn from_nanos(nanos: i64) -> Self {
        Self { nanos }
    }

    #[must_use]
    pub fn from_micros(micros: i64) -> Self {
        Self {
            nanos: micros * 1_000,
        }
    }

    #[must_use]
    pub fn from_millis(millis: i64) -> Self {
        Self {
            nanos: millis * 1_000_000,
        }
    }

    #[must_use]
    pub fn from_secs(secs: i64) -> Self {
        Self {
            nanos: secs * 1_000_000_000,
        }
    }

    #[must_use]
    pub fn as_nanos(self) -> i64 {
        self.nanos
    }

    #[must_use]
    pub fn as_millis(self) -> i64 {
        self.nanos / 1_000_000
    }
}

/// Implementer-specific profile for TIME/DATE/TOD/DT.
#[derive(Debug, Clone, Copy)]
pub struct DateTimeProfile {
    /// Epoch for DATE/DT (default: 1970-01-01).
    pub epoch: DateValue,
    /// Resolution for TIME/DATE/TOD/DT (default: 1 ms).
    pub resolution: Duration,
}

impl Default for DateTimeProfile {
    fn default() -> Self {
        Self {
            epoch: DateValue { ticks: 0 },
            resolution: Duration::from_millis(1),
        }
    }
}

/// DATE value stored as ticks since epoch at midnight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DateValue {
    ticks: i64,
}

impl DateValue {
    #[must_use]
    pub fn new(ticks: i64) -> Self {
        Self { ticks }
    }

    #[must_use]
    pub fn ticks(self) -> i64 {
        self.ticks
    }
}

/// TIME_OF_DAY value stored as ticks since midnight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimeOfDayValue {
    ticks: i64,
}

impl TimeOfDayValue {
    #[must_use]
    pub fn new(ticks: i64) -> Self {
        Self { ticks }
    }

    #[must_use]
    pub fn ticks(self) -> i64 {
        self.ticks
    }
}

/// DATE_AND_TIME value stored as ticks since epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DateTimeValue {
    ticks: i64,
}

impl DateTimeValue {
    #[must_use]
    pub fn new(ticks: i64) -> Self {
        Self { ticks }
    }

    #[must_use]
    pub fn ticks(self) -> i64 {
        self.ticks
    }
}

/// Long DATE stored as nanoseconds since epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LDateValue {
    nanos: i64,
}

impl LDateValue {
    #[must_use]
    pub fn new(nanos: i64) -> Self {
        Self { nanos }
    }

    #[must_use]
    pub fn nanos(self) -> i64 {
        self.nanos
    }
}

/// Long TIME_OF_DAY stored as nanoseconds since midnight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LTimeOfDayValue {
    nanos: i64,
}

impl LTimeOfDayValue {
    #[must_use]
    pub fn new(nanos: i64) -> Self {
        Self { nanos }
    }

    #[must_use]
    pub fn nanos(self) -> i64 {
        self.nanos
    }
}

/// Long DATE_AND_TIME stored as nanoseconds since epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LDateTimeValue {
    nanos: i64,
}

impl LDateTimeValue {
    #[must_use]
    pub fn new(nanos: i64) -> Self {
        Self { nanos }
    }

    #[must_use]
    pub fn nanos(self) -> i64 {
        self.nanos
    }
}

impl DateValue {
    pub fn try_from_ticks(ticks: i128) -> Result<Self, DateTimeError> {
        let ticks = i64::try_from(ticks).map_err(|_| DateTimeError::OutOfRange)?;
        Ok(Self { ticks })
    }
}

impl TimeOfDayValue {
    pub fn try_from_ticks(ticks: i128) -> Result<Self, DateTimeError> {
        let ticks = i64::try_from(ticks).map_err(|_| DateTimeError::OutOfRange)?;
        Ok(Self { ticks })
    }
}

impl DateTimeValue {
    pub fn try_from_ticks(ticks: i128) -> Result<Self, DateTimeError> {
        let ticks = i64::try_from(ticks).map_err(|_| DateTimeError::OutOfRange)?;
        Ok(Self { ticks })
    }
}

/// Combine DATE and TOD into DT, rejecting timezone metadata.
pub fn combine_date_and_tod(
    date: DateValue,
    tod: TimeOfDayValue,
) -> Result<DateTimeValue, DateTimeError> {
    DateTimeValue::try_from_ticks(i128::from(date.ticks) + i128::from(tod.ticks))
}

/// Combine DATE and TOD into DT with an optional timezone offset.
pub fn combine_date_and_tod_with_tz(
    date: DateValue,
    tod: TimeOfDayValue,
    tz_offset_minutes: Option<i32>,
) -> Result<DateTimeValue, DateTimeError> {
    if tz_offset_minutes.is_some() {
        return Err(DateTimeError::TimezoneNotSupported);
    }
    combine_date_and_tod(date, tod)
}
