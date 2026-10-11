wgpu 29.0.4 as published on crates.io, with one change: `webgpu` is not a
default feature. On a wasm32 target that feature compiles wgpu's browser
backend, whose wasm-bindgen imports a wasm32-wasip2 component cannot carry;
vello_hybrid and anyrender_vello_hybrid take wgpu with its defaults, so the
change has to be here. The workspace patches `wgpu` to this copy. Nothing
else is touched.
