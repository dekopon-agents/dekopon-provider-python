# Future: composable native Python modules

Not implemented: the generic Python module bridge described here is a future direction, not a supported provider feature.

## Goal

Build a batteries-included Dekopon Python provider from independently compiled components, without recompiling the RustPython core whenever a library changes or the module set changes:

```text
python-core.wasm
+ python-arrow-memtable.wasm
+ python-dekopon-http.wasm
+ module-registration wiring
──────────────────────────────
       python-provider.wasm
```

These names are illustrative. The core would know a stable module-bridge protocol, not Arrow, SQL, or HTTP-specific Python APIs. This is a packaging-time composition goal, not runtime downloading, arbitrary dynamic loading, or compatibility with CPython extension wheels.

## What the experiment already establishes

The default-off `engine-swap` experiment composes one unchanged prebuilt Python component with either of two separately compiled toy library components. The resulting provider components run through the existing SDK broker host without new host bindings. Both expose the normal provider interface and retain only the existing HTTP interface as an external import.

The current Python component is **not ignorant of its library interface**: it contains a handwritten native `dekopon_engine` module that calls generated bindings for `dekopon:engine/api@0.1.0`. Its functions test component calls, resource lifetime, fuel exhaustion, and memory-allocation failure; they are not a data-analysis API. Adding new Python-facing operations still requires changing that adapter.

A Wasm component is a binary containing typed imports/exports and potentially nested modules/components. WIT describes interfaces; it is not executable module-registration code. Composition connects a component's imports to another component's exports. Wasmtime instantiates the internal graph before the broker invokes the outer provider; Python calls already-connected functionality as needed.

The composed binary carries that graph. The broker does not need a second runtime manifest enumerating bundled libraries, although normal provider configuration and all remaining host-import requirements still apply. A build-time recipe selects dependencies and wires them together.

## The missing binding layer

Connecting WIT functions does not by itself make `import arrow_memtable` work. Something must register Python modules, expose functions/classes, marshal values, and manage object lifetimes in the RustPython VM.

There are two possible directions:

1. **Generated, library-specific adapters.** A bindgen-like tool consumes WIT plus Python mapping metadata and emits RustPython module definitions and wrappers. This preserves typed per-function component interfaces and removes handwritten glue. It does not by itself eliminate rebuilding the component containing those adapters when the module set changes. Separating adapters from the core would still require a stable registration/value bridge, not access to RustPython's private Rust ABI.
2. **A generic bridge compiled into the core.** Libraries expose a shared module-description and invocation protocol, and the core creates Python modules and proxy objects from it. This could allow module-set changes through composition alone. A generic `call(name, args)` interface is flexible but moves per-function type checking from WIT into bridge validation unless additional generated machinery preserves it.

No generic bridge, metadata format, or compatibility promise is selected here. Keep the existing fixed facade as the reference until a small follow-up can demonstrate registration and one resource-backed Python object without library-specific core changes.

## Bindgen-like problems to resolve

### Discovery and registration

- Define how the core receives the selected module inventory: generated aggregate component, explicit registration calls, or another bounded mechanism. Do not assume reflection over arbitrary composed exports.
- Map WIT package/interface names to Python module names, including collisions, aliases, version conflicts, and initialization order.
- Register modules before executing scripts and integrate with the provider's exact import allowlist. Composition must not silently make private or privileged modules importable.
- Decide whether only functions are supported initially or whether classes, methods, constants, and submodules are required. Python import laziness is distinct from lazy Wasm instantiation.

### Python signatures and value conversion

- Map WIT integer ranges, floats, strings, bytes/lists, records, enums, variants, options, results, and resources into explicit Python semantics.
- Specify positional/keyword arguments, defaults, names, documentation, and type hints. WIT alone does not express every Python API convention; any extra metadata needs a defined owner and format.
- Validate conversions, size/depth bounds, overflow, and unsupported values. Python objects cannot simply cross as Rust pointers or CPython objects.
- Decide whether to restrict the initial bridge to a useful subset instead of promising arbitrary Python object interchange.

### Objects, ownership, and errors

- Represent WIT resources as Python proxies while preserving owned versus borrowed handles and their implementing component's identity.
- Define explicit close/context-manager behavior, cleanup on exceptions, interpreter teardown, double-close behavior, and use-after-close errors. Do not rely solely on Python garbage-collection timing.
- Keep resources invocation-scoped under the current fresh-interpreter model; do not accidentally introduce persistence or cross-user handles.
- Map typed library errors to Python exceptions without turning host policy refusals, traps, or exhausted budgets into successful results. Separate argument errors, engine errors, and host execution failures.

### Bulk data and execution

- Keep tables and query state in their implementing component; expose handles and batched operations rather than crossing the boundary for each scalar.
- Choose an explicit bulk representation, potentially Arrow IPC. Separate component memories and canonical ABI conversions mean zero-copy cannot be assumed; Arrow's in-memory format alone does not bridge that boundary.
- Define bounded result collection and working-set behavior. Input/output byte limits do not bound joins, decompression, sorting, or intermediate allocations.
- Start with synchronous bounded calls unless asynchronous calls, callbacks, or streams are demonstrated necessary. Those require executor, reentrancy, cancellation, and resource-lifetime decisions.

### Versioning and packaging

- Version the core bridge separately from each library's domain API. Identify which changes require adapter regeneration, core recompilation, or only library rebuilding and recomposition.
- Fail composition for incompatible typed interfaces; validate any generic metadata/call signatures before script execution.
- Pin component dependencies and tooling in the build recipe. Revalidate the final outer provider contract and all transitive imports, then apply normal artifact signing/distribution to the composed provider.
- Record which tested source and component bytes went into the final artifact. The experiment is not a release-pipeline reproducibility guarantee.

### Authority and resource accounting

- Splitting HTTP into `python-dekopon-http.wasm` would not move network authority into the library. Its external HTTP calls would still require the broker's invocation grants.
- A module being bundled is not permission to access files, secrets, network destinations, or other providers. Keep the final import surface narrow; do not add generic broker dispatch as an incidental plugin mechanism.
- Audit fuel, timeout, memory, resource counts, and host allocations across the composition. The experiment's memory cap applies per Wasm memory, not to the aggregate of all bundled memories.

## Boundary of the proposal

The promising outcome is an independently versioned Python core plus libraries connected by generated registration/binding machinery at packaging time. It is not a general Python extension ABI, a `pip` implementation, runtime plugin discovery, a shared-memory linking scheme, or a reason to change the broker's authority model.

Before pursuing a framework, prove the missing seam with two tiny modules: register them in an unchanged core, call a typed operation, round-trip one resource proxy, reject malformed input, and release everything when the invocation ends. DataFusion/Arrow feasibility remains a separate question from whether that bridge works.

## References

- [Component model and composition](https://component-model.bytecodealliance.org/design/components.html)
- [WIT reference](https://component-model.bytecodealliance.org/design/wit.html)
- [WAC composition tooling](https://github.com/bytecodealliance/wac)
- [componentize-py](https://github.com/bytecodealliance/componentize-py): related WIT-to-Python tooling, not a drop-in module loader for this embedded RustPython provider.
