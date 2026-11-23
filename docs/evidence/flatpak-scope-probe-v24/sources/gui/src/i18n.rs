//! English presentation text and region-aware numeric formatting.
//!
//! The product is English-only. External language catalogs and LANGUAGE never
//! change application messages. Native system dialogs follow desktop settings.
use std::ffi::CStr;

/// Keep English application text stable while returning an owned widget value.
pub fn tr(message: &str) -> String {
    message.to_owned()
}

/// Preserve regional decimal formatting without changing global process locale.
/// Stored configuration, JSON and CSV remain locale-independent.
pub fn number(value: f64, precision: usize) -> String {
    let formatted = format!("{value:.precision$}");
    if precision == 0 || !value.is_finite() {
        return formatted;
    }
    unsafe {
        let locale = libc::newlocale(libc::LC_NUMERIC_MASK, c"".as_ptr(), std::ptr::null_mut());
        if locale.is_null() {
            return formatted;
        }
        let separator = libc::nl_langinfo_l(libc::RADIXCHAR, locale);
        let result = if separator.is_null() {
            formatted.clone()
        } else {
            replace_decimal(&formatted, &CStr::from_ptr(separator).to_string_lossy())
        };
        libc::freelocale(locale);
        result
    }
}

fn replace_decimal(value: &str, separator: &str) -> String {
    if separator.is_empty() || separator == "." {
        value.to_owned()
    } else {
        value.replace('.', separator)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn application_messages_are_english_and_empty_messages_stay_empty() {
        assert_eq!(tr("Unavailable"), "Unavailable");
        assert_eq!(tr(""), "");
    }
    #[test]
    fn decimal_separator_affects_only_numeric_presentation() {
        assert_eq!(replace_decimal("1234.50", ","), "1234,50");
        assert_eq!(replace_decimal("-2.5", "."), "-2.5");
        assert_eq!(replace_decimal("3.5", ""), "3.5");
        assert_eq!(number(12.4, 0), "12");
    }
}
