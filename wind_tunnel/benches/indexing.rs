// Copyright 2026 the Message Format Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#![allow(
    missing_docs,
    reason = "Criterion-generated benchmark entry points are private harness glue."
)]

//! Cost of building per-catalog host indexes, and what sharing them saves.
//!
//! - `index_build/{n}`: one `IndexedCatalog::new` scan — the work amortized
//!   by sharing.
//! - `all_locales_projection/{fresh,shared}`: minting one formatter per
//!   locale from a plain catalog (re-index each time) vs from a shared
//!   `Arc<IndexedCatalog>` (index built once) — the headline number.
//! - `builtin_host_new/{locale}`: the per-locale host cost that remains on
//!   the shared path.

use core::hint::black_box;
use std::sync::Arc;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use icu_locale_core::Locale;
use message_format::compiler::{
    CompileOptions, MessageResource, ResourceInput, SourceKind, compile_resources,
};
use message_format::runtime::{BuiltinHost, Catalog};
use message_format::{IndexedCatalog, IntoIndexedCatalog};

/// Mixed-shape catalog mirroring `loading.rs`'s generator: plain
/// interpolation, string/plural selection, locals — so the index scan sees
/// realistic pool strings and FUNC entries.
fn build_mixed_catalog(message_count: usize) -> Catalog {
    let mut input = ResourceInput::new("bench-indexing", SourceKind::Generated);
    for index in 0..message_count {
        let (id, source) = match index % 5 {
            0 => (
                format!("price_{index}"),
                format!("Total {index}: {{ $amount :number minimumFractionDigits=2 }}"),
            ),
            1 => (
                format!("greeting_{index}"),
                format!(
                    ".input {{ $tone :string }}\
                     \n.match $tone\
                     \nformal {{{{Dear colleague {index}}}}}\
                     \ncasual {{{{Hey there {index}}}}}\
                     \nfriendly {{{{Hi friend {index}}}}}\
                     \n* {{{{Hello {index}}}}}"
                ),
            ),
            2 => (
                format!("count_{index}"),
                format!(
                    ".input {{ $count :number select=plural }}\
                     \n.match $count\
                     \none {{{{{index} item}}}}\
                     \nother {{{{{index} items}}}}\
                     \n* {{{{{index} items}}}}"
                ),
            ),
            3 => (
                format!("invoice_{index}"),
                format!(
                    ".local $total = {{ $raw :number minimumFractionDigits=2 }}\
                     \n{{{{Invoice {index}: {{ $total }}}}}}"
                ),
            ),
            4 => (
                format!("status_{index}"),
                format!(
                    ".local $state = {{ $input :string }}\
                     \n.match $state\
                     \nactive {{{{Active {index}}}}}\
                     \ninactive {{{{Inactive {index}}}}}\
                     \n* {{{{Unknown {index}}}}}"
                ),
            ),
            _ => unreachable!(),
        };
        input.resources.push(MessageResource::new(id, source));
    }
    let bytes = compile_resources([input], CompileOptions::default())
        .into_result()
        .expect("mixed catalog compiles")
        .bytes;
    Catalog::from_bytes(&bytes).expect("valid benchmark catalog")
}

fn locale(tag: &str) -> Locale {
    tag.parse::<Locale>().expect("locale")
}

const PROJECTION_LOCALES: [&str; 10] = [
    "en", "en-GB", "fr", "de", "ja", "ar-EG", "pt-BR", "ru", "zh", "es",
];

fn bench_index_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("index_build");
    for message_count in [10_usize, 100, 1_000, 10_000] {
        let catalog = build_mixed_catalog(message_count);
        group.throughput(Throughput::Elements(
            u64::try_from(message_count).expect("message count fits in u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(message_count),
            &catalog,
            |b, catalog| {
                b.iter(|| IndexedCatalog::new(black_box(catalog)).expect("index builds"));
            },
        );
    }
    group.finish();
}

fn bench_all_locales_projection(c: &mut Criterion) {
    let catalog = build_mixed_catalog(1_000);
    let locales: Vec<Locale> = PROJECTION_LOCALES.iter().map(|tag| locale(tag)).collect();

    let mut group = c.benchmark_group("all_locales_projection");
    group.throughput(Throughput::Elements(
        u64::try_from(locales.len()).expect("locale count fits in u64"),
    ));

    // Today's path: every per-locale formatter re-indexes the catalog.
    group.bench_function("fresh", |b| {
        b.iter(|| {
            for locale in &locales {
                let formatter = catalog
                    .formatter_for_locale(black_box(locale))
                    .expect("formatter");
                black_box(formatter);
            }
        });
    });

    // Shared path: the catalog is indexed once; each locale only builds a host.
    group.bench_function("shared", |b| {
        let shared: Arc<IndexedCatalog<&Catalog>> =
            Arc::new((&catalog).into_indexed_catalog().expect("index"));
        b.iter(|| {
            for locale in &locales {
                let formatter = shared
                    .formatter_for_locale(black_box(locale))
                    .expect("formatter");
                black_box(formatter);
            }
        });
    });

    group.finish();
}

fn bench_builtin_host_new(c: &mut Criterion) {
    let mut group = c.benchmark_group("builtin_host_new");
    group.throughput(Throughput::Elements(1));
    for tag in ["en", "ar-EG", "und"] {
        let locale = locale(tag);
        group.bench_with_input(BenchmarkId::from_parameter(tag), &locale, |b, locale| {
            b.iter(|| BuiltinHost::new(black_box(locale)).expect("host"));
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_index_build,
    bench_all_locales_projection,
    bench_builtin_host_new
);
criterion_main!(benches);
