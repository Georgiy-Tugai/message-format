// Copyright 2026 the Message Format Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use alloc::string::String;
use core::fmt;

use icu_locale_core::Locale;

use crate::{MessageArgs, runtime};

/// Reusable formatter that resolves messages across one or more catalogs.
///
/// The type parameter `C` controls how catalogs are held. Common choices:
///
/// - `&Catalog` — borrow from an existing catalog (the default for
///   [`CatalogBundle::formatter`](crate::CatalogBundle::formatter)).
/// - `Arc<Catalog>` — shared ownership; the resulting formatter is `'static`
///   and can be cached in maps or embedded in session structs.
/// - `Catalog` — full ownership.
///
/// When multiple catalogs are provided, messages are resolved by searching
/// catalogs in the order they were given. This enables message-level fallback:
/// if a message is missing from the primary catalog, it can be found in a
/// secondary one without duplicating messages at compile time.
///
/// Arguments are automatically resolved against the catalog that owns the
/// matched message, so string-pool ids stay consistent.
pub struct MessageFormatter<C = runtime::Catalog> {
    #[cfg(feature = "icu4x")]
    inner: runtime::MultiFormatter<C, alloc::boxed::Box<runtime::BuiltinHost>>,
    #[cfg(not(feature = "icu4x"))]
    inner: runtime::MultiFormatter<C, runtime::NoopHost>,
}

impl<C: AsRef<runtime::Catalog>> fmt::Debug for MessageFormatter<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MessageFormatter").finish_non_exhaustive()
    }
}

impl<C: AsRef<runtime::Catalog>> MessageFormatter<C> {
    #[cfg(feature = "icu4x")]
    pub(crate) fn new(
        catalogs: impl IntoIterator<Item = C>,
        candidates: &[Locale],
    ) -> Result<Self, runtime::FormatError> {
        let mut last_err = None;
        for candidate in candidates {
            match runtime::BuiltinHost::new(candidate) {
                Ok(host) => {
                    return Ok(Self {
                        inner: runtime::MultiFormatter::new(catalogs, host.into())?,
                    });
                }
                Err(err) => last_err = Some(err),
            }
        }
        Err(last_err.unwrap_or(runtime::FormatError::Trap(runtime::Trap::UnsupportedLocale)))
    }

    #[cfg(not(feature = "icu4x"))]
    pub(crate) fn new(
        catalogs: impl IntoIterator<Item = C>,
        _candidates: &[Locale],
    ) -> Result<Self, runtime::FormatError> {
        Ok(Self {
            inner: runtime::MultiFormatter::new(catalogs, runtime::NoopHost)?,
        })
    }

    /// Create a single-catalog formatter bound to one locale.
    ///
    /// Uses CLDR-aware locale fallback to find the best available host locale.
    /// For message-level fallback across multiple catalogs, use
    /// [`CatalogBundle::formatter`](crate::CatalogBundle::formatter) or
    /// [`CatalogBundle::into_formatter`](crate::CatalogBundle::into_formatter)
    /// instead.
    pub fn for_locale(catalog: C, locale: &Locale) -> Result<Self, runtime::FormatError> {
        #[cfg(feature = "profiling")]
        profiling::function_scope!();
        let candidates = crate::catalog::locale_candidates(locale);
        Self::new(core::iter::once(catalog), &candidates)
    }

    /// Set the maximum number of VM instructions per format operation.
    pub fn set_fuel(&mut self, fuel: Option<u64>) {
        self.inner.set_fuel(fuel);
    }

    /// Resolve a message id to a reusable handle.
    ///
    /// Searches catalogs in the order they were provided and returns a handle
    /// to the first catalog that contains the message. Reuse the returned
    /// handle across repeated formatting calls to avoid per-call lookup.
    pub fn resolve(
        &self,
        message_id: &str,
    ) -> Result<runtime::MultiMessageHandle, runtime::FormatError> {
        #[cfg(feature = "profiling")]
        profiling::function_scope!();
        self.inner.resolve(message_id)
    }

    /// Format one message from a previously resolved handle.
    ///
    /// Arguments are resolved against the catalog that owns the matched
    /// message. Recoverable diagnostics from fallback rendering are ignored
    /// in this convenience API. Markup is flattened away; use
    /// [`runtime::MultiFormatter::format_to`] for structured output.
    pub fn format(
        &mut self,
        message: runtime::MultiMessageHandle,
        args: &MessageArgs,
    ) -> Result<String, runtime::FormatError> {
        #[cfg(feature = "profiling")]
        profiling::function_scope!();
        let mut out = String::new();
        self.format_into(message, args, &mut out)?;
        Ok(out)
    }

    /// Format one message into a caller-provided output buffer.
    ///
    /// Recoverable diagnostics from fallback rendering are ignored in this
    /// convenience API. Use runtime-level sink APIs when diagnostics are needed.
    fn format_into(
        &mut self,
        message: runtime::MultiMessageHandle,
        args: &MessageArgs,
        out: &mut String,
    ) -> Result<(), runtime::FormatError> {
        out.clear();
        let catalog = self.inner.catalog_for(message)?;
        let resolved = args.resolve(catalog);
        self.inner.format_to(message, &resolved, out, None)?;
        Ok(())
    }

    /// Format one message by id.
    ///
    /// Recoverable diagnostics from fallback rendering are ignored in this
    /// convenience API. Markup is flattened away; use
    /// [`runtime::MultiFormatter::format_to`] for structured output.
    pub fn format_by_id(
        &mut self,
        message_id: &str,
        args: &MessageArgs,
    ) -> Result<String, runtime::FormatError> {
        #[cfg(feature = "profiling")]
        profiling::function_scope!();
        let message = self.resolve(message_id)?;
        self.format(message, args)
    }
}
