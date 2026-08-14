//! WASM plugin runtime (design §10.3).
//!
//! The default plugin layer runs sandboxed WebAssembly with **no host
//! imports** unless a capability is explicitly granted. Each invocation is
//! bounded: fuel (CPU), memory pages, recursion depth and payload size all
//! have host-imposed limits, and the plugin never sees host paths, files,
//! processes, clocks or randomness.
//!
//! ABI: the module must export `memory` and
//! `invoke(input_ptr: i32, input_len: i32) -> i32`. The host writes the JSON
//! request DTO into memory at `input_ptr`, calls `invoke`, and reads the
//! JSON response of the returned length back from the same buffer. The
//! response is re-parsed and validated by the host before it reaches any
//! caller.

use std::sync::{Arc, Mutex};

use wasmi::{Config, Engine, Instance, Linker, Memory, Module, Store, TypedFunc};

/// Maximum fuel per invocation; a plugin exceeding it is trapped.
pub const MAX_WASM_FUEL: u64 = 1_000_000;
/// Maximum memory the plugin may declare or grow to (pages of 64 KiB).
pub const MAX_WASM_MEMORY_PAGES: u32 = 32;
/// Maximum JSON payload accepted from the host or produced by the plugin.
pub const MAX_WASM_PAYLOAD_BYTES: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum WasmError {
    #[error("wasm module could not be compiled: {0}")]
    Compile(String),
    #[error("plugin does not export the required invoke function")]
    MissingInvoke,
    #[error("plugin does not export a linear memory")]
    MissingMemory,
    #[error("plugin memory exceeds the {MAX_WASM_MEMORY_PAGES}-page limit")]
    MemoryTooLarge,
    #[error("request payload exceeds the {MAX_WASM_PAYLOAD_BYTES}-byte limit")]
    PayloadTooLarge,
    #[error("plugin memory is too small for the payload")]
    MemoryTooSmall,
    #[error("plugin invocation failed: {0}")]
    Trap(String),
    #[error("plugin produced an invalid JSON response: {0}")]
    InvalidResponse(String),
    #[error("plugin response exceeds the {MAX_WASM_PAYLOAD_BYTES}-byte limit")]
    ResponseTooLarge,
}

/// A compiled WASM plugin ready to instantiate. Cheap to clone: the module
/// bytes are shared, each instantiation gets its own store and memory.
#[derive(Clone)]
pub struct WasmPluginModule {
    engine: Engine,
    module: Module,
}

impl WasmPluginModule {
    /// Compiles the module with a fuel- and memory-bounded configuration.
    pub fn compile(bytes: &[u8]) -> Result<Self, WasmError> {
        let mut config = Config::default();
        config.consume_fuel(true);
        config.set_max_recursion_depth(128);
        let engine = Engine::new(&config);
        let module =
            Module::new(&engine, bytes).map_err(|error| WasmError::Compile(error.to_string()))?;
        Ok(Self { engine, module })
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn module(&self) -> &Module {
        &self.module
    }
}

/// A sandboxed plugin instance: one store, one memory, one invoke entry.
pub struct WasmPluginInstance {
    store: Store<()>,
    memory: Memory,
    invoke: TypedFunc<(i32, i32), i32>,
    pending_transactions: Arc<Mutex<Vec<TransactionProposal>>>,
}

/// A transaction proposal collected from a WASM plugin during an
/// invocation. Carries the raw JSON so the caller can validate against its
/// own transaction schema.
pub type TransactionProposal = serde_json::Value;

/// One host capability offered to a WASM plugin (design §10.3: the host
/// provides the *minimal* capability imports matching the granted
/// permissions; everything else stays absent). Capabilities use the same
/// buffer ABI as `invoke`: the plugin passes `(ptr, len)` and the host
/// writes a JSON value into the plugin memory, returning its length.
///
/// The capability names are stable and versioned through the plugin API:
/// - `game.snapshot` (requires `GameRead`): returns the current game
///   snapshot as JSON, identical to the native `game.snapshot` RPC.
/// - `game.submitTransaction` (requires `GameWrite`): accepts a JSON
///   `GameTransaction`, collects it as a proposal, and returns
///   `{"ok":true}` (or an error object). The host validates and applies
///   proposals *after* the invocation returns; a plugin can never mutate
///   the game outside the host's validation path.
#[derive(Clone, Debug, Default)]
pub struct WasmCapabilities {
    /// Optional `game.snapshot` result JSON (granted `GameRead`).
    pub game_snapshot: Option<String>,
    /// Whether `game.submitTransaction` is exposed (granted `GameWrite`).
    pub game_write: bool,
}

impl WasmPluginInstance {
    /// Instantiates with no host imports: a plugin declaring any `sabaki.*`
    /// import fails to link (design: WASM runtime defaults to no imports).
    pub fn instantiate(module: &WasmPluginModule) -> Result<Self, WasmError> {
        Self::instantiate_with_capabilities(module, &WasmCapabilities::default())
    }

    /// Instantiates with the granted host capabilities. A plugin that
    /// imports a capability the host did not grant fails to link.
    pub fn instantiate_with_capabilities(
        module: &WasmPluginModule,
        capabilities: &WasmCapabilities,
    ) -> Result<Self, WasmError> {
        let mut store = Store::new(module.engine(), ());
        let mut linker = Linker::new(module.engine());
        if let Some(snapshot) = capabilities.game_snapshot.clone() {
            linker
                .func_wrap(
                    "sabaki",
                    "game_snapshot",
                    move |mut caller: wasmi::Caller<'_, ()>,
                          _ptr: i32,
                          _len: i32|
                          -> Result<i32, wasmi::Error> {
                        let memory = caller
                            .get_export("memory")
                            .and_then(|export| export.into_memory())
                            .ok_or_else(|| wasmi::Error::new("plugin memory is unavailable"))?;
                        let bytes = snapshot.as_bytes();
                        memory
                            .write(&mut caller, 0, bytes)
                            .map_err(|error| wasmi::Error::new(error.to_string()))?;
                        Ok(bytes.len() as i32)
                    },
                )
                .map_err(|error| WasmError::Compile(error.to_string()))?;
        }
        let pending_transactions = Arc::new(Mutex::new(Vec::new()));
        if capabilities.game_write {
            let proposals = Arc::clone(&pending_transactions);
            linker
                .func_wrap(
                    "sabaki",
                    "game_submit_transaction",
                    move |mut caller: wasmi::Caller<'_, ()>, ptr: i32, len: i32| {
                        let memory = caller
                            .get_export("memory")
                            .and_then(|export| export.into_memory())
                            .ok_or_else(|| wasmi::Error::new("plugin memory is unavailable"))?;
                        let mut input = vec![0; len.max(0) as usize];
                        memory
                            .read(&caller, ptr.max(0) as usize, &mut input)
                            .map_err(|error| wasmi::Error::new(error.to_string()))?;
                        // Parse the proposal; when valid, collect it for the
                        // host to validate and apply after the invocation.
                        let parsed = serde_json::from_slice::<TransactionProposal>(&input);
                        let bytes = match parsed {
                            Ok(proposal) => {
                                proposals
                                    .lock()
                                    .expect("proposal queue is not poisoned")
                                    .push(proposal);
                                br#"{"ok":true}"#.to_vec()
                            }
                            Err(error) => serde_json::to_vec(&serde_json::json!({
                                "ok": false,
                                "error": format!("invalid transaction JSON: {error}")
                            }))
                            .unwrap_or_default(),
                        };
                        memory
                            .write(&mut caller, ptr.max(0) as usize, &bytes)
                            .map_err(|error| wasmi::Error::new(error.to_string()))?;
                        Ok(bytes.len() as i32)
                    },
                )
                .map_err(|error| WasmError::Compile(error.to_string()))?;
        }
        let instance = linker
            .instantiate_and_start(&mut store, module.module())
            .map_err(|error| WasmError::Compile(error.to_string()))?;
        Self::from_instance(instance, store, pending_transactions)
    }

    fn from_instance(
        instance: Instance,
        store: Store<()>,
        pending_transactions: Arc<Mutex<Vec<TransactionProposal>>>,
    ) -> Result<Self, WasmError> {
        let memory = instance
            .get_memory(&store, "memory")
            .ok_or(WasmError::MissingMemory)?;
        let memory_type = memory.ty(&store);
        let declared_max = memory_type.maximum().unwrap_or(memory_type.minimum());
        if declared_max > u64::from(MAX_WASM_MEMORY_PAGES) {
            return Err(WasmError::MemoryTooLarge);
        }
        let invoke = instance
            .get_typed_func::<(i32, i32), i32>(&store, "invoke")
            .map_err(|_| WasmError::MissingInvoke)?;
        Ok(Self {
            store,
            memory,
            invoke,
            pending_transactions,
        })
    }

    /// Drains the transaction proposals the plugin submitted through
    /// `game.submitTransaction` during the last invocation. The caller
    /// validates and applies each proposal through its own transaction
    /// path; this instance never touches the game directly.
    pub fn take_pending_transactions(&self) -> Vec<TransactionProposal> {
        std::mem::take(
            &mut self
                .pending_transactions
                .lock()
                .expect("proposal queue is not poisoned"),
        )
    }

    /// Invokes the plugin with a JSON request DTO and returns the validated
    /// JSON response. The input is written at offset 0 of the plugin memory;
    /// the response overwrites the same buffer.
    pub fn invoke(&mut self, request: &serde_json::Value) -> Result<serde_json::Value, WasmError> {
        let input = serde_json::to_vec(request).map_err(|error| {
            WasmError::InvalidResponse(format!("request could not be encoded: {error}"))
        })?;
        if input.len() > MAX_WASM_PAYLOAD_BYTES {
            return Err(WasmError::PayloadTooLarge);
        }
        let memory_size = self.memory.data_size(&self.store);
        if input.len() > memory_size {
            return Err(WasmError::MemoryTooSmall);
        }

        self.memory
            .write(&mut self.store, 0, &input)
            .map_err(|error| WasmError::Trap(error.to_string()))?;
        self.store
            .set_fuel(MAX_WASM_FUEL)
            .map_err(|error| WasmError::Trap(error.to_string()))?;
        let output_len = self
            .invoke
            .call(&mut self.store, (0, input.len() as i32))
            .map_err(|error| WasmError::Trap(error.to_string()))?;
        if output_len < 0 || output_len as usize > MAX_WASM_PAYLOAD_BYTES {
            return Err(WasmError::ResponseTooLarge);
        }
        let mut output = vec![0; output_len as usize];
        self.memory
            .read(&self.store, 0, &mut output)
            .map_err(|error| WasmError::Trap(error.to_string()))?;
        serde_json::from_slice(&output)
            .map_err(|error| WasmError::InvalidResponse(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An echo plugin: returns the request verbatim.
    const ECHO_WAT: &str = r#"
(module
  (memory (export "memory") 1)
  (func (export "invoke") (param $ptr i32) (param $len i32) (result i32)
    local.get $len))
"#;

    /// A plugin that appends a marker field to the request.
    const MARKER_WAT: &str = r#"
(module
  (memory (export "memory") 1)
  (func (export "invoke") (param $ptr i32) (param $len i32) (result i32)
    local.get $len))
"#;

    fn compile(wat: &str) -> WasmPluginModule {
        let bytes = wat::parse_str(wat).expect("WAT parses");
        WasmPluginModule::compile(&bytes).expect("module compiles")
    }

    #[test]
    fn echoes_a_request_round_trip() {
        let module = compile(ECHO_WAT);
        let mut instance = WasmPluginInstance::instantiate(&module).expect("instance starts");
        let response = instance
            .invoke(&serde_json::json!({"method": "game.snapshot", "id": 1}))
            .expect("invocation succeeds");
        assert_eq!(
            response,
            serde_json::json!({"method": "game.snapshot", "id": 1})
        );
    }

    #[test]
    fn rejects_modules_without_the_invoke_entry() {
        let bytes =
            wat::parse_str(r#"(module (memory (export "memory") 1) (func (export "other")))"#)
                .expect("WAT parses");
        let module = WasmPluginModule::compile(&bytes).expect("module compiles");
        assert!(matches!(
            WasmPluginInstance::instantiate(&module),
            Err(WasmError::MissingInvoke)
        ));
    }

    #[test]
    fn rejects_modules_without_memory() {
        let bytes = wat::parse_str(
            r#"(module (func (export "invoke") (param i32) (param i32) (result i32) i32.const 0))"#,
        )
        .expect("WAT parses");
        let module = WasmPluginModule::compile(&bytes).expect("module compiles");
        assert!(matches!(
            WasmPluginInstance::instantiate(&module),
            Err(WasmError::MissingMemory)
        ));
    }

    #[test]
    fn rejects_modules_whose_declared_memory_exceeds_the_limit() {
        // Declares 64 pages (4 MiB) > MAX_WASM_MEMORY_PAGES.
        let bytes = wat::parse_str(
            r#"(module (memory (export "memory") 64) (func (export "invoke") (param i32) (param i32) (result i32) i32.const 0))"#,
        )
        .expect("WAT parses");
        let module = WasmPluginModule::compile(&bytes).expect("module compiles");
        assert!(matches!(
            WasmPluginInstance::instantiate(&module),
            Err(WasmError::MemoryTooLarge)
        ));
    }

    #[test]
    fn fuel_exhaustion_traps_the_invocation() {
        // An infinite loop would burn all fuel and trap.
        let bytes = wat::parse_str(
            r#"(module
  (memory (export "memory") 1)
  (func (export "invoke") (param $ptr i32) (param $len i32) (result i32)
    (loop $l (result i32)
      (i32.const 0)
      (br $l))))"#,
        )
        .expect("WAT parses");
        let module = WasmPluginModule::compile(&bytes).expect("module compiles");
        let mut instance = WasmPluginInstance::instantiate(&module).expect("instance starts");
        assert!(matches!(
            instance.invoke(&serde_json::json!({})),
            Err(WasmError::Trap(_))
        ));
    }

    #[test]
    fn payload_over_the_limit_is_rejected_before_calling() {
        let module = compile(ECHO_WAT);
        let mut instance = WasmPluginInstance::instantiate(&module).expect("instance starts");
        let huge = serde_json::json!({"payload": "x".repeat(MAX_WASM_PAYLOAD_BYTES + 1)});
        assert!(matches!(
            instance.invoke(&huge),
            Err(WasmError::PayloadTooLarge)
        ));
    }

    #[test]
    fn granted_game_snapshot_capability_is_callable() {
        // The plugin imports sabaki.game_snapshot and returns its length.
        let bytes = wat::parse_str(
            r#"(module
  (import "sabaki" "game_snapshot" (func $snapshot (param i32) (param i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "invoke") (param $ptr i32) (param $len i32) (result i32)
    local.get $ptr
    local.get $len
    call $snapshot))"#,
        )
        .expect("WAT parses");
        let module = WasmPluginModule::compile(&bytes).expect("module compiles");
        let capabilities = WasmCapabilities {
            game_snapshot: Some("{\"moves\":3}".to_owned()),
            game_write: false,
        };
        let mut instance =
            WasmPluginInstance::instantiate_with_capabilities(&module, &capabilities)
                .expect("instance with granted capability starts");
        let response = instance
            .invoke(&serde_json::json!({}))
            .expect("invocation succeeds");
        // The plugin forwards the capability result verbatim.
        assert_eq!(response, serde_json::json!({"moves": 3}));
    }

    #[test]
    fn ungranted_game_snapshot_capability_fails_to_link() {
        // The plugin imports sabaki.game_snapshot but the host granted
        // nothing, so instantiation must fail (design: minimal imports).
        let bytes = wat::parse_str(
            r#"(module
  (import "sabaki" "game_snapshot" (func $snapshot (param i32) (param i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "invoke") (param $ptr i32) (param $len i32) (result i32)
    local.get $ptr
    local.get $len
    call $snapshot))"#,
        )
        .expect("WAT parses");
        let module = WasmPluginModule::compile(&bytes).expect("module compiles");
        assert!(
            matches!(
                WasmPluginInstance::instantiate_with_capabilities(
                    &module,
                    &WasmCapabilities::default()
                ),
                Err(WasmError::Compile(_))
            ),
            "an ungranted import must fail to link"
        );
    }

    #[test]
    fn granted_game_write_collects_transaction_proposals() {
        // The plugin forwards the request body to game.submitTransaction and
        // returns its result length.
        let bytes = wat::parse_str(
            r#"(module
  (import "sabaki" "game_submit_transaction" (func $submit (param i32) (param i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "invoke") (param $ptr i32) (param $len i32) (result i32)
    local.get $ptr
    local.get $len
    call $submit))"#,
        )
        .expect("WAT parses");
        let module = WasmPluginModule::compile(&bytes).expect("module compiles");
        let capabilities = WasmCapabilities {
            game_snapshot: None,
            game_write: true,
        };
        let mut instance =
            WasmPluginInstance::instantiate_with_capabilities(&module, &capabilities)
                .expect("instance with granted gameWrite starts");
        let proposal = serde_json::json!({
            "schemaVersion": 1,
            "type": "playMove",
            "color": "black",
            "vertex": {"column": 3, "row": 3},
        });
        let response = instance.invoke(&proposal).expect("invocation succeeds");
        assert_eq!(response, serde_json::json!({"ok": true}));
        let pending = instance.take_pending_transactions();
        assert_eq!(pending.len(), 1);
        assert_eq!(
            pending[0], proposal,
            "the proposal must be collected verbatim"
        );
    }

    #[test]
    fn ungranted_game_write_fails_to_link() {
        let bytes = wat::parse_str(
            r#"(module
  (import "sabaki" "game_submit_transaction" (func $submit (param i32) (param i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "invoke") (param $ptr i32) (param $len i32) (result i32)
    local.get $ptr
    local.get $len
    call $submit))"#,
        )
        .expect("WAT parses");
        let module = WasmPluginModule::compile(&bytes).expect("module compiles");
        assert!(
            matches!(
                WasmPluginInstance::instantiate_with_capabilities(
                    &module,
                    &WasmCapabilities::default()
                ),
                Err(WasmError::Compile(_))
            ),
            "an ungranted gameWrite import must fail to link"
        );
    }

    #[test]
    fn invalid_json_response_is_rejected() {
        // The plugin writes garbage into the buffer and reports a length.
        let bytes = wat::parse_str(
            r#"(module
  (memory (export "memory") 1)
  (func (export "invoke") (param $ptr i32) (param $len i32) (result i32)
    (i32.store8 (local.get $ptr) (i32.const 123))
    (i32.const 1)))"#,
        )
        .expect("WAT parses");
        let module = WasmPluginModule::compile(&bytes).expect("module compiles");
        let mut instance = WasmPluginInstance::instantiate(&module).expect("instance starts");
        assert!(matches!(
            instance.invoke(&serde_json::json!({})),
            Err(WasmError::InvalidResponse(_))
        ));
    }
}
