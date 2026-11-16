/// Pass-through function for use with `map_or`/`map_or_else` to avoid
/// clippy's `map_or_identity` lint while still expressing identity.
pub const fn pass_through<T>(x: T) -> T {
    x
}

/// Type-annotated pass-through for `&str` values.
pub const fn str_pass_through(s: &str) -> &str {
    s
}
