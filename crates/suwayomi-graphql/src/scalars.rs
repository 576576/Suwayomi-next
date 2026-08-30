//! Custom scalars — mirror `graphql/server/primitives/`:
//! `LongString` (Long→String, JS precision), `Duration` (ISO-8601), `Cursor`.

use async_graphql::{InputValueError, InputValueResult, Scalar, ScalarType, Value};

/// Long encoded as String (JS-safe). Matches Kotlin `LongAsString`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LongString(pub i64);

#[Scalar(name = "LongString")]
impl ScalarType for LongString {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => s
                .parse::<i64>()
                .map(LongString)
                .map_err(|e| InputValueError::custom(format!("invalid LongString: {e}"))),
            Value::Number(n) => n
                .as_i64()
                .map(LongString)
                .ok_or_else(|| InputValueError::custom("expected i64 number")),
            _ => Err(InputValueError::custom("expected string")),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.to_string())
    }
}

/// ISO-8601 duration string. Matches Kotlin `DurationAsString`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DurationScalar(pub std::time::Duration);

#[Scalar(name = "Duration")]
impl ScalarType for DurationScalar {
    fn parse(value: Value) -> InputValueResult<Self> {
        let s = match value {
            Value::String(s) => s,
            _ => return Err(InputValueError::custom("expected string")),
        };
        parse_iso8601_duration(&s)
            .map(DurationScalar)
            .ok_or_else(|| InputValueError::custom(format!("invalid ISO-8601 duration: {s}")))
    }

    fn to_value(&self) -> Value {
        Value::String(format_iso8601_duration(self.0))
    }
}

/// Opaque pagination cursor — mirrors Kotlin `Cursor`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Cursor(pub String);

#[Scalar(name = "Cursor")]
impl ScalarType for Cursor {
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => Ok(Cursor(s)),
            _ => Err(InputValueError::custom("expected string")),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.clone())
    }
}

/// `Duration` (ISO-8601, e.g. PT1H30M) parsing — mirrors Kotlin
/// `java.time.Duration.parse` output consumed by clients.
pub fn format_iso8601_duration(d: std::time::Duration) -> String {
    let total = d.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    let mut out = "PT".to_string();
    if hours > 0 {
        out.push_str(&format!("{hours}H"));
    }
    if minutes > 0 {
        out.push_str(&format!("{minutes}M"));
    }
    if seconds > 0 || out == "PT" {
        out.push_str(&format!("{seconds}S"));
    }
    out
}

pub fn parse_iso8601_duration(s: &str) -> Option<std::time::Duration> {
    let rest = s.strip_prefix("PT")?;
    let mut secs: u64 = 0;
    let mut num = String::new();
    let chars = rest.chars().peekable();
    for c in chars {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
            continue;
        }
        let n: f64 = num.parse().ok()?;
        num.clear();
        match c {
            'H' => secs += (n * 3600.0) as u64,
            'M' => secs += (n * 60.0) as u64,
            'S' => secs += n as u64,
            _ => return None,
        }
    }
    Some(std::time::Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_string_roundtrip() {
        assert_eq!(LongString(1234567890123).to_value(), Value::String("1234567890123".into()));
        let v = Value::String("42".into());
        assert_eq!(LongString::parse(v).unwrap(), LongString(42));
    }

    #[test]
    fn duration_iso8601_roundtrip() {
        let d = std::time::Duration::from_secs(5400);
        assert_eq!(format_iso8601_duration(d), "PT1H30M");
        assert_eq!(parse_iso8601_duration("PT1H30M"), Some(d));
        assert_eq!(parse_iso8601_duration("PT45S"), Some(std::time::Duration::from_secs(45)));
    }
}
