// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! RFC 3339 UTC timestamps with nanoseconds, as IPNS writes its `Validity`
//! (`2026-10-29T12:00:00.000000000Z`), from and to Unix seconds. No calendar library.

/// Days since 1970-01-01 → (year, month, day) (Howard Hinnant's civil-from-days).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Unix seconds → `YYYY-MM-DDTHH:MM:SS.000000000Z`.
pub fn rfc3339(secs: u64) -> String {
    let s = secs as i64;
    let (y, mo, d) = civil(s.div_euclid(86_400));
    let t = s.rem_euclid(86_400);
    format!("{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}.000000000Z", t / 3600, t / 60 % 60, t % 60)
}

/// `YYYY-MM-DDTHH:MM:SS[.fraction]Z` → Unix seconds (fraction dropped). Only UTC (`Z`).
pub fn parse_rfc3339(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' || *b.last()? != b'Z' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> { s.get(r)?.parse().ok() };
    let (y, mo, d, h, mi, sec) = (num(0..4)?, num(5..7)?, num(8..10)?, num(11..13)?, num(14..16)?, num(17..19)?);
    let frac = &s[19..s.len() - 1];
    if !(frac.is_empty() || (frac.len() > 1 && frac.starts_with('.') && frac[1..].bytes().all(|c| c.is_ascii_digit()))) {
        return None;
    }
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 60 {
        return None;
    }
    let days = days_from_civil(i64::from(y), mo, d);
    u64::try_from(days * 86_400 + i64::from(h * 3600 + mi * 60 + sec)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000000000Z");
        assert_eq!(rfc3339(1_790_000_000), "2026-09-21T14:13:20.000000000Z");
        for t in [0, 951_782_400, 1_790_000_000, 4_102_444_800] {
            assert_eq!(parse_rfc3339(&rfc3339(t)), Some(t));
        }
        assert_eq!(parse_rfc3339("2026-09-21T14:13:20Z"), Some(1_790_000_000));
        assert_eq!(parse_rfc3339("2026-09-21T14:13:20+01:00"), None, "only UTC");
        assert_eq!(parse_rfc3339("2026-13-21T14:13:20Z"), None);
    }
}
