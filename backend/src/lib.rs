//! wgpu over `wasi:webgpu@0.0.1`: an implementation of wgpu's `custom`
//! dispatch traits whose every call is a wit import the ffrwd sidecar
//! answers, so a renderer written against wgpu (vello_hybrid here) runs in a
//! wasm32-wasip2 module unchanged. `instance()` is the entry: a
//! `wgpu::Instance` whose adapters are the host's.
//!
//! Scope: what vello_hybrid and the experiment's bench call. Instance,
//! adapter, device, queue, buffers (with the copy-based mapped ranges the
//! 0.0.1 wit has), textures and views, samplers, bind groups, pipeline
//! layouts, shader modules (WGSL), render and compute pipelines, command
//! encoders, render and compute passes, copies, `write-buffer`,
//! `write-texture`, `map-async`, submit, `on-submitted-work-done` and query
//! sets. Render bundles, surfaces, acceleration structures, binding arrays
//! and pipeline caches are `todo!()`.
//!
//! The bytes that cross the wit are counted: `wit_bytes()` reads the
//! totals, `take_wit_bytes()` reads and clears them.
//!
//! On any target but wasm32 the crate is the counters alone, so the
//! workspace builds natively.

/// Bytes that crossed the wit since the counters were last cleared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WitBytes {
    /// Into the host: `write-buffer-with-copy`, `write-texture-with-copy`
    /// and `get-mapped-range-set-with-copy`.
    pub up: u64,
    /// Out of the host: `get-mapped-range-get-with-copy`.
    pub down: u64,
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod counters {
    use std::cell::Cell;

    thread_local! {
        static UP: Cell<u64> = const { Cell::new(0) };
        static DOWN: Cell<u64> = const { Cell::new(0) };
    }

    pub fn up(n: usize) {
        UP.with(|c| c.set(c.get() + n as u64));
    }

    pub fn down(n: usize) {
        DOWN.with(|c| c.set(c.get() + n as u64));
    }

    pub fn read(clear: bool) -> super::WitBytes {
        let up = UP.with(|c| {
            let v = c.get();
            if clear {
                c.set(0);
            }
            v
        });
        let down = DOWN.with(|c| {
            let v = c.get();
            if clear {
                c.set(0);
            }
            v
        });
        super::WitBytes { up, down }
    }
}

pub fn wit_bytes() -> WitBytes {
    counters::read(false)
}

pub fn take_wit_bytes() -> WitBytes {
    counters::read(true)
}

#[cfg(target_arch = "wasm32")]
mod bindings {
    wit_bindgen::generate!({
        path: [
            "../../ffrwd-cli/sidecar/vendor/wasi-webgpu-wasmtime/wit/deps/io",
            "../../ffrwd-cli/sidecar/vendor/wasi-webgpu-wasmtime/wit/deps/graphics-context",
            "../../ffrwd-cli/sidecar/vendor/wasi-webgpu-wasmtime/wit/deps/webgpu",
            "wit",
        ],
        world: "ffrwd:wgpu-wasi-webgpu/guest",
        generate_all,
    });
}

#[cfg(target_arch = "wasm32")]
mod conv;
#[cfg(target_arch = "wasm32")]
mod backend;

#[cfg(target_arch = "wasm32")]
pub use backend::instance;
