//! How we know something: fact, inference or attribution (ADR-0005).

use std::fmt;

use serde::{Deserialize, Serialize};

/// Confidence levels, ordered from weakest to strongest.
/// See `docs/TRACING.md` §6 for what each level means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Low,
    Medium,
    High,
    Certain,
}

impl Confidence {
    /// Map a heuristic score in `[0, 1]` to a level. `Certain` is reserved for facts and is
    /// never produced from a score.
    pub fn from_score(score: f64) -> Confidence {
        if score >= 0.85 {
            Confidence::High
        } else if score >= 0.6 {
            Confidence::Medium
        } else {
            Confidence::Low
        }
    }

    /// Whether automatic tracing may follow a link at this level.
    pub fn followable(self) -> bool {
        self >= Confidence::Medium
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Confidence::Low => "low",
            Confidence::Medium => "medium",
            Confidence::High => "high",
            Confidence::Certain => "certain",
        })
    }
}

/// The basis for a piece of information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Claim {
    /// Read directly from the blockchain.
    Fact,
    /// Produced by a named heuristic.
    Inference {
        heuristic: String,
        confidence: Confidence,
        /// Human-readable reasons, shown to the user.
        evidence: Vec<String>,
    },
    /// Stated by a label source.
    Attribution {
        /// Label pack id, e.g. `ofac-sdn`.
        source: String,
        confidence: Confidence,
        /// When the source made or last confirmed the statement (ISO date), if known.
        as_of: Option<String>,
        /// Where the statement can be checked.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
}

impl Claim {
    pub fn confidence(&self) -> Confidence {
        match self {
            Claim::Fact => Confidence::Certain,
            Claim::Inference { confidence, .. } | Claim::Attribution { confidence, .. } => {
                *confidence
            }
        }
    }

    pub fn is_fact(&self) -> bool {
        matches!(self, Claim::Fact)
    }
}

/// Confidence of a path: the weakest hop. An empty path is certain.
pub fn path_confidence<'a>(claims: impl IntoIterator<Item = &'a Claim>) -> Confidence {
    claims
        .into_iter()
        .map(Claim::confidence)
        .min()
        .unwrap_or(Confidence::Certain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_and_scores() {
        assert!(Confidence::Low < Confidence::Medium && Confidence::High < Confidence::Certain);
        assert_eq!(Confidence::from_score(1.0), Confidence::High);
        assert_eq!(Confidence::from_score(0.7), Confidence::Medium);
        assert_eq!(Confidence::from_score(0.1), Confidence::Low);
        assert!(!Confidence::Low.followable());
        assert!(Confidence::Medium.followable());
    }

    #[test]
    fn path_takes_weakest_link() {
        let hops = [
            Claim::Fact,
            Claim::Inference {
                heuristic: "change.address_type".into(),
                confidence: Confidence::Medium,
                evidence: vec![],
            },
            Claim::Fact,
        ];
        assert_eq!(path_confidence(&hops), Confidence::Medium);
        assert_eq!(path_confidence(&[]), Confidence::Certain);
    }

    #[test]
    fn serialises_with_kind_tag() {
        let json = serde_json::to_value(Claim::Fact).unwrap();
        assert_eq!(json["kind"], "fact");
    }
}
