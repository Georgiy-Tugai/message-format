// Copyright 2026 the Message Format Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use alloc::vec::Vec;
#[cfg(not(feature = "icu4x"))]
use alloc::vec;

use icu_locale_core::Locale;

use crate::{formatter::MessageFormatter, runtime};

#[cfg(feature = "icu4x")]
pub(crate) fn locale_candidates(locale: &Locale) -> Vec<Locale> {
    runtime::locale_fallback_candidates(locale)
}

#[cfg(not(feature = "icu4x"))]
pub(crate) fn locale_candidates(locale: &Locale) -> Vec<Locale> {
    if locale.id.is_unknown() {
        vec![locale.clone()]
    } else {
        vec![locale.clone(), Locale::UNKNOWN]
    }
}

/// Host index type used by the facade's built-in host.
#[cfg(feature = "icu4x")]
pub(crate) type HostIndex = runtime::BuiltinHostCatalogIndex;
/// Host index type used by the facade's no-op host.
#[cfg(not(feature = "icu4x"))]
pub(crate) type HostIndex = ();

/// A catalog paired with its prebuilt host index for the facade's built-in
/// host — the unit applications cache and share across locales.
///
/// Building a formatter from a plain [`Catalog`](runtime::Catalog) scans the
/// catalog to precompute host data. That scan depends only on the catalog, so
/// an `IndexedCatalog` built once (per catalog) can be reused by every
/// formatter and locale — typically cached as `Arc<IndexedCatalog>` and
/// handed to [`CatalogBundle`] or
/// [`MessageFormatter::for_locale`], which accept plain and pre-indexed
/// catalogs interchangeably via [`IntoIndexedCatalog`].
///
/// `C` is the catalog carrier (`Catalog` by default; `&Catalog`,
/// `Arc<Catalog>`, … also work).
pub type IndexedCatalog<C = runtime::Catalog> = runtime::IndexedCatalog<C, HostIndex>;

impl runtime::Catalog {
    /// Resolve a message id to a reusable handle.
    pub fn resolve(
        &self,
        message_id: &str,
    ) -> Result<runtime::MessageHandle, runtime::FormatError> {
        runtime::MessageHandle::from_catalog(self, message_id)
    }

    /// Create a single-catalog formatter bound to one locale.
    ///
    /// Uses CLDR-aware locale fallback to find the best available host locale.
    /// For message-level fallback across multiple catalogs, use
    /// [`CatalogBundle::formatter`] instead. When building formatters for
    /// many locales of the same catalog, index once via
    /// [`IntoIndexedCatalog::into_indexed_catalog`] and call
    /// [`IndexedCatalog::formatter_for_locale`] instead.
    pub fn formatter_for_locale(
        &self,
        locale: &Locale,
    ) -> Result<MessageFormatter<IndexedCatalog<&'_ Self>>, runtime::FormatError> {
        MessageFormatter::for_locale(self, locale)
    }
}

impl<C: AsRef<runtime::Catalog>> IndexedCatalog<C> {
    /// Create a single-catalog formatter bound to one locale, reusing this
    /// prebuilt index.
    ///
    /// Unlike [`Catalog::formatter_for_locale`](runtime::Catalog::formatter_for_locale),
    /// this does not re-scan the catalog — only the per-locale host is built.
    pub fn formatter_for_locale(
        &self,
        locale: &Locale,
    ) -> Result<MessageFormatter<&'_ Self>, runtime::FormatError> {
        MessageFormatter::for_locale(self, locale)
    }
}

/// Boundary conversion accepted by [`CatalogBundle`] and
/// [`MessageFormatter::for_locale`]: plain catalogs are indexed on the way in
/// (the same work a formatter would do), pre-indexed catalogs pass through
/// untouched, so a shared `Arc<IndexedCatalog>` is never re-indexed.
///
/// Implemented for `Catalog`, `&Catalog`, and `Arc<Catalog>` (auto-index) and
/// for `IndexedCatalog<C>`, `&IndexedCatalog<C>`, and `Arc<IndexedCatalog<C>>`
/// (pass-through). Applications can implement it for their own catalog
/// newtypes.
pub trait IntoIndexedCatalog {
    /// Catalog carrier inside the resulting [`IndexedCatalog`].
    type Carrier: AsRef<runtime::Catalog>;
    /// The indexed-catalog carrier this conversion produces.
    type Indexed: AsRef<IndexedCatalog<Self::Carrier>>;
    /// Convert, indexing the catalog unless it is already indexed.
    fn into_indexed_catalog(self) -> Result<Self::Indexed, runtime::FormatError>;
}

impl IntoIndexedCatalog for runtime::Catalog {
    type Carrier = Self;
    type Indexed = IndexedCatalog;

    fn into_indexed_catalog(self) -> Result<Self::Indexed, runtime::FormatError> {
        IndexedCatalog::new(self)
    }
}

impl IntoIndexedCatalog for &runtime::Catalog {
    type Carrier = Self;
    type Indexed = IndexedCatalog<Self>;

    fn into_indexed_catalog(self) -> Result<Self::Indexed, runtime::FormatError> {
        IndexedCatalog::new(self)
    }
}

#[cfg(target_has_atomic = "ptr")]
impl IntoIndexedCatalog for alloc::sync::Arc<runtime::Catalog> {
    type Carrier = Self;
    type Indexed = IndexedCatalog<Self>;

    fn into_indexed_catalog(self) -> Result<Self::Indexed, runtime::FormatError> {
        IndexedCatalog::new(self)
    }
}

impl<C: AsRef<runtime::Catalog>> IntoIndexedCatalog for IndexedCatalog<C> {
    type Carrier = C;
    type Indexed = Self;

    fn into_indexed_catalog(self) -> Result<Self::Indexed, runtime::FormatError> {
        Ok(self)
    }
}

impl<C: AsRef<runtime::Catalog>> IntoIndexedCatalog for &IndexedCatalog<C> {
    type Carrier = C;
    type Indexed = Self;

    fn into_indexed_catalog(self) -> Result<Self::Indexed, runtime::FormatError> {
        Ok(self)
    }
}

#[cfg(target_has_atomic = "ptr")]
impl<C: AsRef<runtime::Catalog>> IntoIndexedCatalog for alloc::sync::Arc<IndexedCatalog<C>> {
    type Carrier = C;
    type Indexed = Self;

    fn into_indexed_catalog(self) -> Result<Self::Indexed, runtime::FormatError> {
        Ok(self)
    }
}

/// A catalog associated with one locale.
#[derive(Debug, Clone)]
pub struct LocalizedCatalog<C = runtime::Catalog> {
    /// Locale for this catalog.
    pub locale: Locale,
    /// Message catalog payload.
    pub catalog: C,
}

impl<C> LocalizedCatalog<C> {
    /// Construct a localized catalog pair.
    #[must_use]
    pub fn new(locale: Locale, catalog: C) -> Self {
        Self { locale, catalog }
    }
}

/// Immutable collection of catalogs pre-sorted in locale fallback order.
///
/// Accepts a set of [`LocalizedCatalog`]s and a target locale at construction,
/// immediately filtering and ordering catalogs by the CLDR fallback chain.
/// Messages are resolved by searching catalogs in order, so a message missing
/// from a more-specific catalog can still be found in a less-specific one.
///
/// The stored carrier `P` is an [`IndexedCatalog`] shape produced by the
/// input's [`IntoIndexedCatalog`] conversion: plain catalogs are indexed at
/// bundle construction, pre-indexed inputs (e.g. `Arc<IndexedCatalog>`) are
/// stored as-is.
#[derive(Debug, Clone)]
pub struct CatalogBundle<P = IndexedCatalog> {
    catalogs: Vec<P>,
    /// Formatting-locale candidates, independent of catalog locales
    candidates: Vec<Locale>,
}

impl<P> CatalogBundle<P> {
    /// Create a bundle targeting `locale` from the given catalogs.
    ///
    /// Computes the CLDR fallback chain for the requested locale and retains
    /// only catalogs whose locale appears in that chain, ordered from most
    /// specific to least. Catalogs that are not already indexed are indexed
    /// here — only the retained ones. Returns an error if no catalog matches
    /// any candidate in the fallback chain, or if indexing fails.
    pub fn new<T: IntoIndexedCatalog<Indexed = P>>(
        catalogs: impl IntoIterator<Item = LocalizedCatalog<T>>,
        locale: &Locale,
    ) -> Result<Self, runtime::FormatError> {
        #[cfg(feature = "profiling")]
        profiling::function_scope!();
        let candidates = locale_candidates(locale);
        let mut slots: Vec<Option<T>> = core::iter::repeat_with(|| None)
            .take(candidates.len())
            .collect();
        for lc in catalogs {
            if let Some(pos) = candidates.iter().position(|c| *c == lc.locale) {
                slots[pos] = Some(lc.catalog);
            }
        }
        let catalogs: Vec<P> = slots
            .into_iter()
            .flatten()
            .map(IntoIndexedCatalog::into_indexed_catalog)
            .collect::<Result<_, _>>()?;
        if catalogs.is_empty() {
            return Err(runtime::FormatError::Trap(
                runtime::Trap::MissingLocaleCatalog,
            ));
        }
        Ok(Self {
            catalogs,
            candidates,
        })
    }

    /// Create a bundle by looking up catalogs for each locale in the fallback
    /// chain.
    ///
    /// Calls `fetch` once per candidate locale, from most specific to least.
    /// The callback returns `Ok(Some(catalog))` when a catalog is available,
    /// `Ok(None)` when none exists for that locale, or `Err(e)` to abort.
    /// Returning pre-indexed catalogs (e.g. `Arc<IndexedCatalog>` clones from
    /// a cache) avoids re-indexing; plain catalogs are indexed here.
    /// Returns [`LookupError::MissingLocaleCatalog`] if no candidate produced
    /// a catalog.
    pub fn from_lookup<T: IntoIndexedCatalog<Indexed = P>, E>(
        locale: &Locale,
        mut fetch: impl FnMut(&Locale) -> Result<Option<T>, E>,
    ) -> Result<Self, LookupError<E>> {
        #[cfg(feature = "profiling")]
        profiling::function_scope!();
        let candidates = locale_candidates(locale);
        let mut catalogs = Vec::new();
        for candidate in &candidates {
            match fetch(candidate) {
                Ok(Some(catalog)) => catalogs.push(
                    catalog
                        .into_indexed_catalog()
                        .map_err(LookupError::Index)?,
                ),
                Ok(None) => {}
                Err(e) => return Err(LookupError::Fetch(e)),
            }
        }
        if catalogs.is_empty() {
            return Err(LookupError::MissingLocaleCatalog);
        }
        Ok(Self {
            catalogs,
            candidates,
        })
    }

    /// Create a multi-catalog formatter with message-level fallback.
    ///
    /// Returns a borrowing formatter tied to the bundle's lifetime; the
    /// carriers are borrowed at their canonical `&IndexedCatalog` view
    /// regardless of how the bundle stores them. For an owning formatter
    /// (e.g. `'static` when `P = Arc<IndexedCatalog>`), use
    /// [`into_formatter`](Self::into_formatter).
    ///
    /// Catalogs are searched in fallback order (most specific to least).
    /// The host locale for number/date formatting is derived from the
    /// target locale's CLDR fallback chain. Catalog indexes were built at
    /// bundle construction and are reused here.
    pub fn formatter<'a, C: AsRef<runtime::Catalog> + 'a>(
        &'a self,
    ) -> Result<MessageFormatter<&'a IndexedCatalog<C>>, runtime::FormatError>
    where
        P: AsRef<IndexedCatalog<C>>,
    {
        #[cfg(feature = "profiling")]
        profiling::function_scope!();
        MessageFormatter::new(self.catalogs.iter().map(AsRef::as_ref), &self.candidates)
    }

    /// Consume the bundle and return an owning formatter.
    ///
    /// When `P = Arc<IndexedCatalog>`, the resulting
    /// `MessageFormatter<Arc<IndexedCatalog>>` is `'static` and keeps sharing
    /// the cached catalog indexes; it can be cached in maps or embedded in
    /// long-lived structs. To keep using the bundle afterward, clone it
    /// first: `bundle.clone().into_formatter()` (cheap for `Arc` carriers).
    pub fn into_formatter<C: AsRef<runtime::Catalog>>(
        self,
    ) -> Result<MessageFormatter<P>, runtime::FormatError>
    where
        P: AsRef<IndexedCatalog<C>>,
    {
        #[cfg(feature = "profiling")]
        profiling::function_scope!();
        MessageFormatter::new(self.catalogs, &self.candidates)
    }

    /// Returns the fallback-chain candidates, most specific first.
    pub fn candidates(&self) -> &[Locale] {
        &self.candidates
    }

    /// Returns the retained catalogs in fallback order.
    pub fn catalogs(&self) -> &[P] {
        &self.catalogs
    }
}

/// Error returned by [`CatalogBundle::from_lookup`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LookupError<E> {
    /// The user-provided callback returned an error.
    Fetch(E),
    /// Building the host index for a fetched catalog failed.
    Index(runtime::FormatError),
    /// No catalog matched any candidate in the fallback chain.
    MissingLocaleCatalog,
}
