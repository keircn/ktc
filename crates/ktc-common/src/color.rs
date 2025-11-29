pub fn parse_color(s: &str) -> Option<u32> {
    let s = s.trim().trim_start_matches('#');
    if s.len() == 6 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some(0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    } else if s.len() == 8 {
        let a = u8::from_str_radix(&s[0..2], 16).ok()?;
        let r = u8::from_str_radix(&s[2..4], 16).ok()?;
        let g = u8::from_str_radix(&s[4..6], 16).ok()?;
        let b = u8::from_str_radix(&s[6..8], 16).ok()?;
        Some(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rgb() {
        assert_eq!(parse_color("#FF0000"), Some(0xFFFF0000));
        assert_eq!(parse_color("#00FF00"), Some(0xFF00FF00));
        assert_eq!(parse_color("#0000FF"), Some(0xFF0000FF));
        assert_eq!(parse_color("1A1A2E"), Some(0xFF1A1A2E));
    }

    #[test]
    fn test_parse_argb() {
        assert_eq!(parse_color("#80FF0000"), Some(0x80FF0000));
        assert_eq!(parse_color("00000000"), Some(0x00000000));
    }

    #[test]
    fn test_invalid() {
        assert_eq!(parse_color(""), None);
        assert_eq!(parse_color("#FFF"), None);
        assert_eq!(parse_color("invalid"), None);
    }
}
