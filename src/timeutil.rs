//! 不引入日期套件的最小時間工具。

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// 自公元 1970-01-01 起算的天數（Howard Hinnant 的 days_from_civil）。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// 解析 `2026-07-06T07:50:32Z`、`2026-07-06T07:50:32.123+08:00` 這類 RFC 3339 時間為 Unix 秒。
pub fn parse_rfc3339(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    let num = |r: std::ops::Range<usize>| text.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, s) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }

    let mut rest = &text[19..];
    if let Some(frac) = rest.strip_prefix('.') {
        rest = frac.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match rest.as_bytes().first()? {
        b'Z' | b'z' if rest.len() == 1 => 0,
        sign @ (b'+' | b'-') if rest.len() == 6 && rest.as_bytes()[3] == b':' => {
            let part = |r: std::ops::Range<usize>| rest.get(r)?.parse::<i64>().ok();
            let secs = part(1..3)? * 3600 + part(4..6)? * 60;
            if *sign == b'+' { secs } else { -secs }
        }
        _ => return None,
    };

    Some(days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + s - offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rfc3339() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2026-07-06T07:50:32Z"), Some(1_783_324_232));
        assert_eq!(parse_rfc3339("2026-07-06T07:50:32.5Z"), Some(1_783_324_232));
        assert_eq!(parse_rfc3339("2026-07-06T15:50:32+08:00"), Some(1_783_324_232));
        assert_eq!(parse_rfc3339("2000-02-29T00:00:00Z"), Some(951_782_400));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_rfc3339(""), None);
        assert_eq!(parse_rfc3339("2026-07-06"), None);
        assert_eq!(parse_rfc3339("2026-13-06T07:50:32Z"), None);
        assert_eq!(parse_rfc3339("2026-07-06T07:50:32"), None);
    }
}
