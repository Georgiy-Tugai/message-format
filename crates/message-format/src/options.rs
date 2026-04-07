// Copyright 2026 the Message Format Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

/// Locale resolution policy for formatter construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LocalePolicy {
    /// Use only the requested locale and return an error if unsupported.
    Exact,
    /// Try locale fallback before returning an error.
    ///
    /// Uses CLDR-aware locale fallback (via ICU4X `LocaleFallbacker`) to
    /// produce linguistically correct candidate chains. For example,
    /// `fr-CA` falls back to `fr`, while `pt-MZ` falls back to `pt-PT`.
    #[default]
    Lookup,
}
