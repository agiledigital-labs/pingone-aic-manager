//! Compact duration parsing and human-readable duration formatting for agent UI.

/// Parse a compact duration into seconds.
///
/// Accepts a bare number of seconds or ordered `h`, `m`, and `s` components,
/// such as `1h20m30s`. Days are deliberately unsupported.
pub fn parse_duration(input: &str) -> Result<u64, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("duration cannot be empty".into());
    }
    if input.starts_with('-') {
        return Err("duration cannot be negative".into());
    }
    if input.bytes().all(|byte| byte.is_ascii_digit()) {
        return input.parse().map_err(|_| "duration is too large".into());
    }

    let mut total = 0_u64;
    let mut previous_unit = 3_u8;
    let mut offset = 0;
    while offset < input.len() {
        let digits_end = input[offset..]
            .find(|character: char| !character.is_ascii_digit())
            .map_or(input.len(), |index| offset + index);
        if digits_end == offset || digits_end == input.len() {
            return Err(format!(
                "invalid duration `{input}`; use forms such as 1h20m30s"
            ));
        }

        let value: u64 = input[offset..digits_end]
            .parse()
            .map_err(|_| "duration is too large".to_string())?;
        let unit = input.as_bytes()[digits_end];
        let (order, multiplier) = match unit {
            b'h' => (2, 3600),
            b'm' => (1, 60),
            b's' => (0, 1),
            _ => return Err(format!("invalid duration `{input}`; use h, m, or s units")),
        };
        if order >= previous_unit {
            return Err(format!(
                "invalid duration `{input}`; units must be ordered h, m, s"
            ));
        }
        total = total
            .checked_add(
                value
                    .checked_mul(multiplier)
                    .ok_or_else(|| "duration is too large".to_string())?,
            )
            .ok_or_else(|| "duration is too large".to_string())?;
        previous_unit = order;
        offset = digits_end + 1;
    }

    Ok(total)
}

/// Render seconds from the largest non-zero unit down to seconds.
pub fn format_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        return format!("{hours}h {minutes}m {seconds}s");
    }
    if minutes > 0 {
        return format!("{minutes}m {seconds}s");
    }
    format!("{seconds}s")
}

#[cfg(test)]
mod tests {
    use super::{format_duration, parse_duration};

    #[test]
    fn parses_compact_and_bare_durations() {
        assert_eq!(parse_duration("1h20m"), Ok(4800));
        assert_eq!(parse_duration("1h20m30s"), Ok(4830));
        assert_eq!(parse_duration("90m"), Ok(5400));
        assert_eq!(parse_duration("3600s"), Ok(3600));
        assert_eq!(parse_duration("1h"), Ok(3600));
        assert_eq!(parse_duration(" 3600 "), Ok(3600));
    }

    #[test]
    fn rejects_invalid_durations() {
        for input in ["", "-1", "1m1h", "1h1h", "1h20", "1d", "seconds"] {
            assert!(parse_duration(input).is_err(), "{input} should be rejected");
        }
    }

    #[test]
    fn formats_durations() {
        assert_eq!(format_duration(4775), "1h 19m 35s");
        assert_eq!(format_duration(1175), "19m 35s");
        assert_eq!(format_duration(35), "35s");
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(4800), "1h 20m 0s");
    }

    #[test]
    fn parse_and_format_round_trip() {
        let seconds = parse_duration("1h20m30s").unwrap();
        assert_eq!(format_duration(seconds), "1h 20m 30s");
    }
}
