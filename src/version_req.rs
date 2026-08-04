use crate::model::Bump;

/// Normalizes a Cargo version requirement for comparison.
pub fn req_norm(req: &str) -> String {
    let mut normalized: String = req.chars().filter(|c| !c.is_whitespace()).collect();
    if let Some(rest) = normalized.strip_prefix('^') {
        normalized = rest.to_string();
    }

    let bytes = normalized.as_bytes();
    let metadata = bytes.windows(2).position(|pair| {
        matches!(pair[0], b'-' | b'+') && (pair[1].is_ascii_alphanumeric() || pair[1] == b'.')
    });
    if let Some(index) = metadata {
        normalized.truncate(index);
    }
    normalized
}

/// Returns the first numeric version literal in a Cargo requirement.
pub fn req_version(req: &str) -> String {
    let normalized = req_norm(req);
    let Some(start) = normalized.find(|c: char| c.is_ascii_digit()) else {
        return String::new();
    };
    let version: String = normalized[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    version.strip_suffix('.').unwrap_or(&version).to_string()
}

/// Classifies a requirement change under Cargo's caret compatibility rules.
pub fn classify_bump(old: &str, new: &str) -> Bump {
    let old_version = req_version(old);
    let new_version = req_version(new);
    if old_version == new_version && req_norm(old) != req_norm(new) {
        return Bump::Unknown;
    }

    let old_major = component(&old_version, 0);
    let new_major = component(&new_version, 0);
    if old_major != new_major {
        return Bump::Major;
    }

    let old_minor = component(&old_version, 1);
    let new_minor = component(&new_version, 1);
    if old_minor != new_minor {
        return if old_major == "0" {
            Bump::Major
        } else {
            Bump::Minor
        };
    }

    if old_major == "0" && old_minor == "0" && old_version != new_version {
        return Bump::Major;
    }
    Bump::Patch
}

fn component(version: &str, index: usize) -> &str {
    version.split('.').nth(index).unwrap_or(version)
}

#[cfg(test)]
#[path = "version_req/tests.rs"]
mod tests;
