# The 4K60 compositing experiment

The brief is `plans/experiment-compose-4k60.md` in ffrwd-cli; the design it
tests is section 3 of `plans/gpu-resident-frames.md` there. The question: for
the lbar document generalised to N video inputs at 3840x2160, every frame
animating, what does one frame cost in CPU time, GPU time and bytes, and does
60 frames a second hold over 600 frames?

The crates and scripts:

- `bench/doc` (`blitz-bench-doc`): what is rendered and what is recorded,
  shared by the native bench and the wasm run. `document(n, animate)` is the
  lbar document generalised: input 0 over the whole frame with the L-bar
  cycle, inputs 1 to N-1 in a picture-in-picture grid with `transform`
  animations, a lower third sliding in and out, a logo pulsing, designed at
  1280 CSS px (`css_width`). `gradient` and `stamp` make the synthetic rgba
  inputs with the frame counter burned in. `Meta`, `Row` and `End` are the
  three kinds of line a run writes.
- `bench/harness` (`blitz-bench-harness`): the frame loop, on whatever
  `wgpu::Instance` it is handed: the document painted by blitz into
  `anyrender_vello_hybrid` (vello_hybrid, the sparse-strip renderer: CPU
  preparation, GPU rasterisation) with no window, one `Row` a frame.
- `bench` (`blitz-bench`), two binaries. `bench` is step A: the harness on
  native wgpu, 600 frames at t = n / 60, one JSON row a frame on stdout.
  `summarize` turns rows into a markdown table, with the pixel hashes of
  frame 123 where a run read it back. `nutframe` pulls one frame of the
  wasm run's picture out of a NUT file into a PNG and compares it with
  the native bench's dump of the same frame.
- `bench/wasm` (`blitz-bench-compose`): step C, the harness as a source
  node in the shape of `page`, built for wasm32-wasip2 on
  `wgpu-wasi-webgpu` (wgpu over wasi:webgpu, a repository of its own,
  pinned by tag) and run under `ffrwd-wasm` with a
  `-gpu` grant; rows on `[@rows=...]`, the picture on `video` only when
  mapped (a readback). It ends itself after `frames`.
- `run-all.ps1` and `run-all.sh` run step A at every variant and N of 2, 4
  and 8 and print the table; `run-c.ps1` does the same for step C through
  `../ffrwd-cli/sidecar/target/release/ffrwd-wasm.exe`.

## Running

```
cargo build --release -p blitz-bench
target/release/bench --n 8 --variant 1 > bench/out/A-n8-v1.ndjson
target/release/summarize bench/out
```

`bench` takes `--n <inputs>` (1 to 64), `--variant <1|2|3|4>`, `--all-4k`
(every input at 3840x2160 instead of 1080p for inputs 1 on), `--frames`
(600), `--backend dx12|vulkan|gl|metal` (otherwise `WGPU_BACKEND` or the
primary backends), and `--dump <frame> <file.png>` to write one rendered
frame for checking by eye: the counter stamped on each input reads the frame
number, and its blue channel tells the inputs apart.

The variants:

| variant | what it stands for |
|---|---|
| 1 `resident` | the same picture blob every frame: uploaded once, render only (wave 3's steady state) |
| 2 `upload` | a new picture blob every frame, uploaded every frame (today's wire) |
| 3 `readback` | variant 1 plus `map-async` readback of the 4K frame each tick (today's output path) |
| 4 `idle` | variant 1 with the animations off (the floor) |

`summarize` prints median / p99 of each phase in ms, frames a second over
the run's wall time, frames over 16.67 ms, bytes uploaded a frame, bytes
across the wit a frame (step C only), peak memory, and the adapter and
backend from the first run's meta line. `--skip <k>` leaves the first k
frames out (pipeline compilation lands in frame 0).

## What a row holds

Times in microseconds. `resolve_us`, `plan_us` and `paint_cmds_us` are the
compositor's own phases (`paint_cmds` includes uploading any picture the
renderer has not got: the conversion to premultiplied and the
`write_texture`). `encode_us` is vello_hybrid's preparation and command
encoding. `cpu` in the table is the sum of the four. `gpu_us` is from wgpu
timestamp queries around the frame's encoder when the adapter has
`TIMESTAMP_QUERY_INSIDE_ENCODERS`, otherwise the wall clock around submit and
the wait for the queue; the meta line says which. Timestamps bracket the
encoder only: uploads staged by `write_texture` and the glyph atlas passes
vello_hybrid submits on its own run before it and are not in `gpu_us`,
but are in `wait_us`. `readback_us` covers mapping the readback buffer and
copying the frame out. `frame_us` is the whole tick, `synth_us` included
(variant 2 copies and stamps every input picture, which is not the
compositor's cost).

## Reading the numbers

The pass line in the brief is for a 4090: at N of 8, render only, CPU under
8 ms, GPU under 8 ms, under 16 MB a frame across the wit, 60 frames a second
held with the readback variant too. On integrated graphics the numbers are a
floor; report them with the adapter's name and read the ratios between
variants, not a miss.
