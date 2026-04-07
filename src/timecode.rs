use anyhow::{Result, anyhow, bail};
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Timecode {
    seconds: u64,
}

impl Timecode {
    pub const ZERO: Self = Self { seconds: 0 };

    pub const fn from_seconds(seconds: u64) -> Self {
        Self { seconds }
    }

    pub const fn as_seconds(self) -> u64 {
        self.seconds
    }

    pub fn from_seconds_f64(seconds: f64) -> Option<Self> {
        if !seconds.is_finite() || seconds.is_sign_negative() {
            return None;
        }

        Some(Self::from_seconds(seconds.floor() as u64))
    }

    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        if input.is_empty() {
            bail!("timestamp cannot be empty");
        }

        let parts = input.split(':').collect::<Vec<_>>();
        if !(1..=3).contains(&parts.len()) {
            bail!("timestamp must use `SS`, `MM:SS`, or `HH:MM:SS`");
        }

        let numbers = parts
            .iter()
            .map(|part| {
                if part.is_empty() {
                    Err(anyhow!("timestamp contains an empty component"))
                } else if !part.chars().all(|ch| ch.is_ascii_digit()) {
                    Err(anyhow!("timestamp must contain only digits and `:`"))
                } else {
                    Ok(part.parse::<u64>()?)
                }
            })
            .collect::<Result<Vec<_>>>()?;

        let seconds = match numbers.as_slice() {
            [seconds] => *seconds,
            [minutes, seconds] => {
                if *seconds >= 60 {
                    bail!("seconds must be less than 60 in `MM:SS`");
                }
                minutes.saturating_mul(60).saturating_add(*seconds)
            }
            [hours, minutes, seconds] => {
                if *minutes >= 60 || *seconds >= 60 {
                    bail!("minutes and seconds must be less than 60 in `HH:MM:SS`");
                }
                hours
                    .saturating_mul(3600)
                    .saturating_add(minutes.saturating_mul(60))
                    .saturating_add(*seconds)
            }
            _ => unreachable!("length is validated above"),
        };

        Ok(Self::from_seconds(seconds))
    }

    pub fn apply_delta(self, delta_seconds: i64) -> Self {
        if delta_seconds.is_negative() {
            Self::from_seconds(self.seconds.saturating_sub(delta_seconds.unsigned_abs()))
        } else {
            Self::from_seconds(self.seconds.saturating_add(delta_seconds as u64))
        }
    }
}

impl fmt::Display for Timecode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total = self.seconds;
        let hours = total / 3600;
        let minutes = (total % 3600) / 60;
        let seconds = total % 60;
        write!(formatter, "{hours:02}:{minutes:02}:{seconds:02}")
    }
}

impl FromStr for Timecode {
    type Err = String;

    fn from_str(input: &str) -> std::result::Result<Self, Self::Err> {
        Self::parse(input).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::Timecode;

    #[test]
    fn parses_supported_formats() {
        assert_eq!(Timecode::parse("90").unwrap().as_seconds(), 90);
        assert_eq!(Timecode::parse("01:30").unwrap().as_seconds(), 90);
        assert_eq!(Timecode::parse("01:02:03").unwrap().as_seconds(), 3723);
    }

    #[test]
    fn rejects_invalid_formats() {
        assert!(Timecode::parse("").is_err());
        assert!(Timecode::parse("1:99").is_err());
        assert!(Timecode::parse("1:2:99").is_err());
        assert!(Timecode::parse("aa").is_err());
    }

    #[test]
    fn formats_as_hms() {
        assert_eq!(Timecode::from_seconds(0).to_string(), "00:00:00");
        assert_eq!(Timecode::from_seconds(3723).to_string(), "01:02:03");
    }

    #[test]
    fn converts_from_f64_seconds() {
        assert_eq!(
            Timecode::from_seconds_f64(3723.9).unwrap().to_string(),
            "01:02:03"
        );
        assert!(Timecode::from_seconds_f64(-1.0).is_none());
    }

    #[test]
    fn applies_deltas_safely() {
        let start = Timecode::from_seconds(90);
        assert_eq!(start.apply_delta(30).as_seconds(), 120);
        assert_eq!(start.apply_delta(-30).as_seconds(), 60);
        assert_eq!(start.apply_delta(-200).as_seconds(), 0);
    }
}
