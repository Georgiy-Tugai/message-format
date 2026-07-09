// Copyright 2026 the Message Format Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

// LINEBENDER LINT SET - lib.rs - v4
// See https://linebender.org/wiki/canonical-lints/
// These lints shouldn't apply to examples or tests.
#![cfg_attr(not(test), warn(unused_crate_dependencies))]
// These lints shouldn't apply to examples.
#![warn(clippy::print_stdout, clippy::print_stderr)]
// Targeting e.g. 32-bit means structs containing usize can give false positives for 64-bit.
#![cfg_attr(target_pointer_width = "64", warn(clippy::trivially_copy_pass_by_ref))]
// END LINEBENDER LINT SET
#![cfg_attr(docsrs, feature(doc_cfg))]
#![no_std]
#![doc = "Message-format runtime and optional compiler APIs."]

//! # Recommended Flow
//!
//! ```rust
//! # #[cfg(all(feature = "compile", feature = "icu4x"))]
//! # {
//! use message_format::{Catalog, Locale, MessageArgs, compiler::CompileOptions};
//!
//! let source = "Hello { $name }!";
//! let catalog = Catalog::compile(source, CompileOptions::default()).unwrap();
//! let locale: Locale = "en-US".parse().unwrap();
//! let mut formatter = catalog.formatter_for_locale(&locale).unwrap();
//!
//! let mut args = MessageArgs::new();
//! args.insert("name", "World");
//! assert_eq!(formatter.format_by_id("main", &args).unwrap(), "Hello World!");
//! # }
//! ```
//!
//! # Advanced Flow
//!
//! A [`CatalogBundle`] creates a multi-catalog formatter with message-level
//! fallback. All catalogs whose locale appears in the CLDR fallback chain for
//! the requested locale are searched in order, so a message missing from a
//! more-specific catalog can still be found in a less-specific one.
//!
//! ```rust
//! # #[cfg(all(feature = "compile", feature = "icu4x"))]
//! # {
//! use message_format::{Catalog, CatalogBundle, LocalizedCatalog, Locale, MessageArgs, compiler::CompileOptions};
//!
//! let fr: Locale = "fr".parse().unwrap();
//! let en: Locale = "en".parse().unwrap();
//! let fr_catalog = Catalog::compile("Salut { $name }", CompileOptions::default()).unwrap();
//! let en_catalog = Catalog::compile("Hello { $name }", CompileOptions::default()).unwrap();
//!
//! let requested: Locale = "fr-CA".parse().unwrap();
//! let bundle = CatalogBundle::new(
//!     [LocalizedCatalog::new(fr, fr_catalog), LocalizedCatalog::new(en, en_catalog)],
//!     &requested,
//! ).unwrap();
//! let mut formatter = bundle.formatter().unwrap();
//! let mut args = MessageArgs::new();
//! args.insert("name", "Ada");
//! assert_eq!(formatter.format_by_id("main", &args).unwrap(), "Salut Ada");
//! # }
//! ```
//!
//! # Sharing Catalog Indexes Across Locales
//!
//! Building a formatter from a plain [`Catalog`] scans the catalog once to
//! precompute host data. That scan depends only on the catalog — not the
//! locale — so applications that format across many locales should index each
//! catalog once and share the result: cache an `Arc<`[`IndexedCatalog`]`>` per
//! catalog and hand out clones. [`CatalogBundle`] and
//! [`MessageFormatter::for_locale`] accept plain and pre-indexed catalogs
//! interchangeably (via [`IntoIndexedCatalog`]); pre-indexed ones are never
//! re-scanned, leaving only the cheap per-locale host construction.
//!
//! ```rust
//! # #[cfg(all(feature = "compile", feature = "icu4x"))]
//! # {
//! use std::sync::Arc;
//! use message_format::{Catalog, IndexedCatalog, IntoIndexedCatalog, Locale, MessageArgs};
//!
//! let catalog = Catalog::compile_str("Hello { $name }").unwrap();
//! // Index once; cache this (it is Send + Sync and cheap to clone the Arc).
//! let shared: Arc<IndexedCatalog> = Arc::new(catalog.into_indexed_catalog().unwrap());
//!
//! let mut args = MessageArgs::new();
//! args.insert("name", "World");
//! for tag in ["en-US", "fr", "ja"] {
//!     let locale: Locale = tag.parse().unwrap();
//!     // Only the per-locale host is built here — no catalog re-scan.
//!     let mut formatter = shared.formatter_for_locale(&locale).unwrap();
//!     assert_eq!(formatter.format_by_id("main", &args).unwrap(), "Hello World");
//! }
//! # }
//! ```
//!
//! `CatalogBundle::from_lookup` composes with a cache naturally: return
//! `Arc<IndexedCatalog>` clones from the `fetch` closure and use
//! [`CatalogBundle::into_formatter`] for an owning, `'static` formatter.
//!
//! # Rich Output
//!
//! The facade APIs optimize for plain string formatting. They do not expose the
//! runtime sink interface directly, and markup is intentionally flattened away
//! in string output.
//!
//! When you need structured output, resolved markup options, or recoverable
//! diagnostics from fallback rendering, use [`runtime::Formatter::format_to`]
//! directly.

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub use icu_locale_core::Locale;

pub mod runtime;

#[cfg(feature = "compile")]
#[cfg_attr(docsrs, doc(cfg(feature = "compile")))]
pub mod compiler;

mod args;
mod catalog;
#[cfg(feature = "compile")]
mod catalog_compile;
mod common;
mod formatter;
pub use args::MessageArgs;
pub use catalog::{
    CatalogBundle, IndexedCatalog, IntoIndexedCatalog, LocalizedCatalog, LookupError,
};
pub use formatter::MessageFormatter;
pub use runtime::Catalog;

#[cfg(test)]
mod tests {
    #[cfg(all(feature = "compile", feature = "icu4x"))]
    use crate::catalog::locale_candidates;

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    use super::runtime::{FormatError, Trap};
    #[cfg(any(
        all(feature = "compile", feature = "icu4x"),
        all(feature = "compile", feature = "std")
    ))]
    use super::*;
    #[cfg(all(feature = "compile", feature = "std"))]
    use alloc::format;
    #[cfg(all(feature = "compile", feature = "icu4x"))]
    use alloc::string::String;
    #[cfg(all(feature = "compile", feature = "std"))]
    use core::sync::atomic::{AtomicU32, Ordering};

    #[cfg(all(feature = "compile", feature = "std"))]
    static TEMP_FILE_COUNTER: AtomicU32 = AtomicU32::new(0);

    #[cfg(all(feature = "compile", feature = "std"))]
    fn unique_temp_path(prefix: &str) -> std::path::PathBuf {
        let mut path = std::path::PathBuf::from(".");
        let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        path.push(format!("{prefix}_{counter}.mf2"));
        path
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    fn locale(tag: &str) -> Locale {
        tag.parse::<Locale>().expect("locale")
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    fn localized(tag: &str, source: &str) -> LocalizedCatalog {
        LocalizedCatalog::new(
            locale(tag),
            Catalog::compile_str(source).unwrap_or_else(|e| panic!("compile {tag}: {e:?}")),
        )
    }

    #[cfg(all(feature = "compile", feature = "std", feature = "icu4x"))]
    #[test]
    fn compile_entry_points_preserve_simple_whitespace() {
        let source = "  hello  ";
        let catalog_direct =
            Catalog::compile(source, compiler::CompileOptions::default()).expect("compile");
        let compiled_str = compiler::compile_str(source).expect("compile_str");
        let catalog_str = Catalog::from_bytes(&compiled_str).expect("from bytes");

        let path = unique_temp_path("mf2_whitespace");
        std::fs::write(&path, source).expect("write temp source");
        let catalog_file = Catalog::compile_file(&path).expect("compile_file");
        std::fs::remove_file(&path).expect("remove temp source");

        let mut fmt_direct = catalog_direct
            .formatter_for_locale(&locale("en-US"))
            .expect("fmt");
        let mut fmt_str = catalog_str
            .formatter_for_locale(&locale("en-US"))
            .expect("fmt");
        let mut fmt_file = catalog_file
            .formatter_for_locale(&locale("en-US"))
            .expect("fmt");
        let args = MessageArgs::new();
        let out_direct = fmt_direct
            .format_by_id("main", &args)
            .expect("format direct");
        let out_str = fmt_str
            .format_by_id("main", &args)
            .expect("format compile_str");
        let out_file = fmt_file.format_by_id("main", &args).expect("format file");

        assert_eq!(out_direct, "  hello  ");
        assert_eq!(out_str, "  hello  ");
        assert_eq!(out_file, "  hello  ");
    }

    #[cfg(all(feature = "compile", feature = "std"))]
    #[test]
    fn compile_file_missing_path_returns_io_error() {
        let path = unique_temp_path("mf2_missing_compile_file_test_should_not_exist");
        let _ = std::fs::remove_file(&path);
        let err = Catalog::compile_file(&path).unwrap_err();
        match err {
            compiler::CompileError::IoError { path: got, source } => {
                assert_eq!(got, path);
                assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
            }
            other => panic!("expected IoError, got: {other:?}"),
        }
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn compile_resources_merges_named_message_bodies() {
        let (catalog, source_map) = Catalog::compile_resources(
            [compiler::ResourceInput::new(
                "app.toml",
                compiler::SourceKind::Other(String::from("resource-toml")),
            )
            .message("hello", "Hello")
            .message("bye", "Bye")],
            compiler::CompileOptions::default(),
        )
        .expect("compile");

        assert_eq!(source_map.sources.len(), 1);
        assert!(catalog.resolve("hello").is_ok());
        assert!(catalog.resolve("bye").is_ok());
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn compile_with_manifest_validates_custom_functions() {
        let mut manifest = compiler::FunctionManifest::new();
        manifest.insert(compiler::FunctionSchema::new("custom:format").allow_format());

        let err = Catalog::compile_with_manifest(
            "{ $value :custom:missing }",
            compiler::CompileOptions::default(),
            &manifest,
        )
        .expect_err("must fail");

        match err {
            compiler::CompileError::UnknownFunction { function, .. } => {
                assert_eq!(function, "custom:missing");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn bundle_lookup_falls_back_to_parent_locale_catalog() {
        let bundle = CatalogBundle::new(
            [
                localized("fr", "Salut { $name }"),
                localized("en", "Hello { $name }"),
            ],
            &locale("fr-CA"),
        )
        .expect("bundle");

        let mut formatter = bundle.formatter().expect("lookup formatter");

        let mut args = MessageArgs::new();
        args.insert("name", "Ada");
        assert_eq!(
            formatter.format_by_id("main", &args).expect("format"),
            "Salut Ada"
        );
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn bundle_returns_exact_match_when_available() {
        let bundle = CatalogBundle::new(
            [localized("en", "Hello"), localized("fr", "Bonjour")],
            &locale("fr"),
        )
        .expect("bundle");

        let mut formatter = bundle.formatter().expect("formatter");
        let args = MessageArgs::new();
        assert_eq!(
            formatter.format_by_id("main", &args).expect("format"),
            "Bonjour"
        );
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn bundle_formatter_resolves_named_args_against_active_catalog() {
        let bundle = CatalogBundle::new(
            [
                localized("en", "Hello { $name }"),
                localized("fr", "Salut { $given } { $name }"),
            ],
            &locale("en-AU"),
        )
        .expect("bundle");

        let mut formatter = bundle.formatter().expect("lookup formatter");

        let mut args = MessageArgs::new();
        args.insert("name", "Ada");

        assert_eq!(
            formatter.format_by_id("main", &args).expect("format"),
            "Hello Ada"
        );
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn empty_bundle_reports_missing_locale_catalog() {
        let err = CatalogBundle::new(core::iter::empty::<LocalizedCatalog>(), &locale("en"))
            .expect_err("must fail");
        assert_eq!(err, FormatError::Trap(Trap::MissingLocaleCatalog));
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn bundle_lookup_uses_cldr_parent_locale() {
        // pt-MZ has CLDR parent pt-PT, not pt (naive truncation would skip pt-PT)
        let bundle = CatalogBundle::new(
            [
                localized("pt-PT", "Olá { $name }"),
                localized("pt", "Oi { $name }"),
            ],
            &locale("pt-MZ"),
        )
        .expect("bundle");

        let mut formatter = bundle.formatter().expect("lookup formatter");

        let mut args = MessageArgs::new();
        args.insert("name", "Ada");
        assert_eq!(
            formatter.format_by_id("main", &args).expect("format"),
            "Olá Ada"
        );
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn bundle_lookup_reports_missing_locale_when_no_catalog_matches() {
        let err = CatalogBundle::new([localized("fr", "Bonjour")], &locale("en-US"))
            .expect_err("must fail");
        assert_eq!(err, FormatError::Trap(Trap::MissingLocaleCatalog));
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    fn compile_messages(messages: &[(&str, &str)]) -> Catalog {
        let (catalog, _) = Catalog::compile_inputs(
            messages.iter().map(|(id, source)| compiler::CompileInput {
                name: id,
                message_id: id,
                source,
                kind: compiler::SourceKind::MessageFormat,
            }),
            compiler::CompileOptions::default(),
        )
        .expect("compile");
        catalog
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn bundle_message_level_fallback_across_catalogs() {
        // pt-MZ CLDR chain: pt-MZ → pt-PT → pt → und
        let bundle = CatalogBundle::new(
            [
                // pt-PT only has "greeting"
                LocalizedCatalog::new(locale("pt-PT"), compile_messages(&[("greeting", "Olá")])),
                // pt has both "greeting" and "farewell"
                LocalizedCatalog::new(
                    locale("pt"),
                    compile_messages(&[("greeting", "Oi"), ("farewell", "Tchau")]),
                ),
            ],
            &locale("pt-MZ"),
        )
        .expect("bundle");

        let mut formatter = bundle.formatter().expect("lookup formatter");
        let args = MessageArgs::new();

        // "greeting" found in pt-PT (first catalog in chain)
        assert_eq!(
            formatter.format_by_id("greeting", &args).expect("format"),
            "Olá"
        );
        // "farewell" not in pt-PT, falls back to pt (second catalog)
        assert_eq!(
            formatter.format_by_id("farewell", &args).expect("format"),
            "Tchau"
        );
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn bundle_fallback_resolves_args_against_matched_catalog() {
        let bundle = CatalogBundle::new(
            [
                // pt-PT has "greeting" (literal only — "recipient" is NOT interned)
                LocalizedCatalog::new(locale("pt-PT"), compile_messages(&[("greeting", "Olá")])),
                // pt has "farewell" which uses $recipient (interned in pt's string pool)
                LocalizedCatalog::new(
                    locale("pt"),
                    compile_messages(&[("farewell", "Adeus { $recipient }")]),
                ),
            ],
            &locale("pt-MZ"),
        )
        .expect("bundle");

        let mut formatter = bundle.formatter().expect("lookup formatter");
        let mut args = MessageArgs::new();
        args.insert("recipient", "Ada");

        // If args resolved against pt-PT (bug), "recipient" would not be
        // interned and silently dropped, producing fallback "{$recipient}".
        assert_eq!(
            formatter.format_by_id("farewell", &args).expect("format"),
            "Adeus Ada"
        );
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    fn compile_and_format(source: &str, args: &MessageArgs) -> String {
        let catalog = Catalog::compile_str(source).expect("compile");
        let mut formatter = catalog
            .formatter_for_locale(&locale("en-US"))
            .expect("formatter");
        formatter.format_by_id("main", args).expect("format")
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn compile_integer_large_float_uses_shortest_representation() {
        let args = MessageArgs::new();
        assert_eq!(
            compile_and_format("{1e23 :integer}", &args),
            "100000000000000000000000"
        );
        assert_eq!(
            compile_and_format("{1e24 :integer}", &args),
            "1000000000000000000000000"
        );
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn compile_number_large_float_uses_shortest_representation() {
        let args = MessageArgs::new();
        assert_eq!(
            compile_and_format("{1e23 :number}", &args),
            "100000000000000000000000"
        );
        assert_eq!(
            compile_and_format("{1e24 :number}", &args),
            "1000000000000000000000000"
        );
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn compile_number_negative_zero_sign_display() {
        let args = MessageArgs::new();
        assert_eq!(
            compile_and_format("{-0.0 :number signDisplay=always}", &args),
            "-0"
        );
        assert_eq!(
            compile_and_format("{-0.0 :number signDisplay=never}", &args),
            "0"
        );
        assert_eq!(
            compile_and_format("{-0.0 :number signDisplay=auto}", &args),
            "-0"
        );
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn compile_offset_large_negative_sign_display_never() {
        let args = MessageArgs::new();
        assert_eq!(
            compile_and_format("{-1e23 :offset subtract=1 signDisplay=never}", &args),
            "100000000000000000000000"
        );
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn borrowed_bundle_via_new() {
        let fr_catalog = Catalog::compile_str("Salut { $name }").expect("compile fr");
        let en_catalog = Catalog::compile_str("Hello { $name }").expect("compile en");

        let bundle = CatalogBundle::new(
            [
                LocalizedCatalog::new(locale("fr"), &fr_catalog),
                LocalizedCatalog::new(locale("en"), &en_catalog),
            ],
            &locale("fr-CA"),
        )
        .expect("bundle");

        let mut formatter = bundle.formatter().expect("formatter");
        let mut args = MessageArgs::new();
        args.insert("name", "Ada");
        assert_eq!(
            formatter.format_by_id("main", &args).expect("format"),
            "Salut Ada"
        );
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn borrowed_bundle_via_from_lookup() {
        let fr_catalog = Catalog::compile_str("Bonjour").expect("compile fr");
        let en_catalog = Catalog::compile_str("Hello").expect("compile en");

        let catalogs: alloc::vec::Vec<(Locale, &Catalog)> = alloc::vec![
            (locale("fr"), &fr_catalog),
            (locale("en"), &en_catalog),
        ];

        let bundle = CatalogBundle::from_lookup(&locale("fr"), |loc| {
            Ok::<_, core::convert::Infallible>(
                catalogs
                    .iter()
                    .find(|(l, _)| l == loc)
                    .map(|(_, c)| *c),
            )
        })
        .expect("bundle");

        let mut formatter = bundle.formatter().expect("formatter");
        let args = MessageArgs::new();
        assert_eq!(
            formatter.format_by_id("main", &args).expect("format"),
            "Bonjour"
        );
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn arc_bundle() {
        use alloc::sync::Arc;

        let fr_catalog = Arc::new(Catalog::compile_str("Salut { $name }").expect("compile fr"));
        let en_catalog = Arc::new(Catalog::compile_str("Hello { $name }").expect("compile en"));

        let bundle = CatalogBundle::new(
            [
                LocalizedCatalog::new(locale("fr"), Arc::clone(&fr_catalog)),
                LocalizedCatalog::new(locale("en"), Arc::clone(&en_catalog)),
            ],
            &locale("fr-CA"),
        )
        .expect("bundle");

        let mut formatter = bundle.formatter().expect("formatter");
        let mut args = MessageArgs::new();
        args.insert("name", "Ada");
        assert_eq!(
            formatter.format_by_id("main", &args).expect("format"),
            "Salut Ada"
        );
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn arc_bundle_into_owning_formatter() {
        use alloc::sync::Arc;

        fn make_formatter() -> MessageFormatter<IndexedCatalog<Arc<Catalog>>> {
            let fr = Arc::new(Catalog::compile_str("Salut { $name }").expect("compile fr"));
            let en = Arc::new(Catalog::compile_str("Hello { $name }").expect("compile en"));
            let bundle = CatalogBundle::new(
                [
                    LocalizedCatalog::new(locale("fr"), fr),
                    LocalizedCatalog::new(locale("en"), en),
                ],
                &locale("fr-CA"),
            )
            .expect("bundle");
            bundle.into_formatter().expect("formatter")
        }

        let mut formatter = make_formatter();
        let mut args = MessageArgs::new();
        args.insert("name", "Ada");
        assert_eq!(
            formatter.format_by_id("main", &args).expect("format"),
            "Salut Ada"
        );
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn bundle_clone_into_formatter_retains_bundle() {
        use alloc::sync::Arc;

        let fr = Arc::new(Catalog::compile_str("Salut { $name }").expect("compile fr"));
        let en = Arc::new(Catalog::compile_str("Hello { $name }").expect("compile en"));
        let bundle = CatalogBundle::new(
            [
                LocalizedCatalog::new(locale("fr"), Arc::clone(&fr)),
                LocalizedCatalog::new(locale("en"), Arc::clone(&en)),
            ],
            &locale("fr-CA"),
        )
        .expect("bundle");

        let mut owning = bundle.clone().into_formatter().expect("owning formatter");
        let mut borrowing = bundle.formatter().expect("borrowing formatter");

        let mut args = MessageArgs::new();
        args.insert("name", "Ada");
        assert_eq!(
            owning.format_by_id("main", &args).expect("owning format"),
            "Salut Ada"
        );
        assert_eq!(
            borrowing
                .format_by_id("main", &args)
                .expect("borrowing format"),
            "Salut Ada"
        );
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn for_locale_owning_arc() {
        use alloc::sync::Arc;

        let catalog = Arc::new(Catalog::compile_str("Hello { $name }").expect("compile"));
        let mut formatter =
            MessageFormatter::for_locale(catalog, &locale("en-US")).expect("formatter");

        let mut args = MessageArgs::new();
        args.insert("name", "World");
        assert_eq!(
            formatter.format_by_id("main", &args).expect("format"),
            "Hello World"
        );
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn for_locale_static_catalog() {
        use alloc::boxed::Box;

        let catalog: &'static Catalog = {
            let c = Catalog::compile_str("Static { $name }").expect("compile");
            Box::leak(Box::new(c))
        };

        fn needs_static(
            f: MessageFormatter<IndexedCatalog<&'static Catalog>>,
        ) -> MessageFormatter<IndexedCatalog<&'static Catalog>> {
            f
        }

        let formatter =
            MessageFormatter::for_locale(catalog, &locale("en-US")).expect("formatter");
        let mut formatter = needs_static(formatter);

        let mut args = MessageArgs::new();
        args.insert("name", "World");
        assert_eq!(
            formatter.format_by_id("main", &args).expect("format"),
            "Static World"
        );

        unsafe {
            let _ = Box::from_raw(catalog as *const Catalog as *mut Catalog);
        }
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn bundle_accessors() {
        use alloc::sync::Arc;

        let fr = Arc::new(Catalog::compile_str("Salut").expect("compile fr"));
        let en = Arc::new(Catalog::compile_str("Hello").expect("compile en"));
        let bundle = CatalogBundle::new(
            [
                LocalizedCatalog::new(locale("fr"), Arc::clone(&fr)),
                LocalizedCatalog::new(locale("en"), Arc::clone(&en)),
            ],
            &locale("fr-CA"),
        )
        .expect("bundle");

        assert!(!bundle.candidates().is_empty());
        assert_eq!(bundle.catalogs().len(), 1);
    }

    /// One `Arc<IndexedCatalog>` (an `und` catalog with a plural-select
    /// message, exercising `by_id`, option keys, and `category_pool_ids`)
    /// shared across `en` and `fr` bundles: the same allocation is reused —
    /// each formatter adds one strong reference instead of re-indexing — and
    /// output matches the auto-index path.
    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn shared_indexed_und_catalog_across_locale_bundles() {
        use alloc::sync::Arc;

        let und_catalog = compile_messages(&[(
            "items",
            ".input {$count :number select=plural}\n.match $count\none {{one item}}\n* {{many items}}",
        )]);

        // Auto-index reference path: plain catalog, indexed inside the bundle.
        let plain_bundle = CatalogBundle::new(
            [LocalizedCatalog::new(locale("und"), &und_catalog)],
            &locale("en"),
        )
        .expect("plain bundle");
        let mut plain = plain_bundle.formatter().expect("plain formatter");

        let shared: Arc<IndexedCatalog> = Arc::new(
            und_catalog
                .clone()
                .into_indexed_catalog()
                .expect("index once"),
        );
        let base = Arc::strong_count(&shared);

        let en_bundle = CatalogBundle::new(
            [LocalizedCatalog::new(locale("und"), Arc::clone(&shared))],
            &locale("en"),
        )
        .expect("en bundle");
        let fr_bundle = CatalogBundle::new(
            [LocalizedCatalog::new(locale("und"), Arc::clone(&shared))],
            &locale("fr"),
        )
        .expect("fr bundle");
        // Pass-through: each bundle holds a clone of the same allocation.
        assert_eq!(Arc::strong_count(&shared), base + 2);

        let mut en = en_bundle.into_formatter().expect("en formatter");
        let mut fr = fr_bundle.into_formatter().expect("fr formatter");
        // The formatters take over the bundles' references — still no rebuild.
        assert_eq!(Arc::strong_count(&shared), base + 2);

        let mut args = MessageArgs::new();
        args.insert("count", 1);
        assert_eq!(en.format_by_id("items", &args).expect("en"), "one item");
        assert_eq!(
            en.format_by_id("items", &args).expect("en"),
            plain.format_by_id("items", &args).expect("plain")
        );
        args.insert("count", 2);
        assert_eq!(en.format_by_id("items", &args).expect("en"), "many items");
        assert_eq!(fr.format_by_id("items", &args).expect("fr"), "many items");

        drop((en, fr));
        assert_eq!(Arc::strong_count(&shared), base);
    }

    /// `from_lookup` closures can hand out `Arc<IndexedCatalog>` clones from a
    /// cache; `into_formatter` keeps the shared index and is `'static`-capable.
    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn from_lookup_shared_indexed_owning_formatter() {
        use alloc::sync::Arc;

        let cache: Arc<IndexedCatalog> = Arc::new(
            IndexedCatalog::new(Catalog::compile_str("Hello { $name }").expect("compile"))
                .expect("index"),
        );

        fn make_formatter(
            cache: &Arc<IndexedCatalog>,
        ) -> MessageFormatter<Arc<IndexedCatalog>> {
            let en = "en".parse::<Locale>().expect("locale");
            let bundle = CatalogBundle::from_lookup(&en, |loc| {
                Ok::<_, core::convert::Infallible>((*loc == en).then(|| Arc::clone(cache)))
            })
            .expect("bundle");
            bundle.into_formatter().expect("formatter")
        }

        let base = Arc::strong_count(&cache);
        let mut formatter = make_formatter(&cache);
        assert_eq!(Arc::strong_count(&cache), base + 1);

        let mut args = MessageArgs::new();
        args.insert("name", "Ada");
        assert_eq!(
            formatter.format_by_id("main", &args).expect("format"),
            "Hello Ada"
        );
    }

    /// Pre-indexed catalogs can be passed to bundles by reference too.
    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn bundle_accepts_pre_indexed_borrow() {
        let fr = Catalog::compile_str("Salut { $name }").expect("compile");
        let fr_indexed = (&fr).into_indexed_catalog().expect("index");

        let bundle = CatalogBundle::new(
            [LocalizedCatalog::new(locale("fr"), &fr_indexed)],
            &locale("fr"),
        )
        .expect("bundle");
        let mut formatter = bundle.formatter().expect("formatter");

        let mut args = MessageArgs::new();
        args.insert("name", "Ada");
        assert_eq!(
            formatter.format_by_id("main", &args).expect("format"),
            "Salut Ada"
        );
    }

    /// Facade convenience: mint per-locale formatters straight off a cached
    /// `IndexedCatalog` without re-indexing.
    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn indexed_catalog_formatter_for_locale() {
        let indexed = Catalog::compile_str("Hello { $name }")
            .expect("compile")
            .into_indexed_catalog()
            .expect("index");

        let mut args = MessageArgs::new();
        args.insert("name", "Ada");
        for tag in ["en-US", "fr"] {
            let mut formatter = indexed
                .formatter_for_locale(&locale(tag))
                .expect("formatter");
            assert_eq!(
                formatter.format_by_id("main", &args).expect("format"),
                "Hello Ada"
            );
        }
    }

    #[cfg(all(feature = "compile", feature = "icu4x"))]
    #[test]
    fn formatter_host_locale_independent_of_catalog() {
        // Compile a catalog with a bare expression (no :number annotation).
        // Float values go through BuiltinHost::format_default which is
        // locale-sensitive.
        let catalog = Catalog::compile_str("{ $n }").expect("compile");

        // Create a formatter with host locale "fr" (French formatting uses
        // comma as decimal separator) — the catalog itself has no locale.
        let candidates = locale_candidates(&locale("fr"));
        let indexed = IndexedCatalog::new(&catalog).expect("index");
        let mut formatter =
            MessageFormatter::new(core::iter::once(&indexed), &candidates).expect("formatter");

        let mut args = MessageArgs::new();
        args.insert("n", 123.5);

        let result = formatter.format_by_id("main", &args).expect("format");

        // French replaces '.' with ',' → "123,5".
        // English would produce "123.5" (period decimal).
        assert_eq!(result, "123,5");
    }
}
