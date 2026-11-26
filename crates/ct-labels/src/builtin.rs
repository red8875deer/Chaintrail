//! Label packs compiled into the binary, from `data/labels/*.toml` (see `build.rs`).

/// `(file name, contents)` of every built-in pack.
pub fn builtin_packs() -> &'static [(&'static str, &'static str)] {
    BUILTIN
}

const BUILTIN: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/builtin_packs.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_pack_is_valid() {
        let index = crate::LabelIndex::with_builtin().unwrap();
        assert_eq!(index.packs().len(), BUILTIN.len());
        assert!(BUILTIN.iter().any(|(n, _)| *n == "ofac-sdn.toml"));
    }
}
