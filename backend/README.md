# wgpu-wasi-webgpu

wgpu's `custom` dispatch traits implemented over `wasi:webgpu@0.0.1`, the
wit the ffrwd sidecar hosts (`sidecar/vendor/wasi-webgpu-wasmtime` in
ffrwd-cli). A renderer written against wgpu, vello_hybrid here, runs in a
wasm32-wasip2 module unchanged: `wgpu_wasi_webgpu::instance()` is a
`wgpu::Instance` whose adapters are the host's, and everything below it is
a wit import.

Step B of the 4K60 compositing experiment (`plans/experiment-compose-4k60.md`
in ffrwd-cli). It started in this workspace for iteration speed and moves to
its own repository once it has run on the 4090 and the L4.

## Scope

What vello_hybrid 0.1 and the experiment's harness call, pinned to
vello_hybrid's wgpu (29): instance, adapter, device, queue, buffers,
textures and views, samplers, bind groups and layouts, pipeline layouts,
WGSL shader modules, render and compute pipelines, command encoders,
render and compute passes, the four copies, `write-buffer`,
`write-texture`, `map-async`, submit, `on-submitted-work-done`, query sets,
`write-timestamp` and `resolve-query-set`, error scopes. Everything else
is `todo!()`: render bundles, surfaces, acceleration structures, binding
arrays, pipeline caches, passthrough shaders, immediates, multi-draw,
mesh shaders, timestamps inside passes, `clear-texture` (not in the 0.0.1
wit; vello_hybrid clears atlas regions with a render pass instead).

## How the 0.0.1 wit shapes it

- **Mapping is a copy.** `get-mapped-range-get-with-copy` hands back a
  `Vec`, so a read mapping is that Vec and `read_slice` is a borrow of it.
  A write mapping (`mapped_at_creation`, or `map-async` with WRITE) is a
  zeroed Vec the caller fills; when wgpu drops the mapped range it goes
  back through `get-mapped-range-set-with-copy`. `map-async` blocks on
  the host, so the callback runs before it returns and `poll` is a no-op.
- **Uploads are copies.** `write-buffer-with-copy` and
  `write-texture-with-copy` take the bytes as a list, lifted out of wasm
  memory by the canonical ABI and copied again by wgpu's staging.
- **Bytes are counted.** Every list crossing the wit is added to a
  counter: `wit_bytes()` reads the totals (up: writes and write-mappings;
  down: read-mappings), `take_wit_bytes()` reads and clears them. The
  experiment's rows carry the sum per frame.
- **Two functions the IDL does not have**, added to the vendored wit and
  host for the experiment: `gpu-command-encoder.write-timestamp` (wgpu's
  `TIMESTAMP_QUERY_INSIDE_ENCODERS`, which the host requests whenever
  `timestamp-query` is) and `gpu-queue.get-timestamp-period`, without
  which resolved timestamps are in unknown units.
- **Errors.** `RequestDeviceError` cannot be built outside wgpu, so a
  refused device is a panic with the host's message. A WGSL error reaches
  `get_compilation_info` as a message with its line (the host keeps naga's
  report); the host also prints it as an uncaptured error.
- **Adapter info.** The wit gives the adapter's name but not the host's
  backend; `AdapterInfo::backend` says `BrowserWebGpu`, the closest wgpu
  has to "WebGPU through a host".

## Building

The crate compiles on any target, as the counters alone off wasm32, so
the workspace checks natively. The bindings are generated from
ffrwd-cli's vendored wit by path (`../../ffrwd-cli/sidecar/vendor/...`),
so ffrwd-cli must be checked out beside this repository, on the branch
that carries the two added functions, and guest and host cannot drift.

```
cargo check -p wgpu-wasi-webgpu --target wasm32-wasip2
```

`bench/wasm` is the first user: the experiment's harness as a source node.
