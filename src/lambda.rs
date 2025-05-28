//! The [`Lambda`] type

use std::fmt;

use serde::*;

use ecow::EcoString;

use crate::{Node, SigNode, Value};

/// A lambda
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct Lambda {
    /// Callable node
    pub sn: SigNode,
    /// Representation
    pub repr: EcoString,
}

impl fmt::Debug for Lambda {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "⋋")?;
        self.sn.node.fmt(f)
    }
}

impl fmt::Display for Lambda {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "⋋{}", self.repr)
    }
}

impl Lambda {
    /// Wrap a noad into a lambda that returns it
    pub fn noad(value: Value) -> Lambda {
        Lambda {
            repr: value.representation().into(),
            sn: SigNode::new((0, 1), Node::new_push(value)),
        }
    }
}
