use std::fmt;

use serde::*;

use crate::SUPERSCRIPT_DIGITS;

/// A superscript
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Superscript<N = NumericSuperscript> {
    /// The superscripted number
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num: Option<N>,
}

/// The numeric part of a subscript
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum NumericSuperscript {
    /// The number is too large to be represented
    TooLarge,
    /// Infinity, with true indicating a negative sign
    Infinity(bool),
    /// A valid number
    #[serde(untagged)]
    N(i32),
}

impl<N: fmt::Display> fmt::Display for Superscript<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.num.is_none() {
            return write!(f, "*");
        };
        if let Some(num) = &self.num {
            num.fmt(f)?;
        }
        Ok(())
    }
}

impl fmt::Display for NumericSuperscript {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NumericSuperscript::N(n) => {
                if *n < 0 {
                    write!(f, "⁻")?;
                }
                for c in n.abs().to_string().chars() {
                    write!(
                        f,
                        "{}",
                        SUPERSCRIPT_DIGITS[(c as u32 as u8 - b'0') as usize]
                    )?;
                }
                Ok(())
            }
            NumericSuperscript::Infinity(false) => write!(f, "ºº"),
            NumericSuperscript::Infinity(true) => write!(f, "⁻ºº"),
            NumericSuperscript::TooLarge => write!(f, "…"),
        }
    }
}
