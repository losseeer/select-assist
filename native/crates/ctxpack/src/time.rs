//! `new Date().toISOString()` 的等价物：不引日期库，直接用 civil-from-days 换算。

use std::time::{SystemTime, UNIX_EPOCH};

/// 公历日换算（Howard Hinnant 的算法），days 以 1970-01-01 为 0
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1461 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn iso_from_millis(millis: i64) -> String {
    let days = millis.div_euclid(86_400_000);
    let rest = millis.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    let hour = rest / 3_600_000;
    let minute = rest % 3_600_000 / 60_000;
    let second = rest % 60_000 / 1000;
    let frac = rest % 1000;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{frac:03}Z")
}

pub fn now_iso() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default();
    iso_from_millis(millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_leap_second_boundaries() {
        assert_eq!(iso_from_millis(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            iso_from_millis(1_767_225_600_000),
            "2026-01-01T00:00:00.000Z"
        );
        assert_eq!(iso_from_millis(-1), "1969-12-31T23:59:59.999Z");
        assert_eq!(iso_from_millis(86_399_999), "1970-01-01T23:59:59.999Z");
        assert_eq!(
            iso_from_millis(1_735_689_600_000),
            "2025-01-01T00:00:00.000Z"
        );
    }

    #[test]
    fn now_looks_like_an_iso_stamp() {
        let now = now_iso();
        assert_eq!(now.len(), 24, "{now}");
        assert!(now.ends_with('Z'));
        assert_eq!(&now[4..5], "-");
    }
}
