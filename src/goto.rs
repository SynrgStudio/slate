#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GotoTarget {
    Absolute {
        line: usize,
        column: Option<usize>,
    },
    Relative {
        offset: isize,
        column: Option<usize>,
    },
}

impl GotoTarget {
    pub(crate) fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        if input.is_empty() {
            return None;
        }

        let (line_part, column) = match input.split_once(':') {
            Some((line, column)) => (line.trim(), parse_positive_usize(column.trim())),
            None => (input, None),
        };

        if line_part.is_empty() {
            return None;
        }

        if let Some(rest) = line_part.strip_prefix('+') {
            let offset = parse_positive_usize(rest)? as isize;
            return Some(Self::Relative { offset, column });
        }

        if let Some(rest) = line_part.strip_prefix('-') {
            let offset = parse_positive_usize(rest)? as isize;
            return Some(Self::Relative {
                offset: -offset,
                column,
            });
        }

        let line = parse_positive_usize(line_part)?;
        Some(Self::Absolute { line, column })
    }
}

fn parse_positive_usize(input: &str) -> Option<usize> {
    let value = input.parse::<usize>().ok()?;
    (value > 0).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::GotoTarget;

    #[test]
    fn parses_absolute_line() {
        assert_eq!(
            GotoTarget::parse("10"),
            Some(GotoTarget::Absolute {
                line: 10,
                column: None
            })
        );
    }

    #[test]
    fn parses_absolute_line_and_column() {
        assert_eq!(
            GotoTarget::parse("10:8"),
            Some(GotoTarget::Absolute {
                line: 10,
                column: Some(8)
            })
        );
    }

    #[test]
    fn parses_relative_offsets() {
        assert_eq!(
            GotoTarget::parse("+10"),
            Some(GotoTarget::Relative {
                offset: 10,
                column: None
            })
        );
        assert_eq!(
            GotoTarget::parse("-10"),
            Some(GotoTarget::Relative {
                offset: -10,
                column: None
            })
        );
    }

    #[test]
    fn rejects_zero_and_empty_targets() {
        assert_eq!(GotoTarget::parse(""), None);
        assert_eq!(GotoTarget::parse("0"), None);
        assert_eq!(GotoTarget::parse("+0"), None);
        assert_eq!(GotoTarget::parse("abc"), None);
    }
}
