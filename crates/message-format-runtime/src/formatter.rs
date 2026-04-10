// Copyright 2026 the Message Format Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! High-level formatting wrappers around the bytecode VM.

use alloc::vec::Vec;
use core::fmt;

use crate::{
    catalog::Catalog,
    error::FormatError,
    value::{Args, Value},
    vm::{FormatSink, Host, MessageHandle, VecDiagnostics, run_bytecode},
};

#[derive(Default)]
pub(crate) struct VmState {
    pub(crate) fuel: Option<u64>,
    pub(crate) stack: Vec<Value>,
    pub(crate) call_args: Vec<Value>,
    pub(crate) call_options: Vec<(u32, Value)>,
}

/// Formatter executes catalog messages with caller-provided arguments and host functions.
///
/// Catalogs are expected to come from the compiler or prebuilt assets. This
/// example assumes a loaded catalog whose `"main"` message invokes a host
/// function and formats its result.
///
/// ```rust,no_run
/// use message_format_runtime::{Catalog, FormatError, Formatter, HostFn, HostCallError, Value};
///
/// # fn render(catalog: &Catalog) -> Result<String, FormatError> {
/// let host = HostFn(|_fn_id, _args, _opts| Ok(Value::Str("called".to_string())));
/// let mut formatter = Formatter::new(&catalog, host)?;
/// let message = formatter.resolve("main")?;
/// struct StringSink<'a>(&'a mut String);
/// impl message_format_runtime::FormatSink for StringSink<'_> {
///     fn literal(&mut self, s: &str) { self.0.push_str(s); }
///     fn expression(&mut self, s: &str) { self.0.push_str(s); }
///     fn markup_open(&mut self, _name: &str, _options: &[message_format_runtime::FormatOption<'_>]) {}
///     fn markup_close(&mut self, _name: &str, _options: &[message_format_runtime::FormatOption<'_>]) {}
/// }
/// let mut out = String::new();
/// let mut sink = StringSink(&mut out);
/// let _errors = formatter
///     .format_to(message, &Vec::<(u32, Value)>::new(), &mut sink)
///     ?;
/// # Ok(out)
/// # }
/// ```
pub struct Formatter<'a, H: Host> {
    catalog: &'a Catalog,
    index: H::CatalogIndex,
    host: H,
    vm: VmState,
}

impl<H: Host> fmt::Debug for Formatter<'_, H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Formatter")
            .field("catalog", &self.catalog)
            .finish_non_exhaustive()
    }
}

impl<'a, H: Host> Formatter<'a, H> {
    /// Create a formatter for a loaded catalog.
    ///
    /// Calls [`Host::index`] to pre-compute catalog-specific data.
    pub fn new(catalog: &'a Catalog, mut host: H) -> Result<Self, FormatError> {
        let index = host.index(catalog)?;
        Ok(Self {
            catalog,
            index,
            host,
            vm: VmState::default(),
        })
    }

    /// Set the maximum number of instructions the VM may execute per message.
    ///
    /// When the budget is exhausted, formatting returns
    /// [`FormatError::Trap`]. Pass `None` for unlimited execution (the
    /// default). Use this to defend against denial-of-service from untrusted
    /// catalogs that may contain infinite loops.
    pub fn set_fuel(&mut self, fuel: Option<u64>) {
        self.vm.fuel = fuel;
    }

    /// Resolve a message id to a reusable handle.
    pub fn resolve(&self, message_id: &str) -> Result<MessageHandle, FormatError> {
        MessageHandle::from_catalog(self.catalog, message_id)
    }

    /// Format one message from a previously resolved handle, dispatching events to a [`FormatSink`].
    ///
    /// Returns recoverable formatting diagnostics collected during fallback
    /// rendering. Fatal execution failures are returned as `Err`.
    ///
    /// This is the runtime API that preserves structured markup. In contrast,
    /// string-oriented convenience helpers flatten only literal/expression text
    /// and drop markup events.
    pub fn format_to<S: FormatSink + ?Sized>(
        &mut self,
        message: MessageHandle,
        args: &dyn Args,
        sink: &mut S,
    ) -> Result<Vec<FormatError>, FormatError> {
        let mut diagnostics = VecDiagnostics::default();
        run_bytecode(
            self.catalog,
            &mut self.host,
            &self.index,
            message.entry_pc,
            args,
            self.vm.fuel,
            &mut self.vm.stack,
            sink,
            Some(&mut diagnostics),
            &mut self.vm.call_args,
            &mut self.vm.call_options,
        )?;
        Ok(diagnostics.into_inner())
    }
}
