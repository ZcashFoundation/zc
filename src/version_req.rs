use semver::{Op, VersionReq};

use crate::model::Bump;

/// Classifies simple version changes under Cargo's compatibility rules.
///
/// Bounds and compound requirements need review. Ordinary requirements can prove
/// incompatible compatibility domains even when their operators or precision differ.
pub fn classify_bump(old: &str, new: &str) -> Bump {
    let (Ok(old), Ok(new)) = (VersionReq::parse(old), VersionReq::parse(new)) else {
        return Bump::Unknown;
    };
    if old == new {
        return Bump::Patch;
    }
    let ([old], [new]) = (old.comparators.as_slice(), new.comparators.as_slice()) else {
        return Bump::Unknown;
    };
    if !matches!(old.op, Op::Caret | Op::Tilde | Op::Exact | Op::Wildcard)
        || !matches!(new.op, Op::Caret | Op::Tilde | Op::Exact | Op::Wildcard)
    {
        return Bump::Unknown;
    }
    if !old.pre.is_empty() || !new.pre.is_empty() {
        // Graduation of the same version is compatible; other prerelease changes need review.
        return if old.op == new.op
            && old.major == new.major
            && old.minor == new.minor
            && old.patch == new.patch
            && !old.pre.is_empty()
            && new.pre.is_empty()
        {
            Bump::Patch
        } else {
            Bump::Unknown
        };
    }
    if old.major != new.major {
        return Bump::Major;
    }
    if old.major == 0 {
        if matches!((old.minor, new.minor), (Some(a), Some(b)) if a != b) {
            return Bump::Major;
        }
        if old.minor == Some(0) && new.minor == Some(0) && old.patch != new.patch {
            // Caret and exact requirements pin a 0.0 patch; tilde can overlap later patches.
            return match (old.patch, new.patch) {
                (Some(a), Some(b))
                    if (a < b && matches!(old.op, Op::Caret | Op::Exact))
                        || (b < a && matches!(new.op, Op::Caret | Op::Exact)) =>
                {
                    Bump::Major
                }
                _ => Bump::Unknown,
            };
        }
    }
    if old.op != new.op
        || old.minor.is_some() != new.minor.is_some()
        || old.patch.is_some() != new.patch.is_some()
    {
        return Bump::Unknown;
    }
    if old.minor != new.minor {
        return Bump::Minor;
    }
    Bump::Patch
}

#[cfg(test)]
#[path = "version_req/tests.rs"]
mod tests;
