# Backend and process ports

Target contract for v0.5.0-alpha.1. Code blocks are design sketches unless the
[implementation ledger](implementation.md) identifies a compiled API.

## 12. Backend and process contracts

### 12.1 Backend receives a recipe, not a command string

```rust
pub trait PackBackend: Send + Sync {
    fn capabilities(&self) -> BackendCapabilities;

    fn materialize<'a>(
        &'a self,
        recipe: &'a BackendRecipe,
        stage: &'a StageWriteRoot,
        context: &'a ExecutionContext,
    ) -> PortFuture<'a, BackendObservation, BackendError>;

    fn export<'a>(
        &'a self,
        recipe: &'a ExportRecipe,
        stage: &'a StageWriteRoot,
        context: &'a ExecutionContext,
    ) -> PortFuture<'a, ExportObservation, BackendError>;
}
```

`BackendRecipe` is an allowlisted typed sequence such as ensure exact provider file, remove an observed metadata key, refresh index, and apply representable metadata requirements. No serialized shell command or arbitrary user-supplied executable is reconstructed from an import record.

`BackendCapabilities` states supported formats, environment combinations, structured-result version, and exact-selection behavior. Preflight rejects unsupported semantics. An explicit conversion decision can revise the desired model; a backend cannot quietly decide the conversion itself.

`BackendObservation` includes exit status, observed installed records, generated files, dependency additions, omitted items, restricted acquisitions, diagnostics, and log references. It is evidence, not a successful domain outcome.

The pin and identity model must be checked against fresh backend metadata. Platform additions and URL-backed metadata pass through the same final postcondition checks even if different adapters produced them.

### 12.2 Process interface

```rust
pub struct ToolInvocation {
    pub tool: ToolIdentity,
    pub args: Vec<OsString>,
    pub cwd: StageLocation,
    pub environment: ProcessEnvironment,
    pub deadline: Deadline,
    pub output: OutputLimits,
}

pub trait ProcessRunner: Send + Sync {
    fn run<'a>(
        &'a self,
        invocation: ToolInvocation,
        context: &'a ExecutionContext,
    ) -> PortFuture<'a, ProcessObservation, ProcessError>;
}
```

The process owner retains Unix process-group or Windows job ownership and includes stdout/stderr lifetime in deadline handling. Closing pipes does not authorize unbounded `wait`; immediate-child exit does not mean descendants retired. Cancellation kills or drains the owned tree according to policy, then releases resources when retirement is confirmed.

Arguments are separate native arguments. Shell execution is an explicit tool capability, not the default way to interpolate a command. Script templates require output-language escaping: shell quoting is not HTML escaping, and data inserted into comments still needs newline handling.

Capture bounds and streaming display are independent. A slow progress observer cannot block process supervision. Excess captured output has a declared truncate/spool/fail policy rather than unbounded accumulation.

### 12.3 Tool resolution

`ToolResolver::resolve(ToolRequirement)` is lazy, scoped to operations that need the tool, and returns a `ToolIdentity` plus a retained executable handle/location. Managed downloads use bounded acquisition and expected release digests. Probes have deadlines and require an acceptable exit status and parsed capability output.

External tooling remains supported through explicit configuration. Record the actual version/capability identity; report when exact provenance cannot be established. A version label alone is not equivalent to a digest of the executable used.

Do not bootstrap tools for help, version, unrelated inspection, or pure preview. A preview that cannot fully resolve without executing a tool reports that limitation.
