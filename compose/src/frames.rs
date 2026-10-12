//! `ffrwd:gpu@0.1.0`, the node world's companion for frames held on the
//! GPU: `ports.memory`, which says which of the node's video ports are held
//! there, `frames.planes`, an input frame's planes as textures of the
//! sidecar's device, and `frames.emit`, a rendered frame handed back as
//! one. `wit/` is the sidecar's `wit-gpu/` as the `ffrwd/gpu` package
//! carries it. Its `wasi:webgpu` is wgpu-wasi-webgpu's, so a texture is one
//! type here and in the wgpu backend.

use std::cell::Cell;

use compose_core::params::Renderer;

use crate::Params;

mod bindings {
    wit_bindgen::generate!({
        path: "wit",
        world: "ffrwd:gpu/gpu-module@0.1.0",
        with: {
            "wasi:webgpu/webgpu@0.0.1": wgpu_wasi_webgpu::wasi::webgpu::webgpu,
            "wasi:graphics-context/graphics-context@0.0.1": wgpu_wasi_webgpu::wasi::graphics_context::graphics_context,
            "wasi:io/poll@0.2.0": wgpu_wasi_webgpu::wasi::io::poll,
        },
        generate_all,
    });
}

use bindings::exports::ffrwd::gpu::ports::Guest;
use bindings::ffrwd::gpu::frames;
use bindings::ffrwd::gpu::types::{FrameMemory, PortMemory};

thread_local! {
    /// Whether the shape last asked for binds `v`: `memory` is asked right
    /// after `shape`, with the same params and no bindings.
    static CLOCKED: Cell<bool> = const { Cell::new(false) };
}

/// Notes whether the shape being answered binds `v`.
pub fn shaped(clocked: bool) {
    CLOCKED.with(|c| c.set(clocked));
}

struct Ports;

impl Guest for Ports {
    /// The video ports, `v` where it is bound, `inputs` and `video`, unless
    /// `renderer` is `cpu`.
    fn memory(params: String) -> Vec<PortMemory> {
        let renderer = ffrwd_node::read_params::<Params>(crate::PARAMS_SCHEMA, &params)
            .ok()
            .and_then(|(params, _)| params.document().ok())
            .map(|doc| doc.renderer);
        if renderer.is_none_or(|r| r == Renderer::Cpu) {
            return Vec::new();
        }
        let clocked = CLOCKED.with(Cell::get);
        ["v", "inputs", "video"]
            .into_iter()
            .filter(|port| clocked || *port != "v")
            .map(|port| PortMemory {
                port: port.to_owned(),
                memory: FrameMemory::Gpu,
            })
            .collect()
    }
}

bindings::export!(Ports with_types_in bindings);

/// The planes of frame `index` of stream `id` this tick, as textures of the
/// module's device, each `(width, height, format)` as `sizes` says.
pub fn planes(
    id: u32,
    index: u32,
    sizes: &[(u32, u32, wgpu::TextureFormat)],
) -> Result<Vec<wgpu::Texture>, String> {
    let handed = frames::planes(id, index)?;
    if handed.len() != sizes.len() {
        return Err(format!(
            "frame {index} of stream {id} came as {} planes, and its format has {}",
            handed.len(),
            sizes.len()
        ));
    }
    Ok(handed
        .into_iter()
        .zip(sizes)
        .map(|(texture, &(w, h, format))| {
            wgpu_wasi_webgpu::texture_from_host(texture, w, h, format)
        })
        .collect())
}

/// Hands `texture` to the host as the frame at `pts` on `port`.
pub fn emit(
    port: &str,
    pts: i64,
    duration: Option<i64>,
    texture: wgpu::Texture,
) -> Result<(), String> {
    frames::emit(port, pts, duration, wgpu_wasi_webgpu::host_texture(texture))
}
