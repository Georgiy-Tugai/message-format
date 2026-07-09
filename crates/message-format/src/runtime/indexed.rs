// Copyright 2026 the Message Format Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Shareable per-catalog host indexes.
//!
//! [`Host::index`](crate::runtime::Host::index) computes catalog-specific data
//! once per formatter construction. For most hosts that data is a pure
//! function of the catalog — no host instance, no locale — so recomputing it
//! for every formatter (one per locale, typically) is wasted work.
//! [`CatalogDerived`] declares that property, and [`IndexedCatalog`] pairs a
//! catalog with an index built once, so the pair can be shared (e.g. via
//! `Arc`) across all formatters and locales that use the catalog.

use crate::runtime::{catalog::Catalog, error::FormatError};

/// Declares that a host index is a pure function of the catalog.
///
/// Implement this for a [`Host::CatalogIndex`](crate::runtime::Host::CatalogIndex)
/// type when building it needs *only* the catalog — no host instance and no
/// locale. That property is the precondition for sharing one prebuilt index
/// across formatters and locales via [`IndexedCatalog::new`]. Hosts whose
/// index depends on host state must instead build it through
/// [`Host::index`](crate::runtime::Host::index) and are not eligible for the
/// catalog-only sharing path.
///
/// Implementations must keep [`Host::index`](crate::runtime::Host::index)
/// and [`from_catalog`](Self::from_catalog) in agreement; catalog-only hosts
/// should implement `Host::index` as the one-line delegation
/// `Self::CatalogIndex::from_catalog(catalog)` so the two build paths cannot
/// diverge.
pub trait CatalogDerived: Sized {
    /// Build the index from the catalog alone.
    fn from_catalog(catalog: &Catalog) -> Result<Self, FormatError>;
}

/// Hosts without catalog data (`NoopHost`, `HostFn`) use `()` — zero-cost.
impl CatalogDerived for () {
    fn from_catalog(_catalog: &Catalog) -> Result<Self, FormatError> {
        Ok(())
    }
}

/// A catalog paired with a prebuilt host index.
///
/// This is the unit applications cache and share: build it once per catalog
/// with [`new`](Self::new), then construct any number of formatters from it
/// (across locales and threads, e.g. behind an `Arc`) via
/// [`Formatter::from_indexed`](crate::runtime::Formatter::from_indexed) /
/// [`MultiFormatter::from_indexed`](crate::runtime::MultiFormatter::from_indexed)
/// without re-running the index scan.
///
/// `C` is any catalog carrier (`Catalog`, `&Catalog`, `Arc<Catalog>`, …) via
/// `AsRef<Catalog>`; `I` is the host's
/// [`CatalogIndex`](crate::runtime::Host::CatalogIndex) type.
#[derive(Debug, Clone)]
pub struct IndexedCatalog<C, I> {
    catalog: C,
    index: I,
}

impl<C: AsRef<Catalog>, I: CatalogDerived> IndexedCatalog<C, I> {
    /// Build the index for `catalog` — the catalog-only sharing entry point.
    ///
    /// Requires the index to be [`CatalogDerived`]; hosts with
    /// instance-dependent indexes must go through
    /// [`Host::index`](crate::runtime::Host::index) and [`Self::from_parts`].
    pub fn new(catalog: C) -> Result<Self, FormatError> {
        #[cfg(feature = "profiling")]
        profiling::function_scope!();
        let index = I::from_catalog(catalog.as_ref())?;
        Ok(Self { catalog, index })
    }
}

impl<C: AsRef<Catalog>, I> IndexedCatalog<C, I> {
    /// Pair an explicitly built index with its catalog.
    ///
    /// Use this when the index came from a host-dependent
    /// [`Host::index`](crate::runtime::Host::index) call. The caller asserts
    /// provenance: `index` must have been built from this same `catalog`. A
    /// mismatched index is memory-safe and panic-free (all id lookups are
    /// bounds-checked), but silently produces wrong output — the same
    /// contract class as the handle-provenance caveat on
    /// [`MultiFormatter::catalog_for`](crate::runtime::MultiFormatter::catalog_for).
    pub fn from_parts(catalog: C, index: I) -> Self {
        Self { catalog, index }
    }

    /// Returns a reference to the underlying catalog.
    pub fn catalog(&self) -> &Catalog {
        self.catalog.as_ref()
    }

    /// Returns the prebuilt host index.
    pub fn index(&self) -> &I {
        &self.index
    }

    /// Consume the pair and return the catalog carrier, discarding the index.
    pub fn into_inner(self) -> C {
        self.catalog
    }
}

impl<C: AsRef<Catalog>, I> AsRef<Catalog> for IndexedCatalog<C, I> {
    fn as_ref(&self) -> &Catalog {
        self.catalog.as_ref()
    }
}

/// Reflexive impl so a by-value `IndexedCatalog` satisfies the
/// `P: AsRef<IndexedCatalog<C, I>>` carrier bounds on formatters; std's
/// blanket impls extend this to `&T` (including nesting), `Box`, `Rc`, `Arc`,
/// and custom pointers. Mirrors `impl AsRef<Self> for Catalog`.
impl<C, I> AsRef<Self> for IndexedCatalog<C, I> {
    fn as_ref(&self) -> &Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, rc::Rc, sync::Arc};

    use super::*;
    use crate::runtime::catalog::{MessageEntry, build_catalog};
    use crate::runtime::schema::TestOps;

    fn test_catalog() -> Catalog {
        let code = TestOps::new().out_slice(0, 2).halt().build();
        let bytes = build_catalog(
            &["greet"],
            "hi",
            &[MessageEntry {
                name_str_id: 0,
                entry_pc: 0,
            }],
            &code,
        );
        Catalog::from_bytes(&bytes).expect("valid catalog")
    }

    #[test]
    fn unit_index_from_catalog() {
        let catalog = test_catalog();
        let indexed: IndexedCatalog<&Catalog, ()> =
            IndexedCatalog::new(&catalog).expect("unit index");
        assert!(indexed.catalog().string_id("greet").is_some());
        assert_eq!(*indexed.index(), ());
        let carrier = indexed.into_inner();
        assert!(carrier.string_id("greet").is_some());
    }

    #[test]
    fn new_propagates_index_errors() {
        #[derive(Debug)]
        struct Failing;
        impl CatalogDerived for Failing {
            fn from_catalog(_catalog: &Catalog) -> Result<Self, FormatError> {
                Err(FormatError::StackUnderflow)
            }
        }
        let catalog = test_catalog();
        let err = IndexedCatalog::<_, Failing>::new(&catalog).unwrap_err();
        assert_eq!(err, FormatError::StackUnderflow);
    }

    /// Every carrier shape reaches the same `IndexedCatalog` through the
    /// reflexive impl plus std's `AsRef` blankets.
    #[test]
    fn carrier_shapes_reach_indexed_catalog() {
        fn catalog_of<'a, P, C: AsRef<Catalog> + 'a>(carrier: &'a P) -> &'a Catalog
        where
            P: AsRef<IndexedCatalog<C, ()>>,
        {
            carrier.as_ref().catalog()
        }

        let catalog = test_catalog();
        let by_value: IndexedCatalog<&Catalog, ()> =
            IndexedCatalog::new(&catalog).expect("index");
        assert!(catalog_of(&by_value).string_id("greet").is_some());
        assert!(catalog_of(&&by_value).string_id("greet").is_some());

        let boxed = Box::new(by_value.clone());
        assert!(catalog_of(&boxed).string_id("greet").is_some());

        let rced = Rc::new(by_value.clone());
        assert!(catalog_of(&rced).string_id("greet").is_some());

        let arced = Arc::new(by_value.clone());
        assert!(catalog_of(&arced).string_id("greet").is_some());
        // Nested: &Arc<IndexedCatalog> forwards through the reference blanket.
        assert!(catalog_of(&&arced).string_id("greet").is_some());
    }

    #[test]
    fn as_ref_catalog_view() {
        let catalog = test_catalog();
        let indexed: IndexedCatalog<&Catalog, ()> =
            IndexedCatalog::new(&catalog).expect("index");
        let view: &Catalog = indexed.as_ref();
        assert!(view.string_id("greet").is_some());
    }

    #[cfg(feature = "icu4x")]
    #[test]
    fn indexed_catalog_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<IndexedCatalog<Catalog, crate::runtime::BuiltinHostCatalogIndex>>();
    }
}
