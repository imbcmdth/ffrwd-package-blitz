# ffrwd/blitz

An HTML/CSS compositor for video. You write one HTML document with the
video inputs in it as images, and change it over time with change rows:
add or remove a class, set an element's text, replace its HTML. Each frame
is the document at that frame's time, rendered over that frame's pictures.
CSS animations and transitions run on stream time, so a row that adds a
class at 3.01 s starts its transition at exactly 3.01 s.

The document is rendered by [Blitz](https://github.com/DioxusLabs/blitz)
(Stylo for CSS, Taffy for layout, Parley for text, blitz-paint and Vello for
pixels), compiled to a wasm module that ffrwd's sidecar hosts. Vello
rasterises on the GPU (vello_hybrid, through wasi:webgpu) when the host
has an adapter, and on the CPU (vello_cpu) otherwise or when a call asks
for it; see [The GPU path](#the-gpu-path).

![Programme full frame](https://raw.githubusercontent.com/imbcmdth/ffrwd-package-blitz/main/examples/lbar-1-in.png)
![Programme shrinking to reveal the ad](https://raw.githubusercontent.com/imbcmdth/ffrwd-package-blitz/main/examples/lbar-2-mid-transition.png)
![L-bar with lower third and logo](https://raw.githubusercontent.com/imbcmdth/ffrwd-package-blitz/main/examples/lbar-3-squeezed.png)
![Programme growing back](https://raw.githubusercontent.com/imbcmdth/ffrwd-package-blitz/main/examples/lbar-4-out.png)

The four stills are from `examples/lbar.sql` at 1.0 s, 2.4 s, 5.0 s and
8.9 s: the programme squeezes into the top right over 0.75 s to show an ad
behind it, a lower third slides in with its name set by a change row, a
pulsing logo starts when a row inserts it and stops when another removes
it, and the programme grows back. The ad is a page of its own, rendered by
the same module on its own clock.

## Requires

The ffrwd release after 0.29.5. The module imports wasi:webgpu, and that
release is the first whose sidecar hosts the two functions the module's
backend adds to the interface (an encoder timestamp and the queue's
timestamp period) and answers the interface's texture getters (a
texture's width, height, format and the rest). 0.29.5 and earlier do not
load this version of the package, with a GPU or without one; 0.2.1 is the
version for them. The module is a node, built against `ffrwd:av@0.19.1`,
and the package's `ffrwd/wasm` is 0.19.1.

The package asks for one capability, `gpu`, which the compiler grants to
every module that imports wasi:webgpu (it reads the import from the
module's describe and passes `-gpu` to the sidecar). It needs no network
and no files.

The `gpu` capability costs the CPU path nothing: the sidecar that release
ships opens one GPU device for the whole process, so a query runs this
module on every worker, as 0.2.1 did. (An earlier sidecar capped a
module granted `-gpu` at two workers, since each worker opened a device of
its own; that cap is gone from the release this version requires.)
A held input fed by a port is handed whatever connects there by the host,
which does the listening.

## A minimal query

```pgsql
COPY (
  SELECT ffrwd.blitz.compose(f.video[1],
           html => '<style>
                      body { margin: 0; background: #000; font-family: sans-serif; }
                      #v { position: absolute; width: 100vw; height: 100vh; }
                      #lower { position: absolute; left: 60px; bottom: 60px; padding: 12px 24px;
                               background: rgba(0, 0, 0, 0.8); color: #fff; font-size: 40px;
                               opacity: 0; transition: opacity 0.5s; }
                      #lower.on { opacity: 1; }
                    </style>
                    <img id="v" src="ffrwd:0"><div id="lower"></div>',
           rows => '[{"at": 1.0, "select": "#lower", "text": "Jane Example"},
                     {"at": 1.0, "select": "#lower", "change": "+on"},
                     {"at": 3.0, "select": "#lower", "change": "-on"}]'),
         f.audio[1]
  FROM input(:'source') f
) TO :'dest' WITH (video_codec 'libx264', pix_fmt 'yuv420p')
```

`pix_fmt 'yuv420p'` matters: the module hands back rgba, and without it
libx264 picks a 4:4:4 format.

The example query runs as a recipe:

```
ffrwd run ffrwd/blitz:lbar -v dest=lbar.mkv
```

## Exports

| export | reads | clock | output |
|---|---|---|---|
| `compose` | `v`, any number of held `inputs`, rows on `changes` | `v`'s frames | one frame per frame of `v`, `v`'s size or `width` x `height` |
| `page` | nothing | `fps` frames a second | `width` x `height`, a source read in FROM |

Both are one module. A node's ports follow its params and the inputs a
call binds, so the module is clocked by `v` when the call binds it and by
its own rate when it does not; `page` is the second declaration, with no
`v`.

Every row the document applies is folded as state, and a worker is handed
the rows of the ticks it did not render, so `compose` and `page` spread
over the sidecar's workers and the output is the same at any worker
count. Presence rows are made from the host's record of each feed, which
every worker is handed, so a call that gives `presence` spreads too.

## The document

The document is static at load: the `html` parameter, as text. An empty
`html` is input 0 over the whole frame.

ffrwd has no SQL function that reads a file into a text value. Read the
file in the shell and pass it as a variable:

```
ffrwd run -f query.sql -v html="$(cat page.html)" -v source=in.mp4 -v dest=out.mkv
```

with `html => :'html'` in the query. Quotes in the document survive the
variable, and a long document reaches the sidecar in a file of its own.

### Video in the document

- `<img src="ffrwd:N">` shows input N: `v` is 0, and the streams on
  `inputs` are 1 on, in the order the call names them. Each frame the
  element is handed input N's picture for that frame. Give it a size in
  CSS; `object-fit` and `object-position` work. An `<img>` with no CSS
  size is laid out at the input's size in pixels, taken as CSS px.
- `background-image: url(ffrwd:N)` works too, frame for frame, with
  `background-size`, `background-position` and `background-repeat`.
- Each input keeps its own size; nothing is scaled to `v`'s before the
  document places it.
- An input whose element is not drawn in a frame (outside the frame,
  `display: none`, `opacity: 0`) is not copied into the module for that
  frame. A held input with no picture at a frame (before its feed starts,
  after it ends) draws nothing there.
- When a frame is nothing but one input, drawn 1:1 over the whole frame
  with nothing visible over it, the module hands that input back without
  copying it (`bypass`): `v` while the document is idle, an ad while it
  fills the screen. It decides from the paint commands, before any pixels
  arrive. An input in yuv420p cannot leave on the rgba output as it is:
  on the CPU it is fetched, converted and handed on as a new frame, and on
  the GPU converted on the device.

### Pixel formats

`v` and `inputs` take rgba or yuv420p, whatever `renderer` says. The
compiler puts the first one listed on the wire: yuv420p with
`renderer => 'cpu'`, so ffmpeg hands the module its decoded planes, and
rgba otherwise, which the sidecar makes from the decoder's pictures on the
device. A yuv420p picture is converted to straight rgba in the module, on
the CPU path before it is drawn, and on the GPU path on the device. Both
read the stream's colour the same way: BT.709 when its matrix is `bt709`
and BT.601 otherwise (as swscale reads a stream that does not say), full
range when its range is `pc` and limited otherwise. Each chroma sample
covers the two by two pixels it is coded for, as swscale's default
conversion places it.
On the CPU the conversion is ffrwd-frame's, within one level of swscale
computed exactly and within three of swscale's default fast path. yuv420p
is first on the CPU path because the module's conversion runs on every
worker, where ffmpeg's conversion to rgba runs in the one ffmpeg ahead of
the sidecar and held a 4K run to 23 frames a second against 30; see
[Through ffrwd](#through-ffrwd-at-the-sidecars-default-workers).

### Held inputs

`inputs` are paired with `v` the way the switch pairs a feeder: each shows
its newest frame at or before the tick, the last one repeats while its
source runs late, and frames are skipped when it catches up. A source
tagged `smart_timed=1` is on `v`'s time and waits for it. Any other is
shown from `lead` seconds after the host holds `lead` seconds of it, on
`v`'s frames; `lead => 0` shows a stream of the query from the tick its
first frame is at, which keeps it in step with `v` when both start
together. `linger` keeps a source's last frame for that many seconds after
it ends, and `timeout` gives up on one that stops sending (0 never does).
`lead`, `linger` and `timeout` are fixed for a call, since the host plans
the pairing before anything runs.

The inputs need not come from one source: two files, or a page and a
file, are held alike.

A run-time lateral's stream is a held input too, as it is a switch's
feeder: `compose(prog.v, ad.video)` over `LATERAL ffrwd.vast.play(...)
ad` gives the ad a loopback port, which each instance of the lateral
writes to, and the ad is shown over `v` while one plays. The compiler
writes that port into `port` and the call leaves `port` out. `port` is a
list, one port per held input in `inputs` order, or a single port read
as a list of one.

## Change rows

```json
{"at": 3.01, "select": "div.foo",  "change": "-foo +bar ~baz"}
{"at": 4.0,  "select": "#name",    "text": "Jane Example"}
{"at": 5.0,  "select": "#slot",    "html": "<div class=\"logo\"></div>"}
```

- `select` is any CSS selector. The change applies to every element it
  matches. There is no allow-list and no restriction on selectors or
  markup, whatever the rows come from.
- `change` removes (`-`), adds (`+`) or toggles (`~`) classes.
- `text` replaces the element's content with that text.
- `html` replaces the element's content with that markup. On `body` or
  `html` it replaces the page.
- `at` is seconds of stream time, the frame's time. It is optional.

Where a row is expected, an array of rows is accepted too, so one message
can carry several changes:

```json
[{"at": 3.0, "select": "#stage", "change": "+lbar"},
 {"at": 3.0, "select": "#lower .name", "text": "Jane"}]
```

Rows apply in the order they arrive, an array in array order, and then in
order of `at`; rows with the same `at` keep their arrival order.

### Where rows come from

1. **The `rows` parameter**: JSON text holding one row, an array of rows
   (whose elements may themselves be arrays), or one row or array per
   line. A malformed row is refused when the query is compiled.
2. **The `changes` input**: rows from any producer in the query, a node
   that writes rows or a data stream, several of them in an `ARRAY[...]`.
   Writing the producer's call in `changes`' place passes its rows:

   ```pgsql
   SELECT ffrwd.blitz.compose(f.video[1], changes => my_cues(f.video[1]), html => ...)
   ```

   Each row arrives at its message's time, and the host hands a frame the
   rows stamped up to it, waiting for each producer to get that far (at
   most `latency` seconds, when that is given; a producer that says
   nothing of its progress needs one). A row is read when it has `select`:
   the producer's record needs that field, and others pass. Rows without
   `select`, a deal track's for instance, are left alone.
   `tests/stream_rows.sql` drives a document from a test node, and
   `tests/stream_rows2.sql` does the same with a second picture held.
3. **Presence**: rows the module makes when a held input's feed comes and
   goes, below.

### Timing

- A row takes effect at its `at` exactly: the module applies it and
  resolves the document at `at`, then resolves at the frame's time. What
  it starts (a CSS animation, a transition) starts at `at`, whether or not
  a frame falls there.
- A row without `at` takes effect at the time it arrives: its message's
  time, or stream time 0 for the `rows` parameter.
- An `at` later than the arrival schedules the change for then.
- An `at` earlier than the arrival takes effect at the arrival: the style
  clock never runs backwards, so a change cannot be put in the past.
- An animation starts when the row that adds it takes effect. A loop runs
  until a row removes it (removes the element, or the class that carries
  the animation).
- The document's own animations, those in the static `html`, start at
  stream time 0, not at the first frame.
- The module keeps a log of the rows applied since the last full-page
  `html` replace, each with the time it took effect. New parameters
  (`set-params`) that change the document, `rows` or the sizes build the
  document again and replay that log.

### Presence

`presence` gives the document the rows `ffrwd/switch`'s `flex_input` sent
at the edges of an insertion, made from the host's record of each held
input's feed: the frame its start was fixed on, the frame its first
picture shows on, and, once the source has ended, the last frame it shows
on. Every worker is handed that record, including the feeds that ended
between two frames it rendered, so each says the same rows at the same
times. It is JSON text, one entry or an array, one entry per input:

```pgsql
presence => '[{"input": 1, "on": {"select": "#stage", "change": "~takeover"},
               "coming": {"select": "#stage", "change": "~coming"},
               "countdown": "#count", "lead_out": 0.2}]'
```

- `on` takes effect at the feed's first frame; `off`, `on` again when left
  out (what a `~` toggle wants), `lead_out` seconds before `v` is shown
  again, so a transition out ends as the feed's last frame leaves, or on
  the frame `v` is back when the end was not told ahead.
- A start known ahead of itself (a source held for `lead`, a timed one
  waiting for its time) says `coming` on the frame the start was fixed on
  and again at the start, so a toggle is on for the wait, and gives the
  `countdown` element the whole seconds left, one a second, each at its
  own time. A wait cut short says `coming` where it was cut and nothing
  more.
- `tests/presence.sql` is an L-bar and a countdown driven by an ad's feed
  alone, the same frame for frame at `--jobs 1` and `--jobs 4`.
- Each message is a change object or an array of them, without `at`,
  which the feed's times fill in.

## Parameters

| parameter | type | default | |
|---|---|---|---|
| `html` | text | `''` | The document. `''` is input 0 over the frame. |
| `rows` | text | `''` | Change rows as JSON. |
| `width`, `height` | number | `v`'s | The canvas, px. Both given, the output is that size. |
| `css_width`, `css_height` | number | the canvas's | The design size, CSS px: the document's viewport. One side alone takes the canvas's aspect for the other. |
| `fit` | text | `'contain'` | How the design fills the canvas: `'contain'` (uniform scale, centred, black bars) or `'stretch'` (each axis on its own). |
| `bypass` | boolean | `true` | Hand an input back uncopied on frames that are nothing but it. A yuv420p input leaves converted, as a new frame. |
| `port` | number, or a list of them | none | `compose`: the loopback ports the held inputs are given on, one each in `inputs` order; written by the compiler for laterals. |
| `lead`, `linger`, `timeout` | number | `0.3`, `0`, `1` | `compose`: the held inputs' pairing, in seconds. |
| `latency` | number | none | `compose`: the longest wait for a producer on `changes`, in seconds. |
| `presence` | text | `''` | `compose`: rows at the edges of held inputs' feeds. |
| `renderer` | text | `'auto'` | Who rasterises the document: `'auto'` (the GPU when the host has an adapter, the CPU otherwise), `'gpu'` (refused with no adapter) or `'cpu'` (no adapter asked for). See [The GPU path](#the-gpu-path). Every renderer takes rgba or yuv420p on `v` and `inputs` (see [Pixel formats](#pixel-formats)); 0.3.0's CPU path took rgba alone. |
| `fps` | number | `30` | `page`: frames a second. |
| `log` | text | `'off'` | `'off'`, `'summary'` (a line when an instance opens and one when it ends) or `'frame'` (a line per frame with its timings), on stderr (`FFRWD_DUMP_STDERR`). Each worker opens an instance. |

`page` takes `html`, `width` and `height` (1280 and 720 by default),
`fps`, `rows`, the design size, `fit`, `renderer` and `log`.

### Canvas, design size and output

- **Design size**: the viewport the document is laid out in, in CSS px.
  By default CSS px are frame px. `css_width => 1280` lays a document out
  1280 CSS px wide and paints it at a device scale of frame width / 1280,
  so one document serves 720p (scale 1), 1080p (1.5) and 4K (3).
- **Canvas**: the frame the document is composed for, in px. A design
  whose aspect differs from the canvas's is fitted by `fit`.
- **Output**: with `width` and `height` both given, the output is the
  canvas, at that size, whatever size `v` is: a 1080x1920 output from a
  1920x1080 programme is one call. Without them it is `v`'s size, and a
  canvas given by one side alone is fitted into it uniformly and centred,
  with black bars.

The bypass needs an input 1:1 over the whole output, so it is off
whenever the design is scaled into bars, and never hands back an input of
another size than the output's.

## The GPU path

With `renderer => 'auto'`, the default, or `renderer => 'gpu'`, an
instance asks the host for a GPU adapter when it opens, through
wasi:webgpu, with wgpu running on
[wgpu-wasi-webgpu](https://github.com/imbcmdth/wgpu-wasi-webgpu). With an
adapter, blitz-paint paints the document into anyrender_vello_hybrid,
which prepares the scene on the CPU and rasterises it on the GPU into a
texture of the output's size: RGBA8, premultiplied where the page is
translucent, as the CPU path's frames are. With no adapter, and with
`renderer => 'cpu'`, the document is rasterised as in 0.2, on vello_cpu.

On a sidecar that holds frames on its GPU (the release after ffrwd
0.29.5), the pictures and the frame never leave the device. The module
says, beside its shape, that `v`, `inputs` and `video` are held on the
GPU (`ffrwd:gpu`'s `ports.memory`, with `renderer` `gpu` or `auto`), so
the sidecar hands each picture over as textures of its own device and
takes the frame back the same way:

- An rgba picture is drawn straight from the sidecar's texture.
- A yuv420p picture arrives as its three planes and is converted to rgba
  on the device, once per picture, in the stream's matrix and range
  (BT.709 for `bt709`, BT.601 otherwise; full range for `pc`, limited
  otherwise), then drawn the same way. A decoder running on the device
  writes such planes, so decoded video reaches the page without a copy
  through host memory.
- The rendered frame leaves as a texture (`frames.emit`); whatever reads
  it next on the device (an encoder on the device, another module)
  reads it there, and only an edge to ffmpeg downloads it.
- A bypassed frame still leaves without being drawn: an rgba input by
  `same`, a yuv420p one as its conversion.

Only the rasteriser changes. The document, the change rows, presence, the
choice of which inputs a frame draws and the bypass are the same on both
paths, and a frame handed back uncopied reaches neither.

`renderer` chooses:

- `'auto'`, the default: the GPU when the host has an adapter, and the
  CPU otherwise. The CPU also when an input's pictures are larger than
  4096 px on a side, which vello_hybrid's image atlas does not take. On a
  sidecar with no device the ports `auto` holds on the GPU are handed
  their frames in host memory, and the module draws them on the CPU.
- `'gpu'`: the GPU, or a refusal when the instance opens, naming
  `renderer => 'gpu'` and the reason (no adapter, a device refused, a
  frame or picture too large).
- `'cpu'`: the CPU. No adapter is asked for.

The default is `'auto'` because the GPU path is the faster one at the
sidecar's default workers: a programme and two 1080p pictures in picture,
decoded and encoded on the device, run at 266 frames a second at 1080p
and 159 at 4K, against 54 and 23 on the CPU with ffmpeg decoding and
encoding. With the frame read back once for an encoder in ffmpeg, the GPU
path still runs at 147 and 39. See
[Through ffrwd](#through-ffrwd-at-the-sidecars-default-workers).

`log => 'summary'` names the path, and the adapter, in the line each
instance opens with. New parameters (`set-params`) that change `renderer`
move the instance to the other path.

Where a picture does arrive as bytes (a sidecar that holds frames in
host memory), it is uploaded to the GPU once for each frame of its
source: a held input showing the same frame again, because its source is
late or it lingers, is neither fetched nor uploaded again. A picture whose
alpha is 255 everywhere, as decoded video is, is written to the GPU as it
is; one with transparency is premultiplied on the way. An upload that a
frame did not draw is freed before the next frame's uploads.

Each worker opens an instance; every instance shares the sidecar's one
GPU device.

### How close the two paths are

`compose/tests/sidecar.rs` runs the lbar example (its document and rows,
over a programme of colour bars, a gradient and a moving box, and an ad of
the example page's gradient and ball) through the sidecar at 1080p, once
with `renderer => 'gpu'` and once with `renderer => 'cpu'`, and compares
the 285 frames. In the 241 frames where the programme is not being
scaled, at most 0.0123% of a frame's pixels differ by more than 2 levels
on any channel, and no pixel by more than 3. In the 44 frames where the
programme is being scaled (2.0 s to 2.75 s and 8.5 s to 9.25 s), up to
0.25% of a frame's pixels differ by more than 2 levels: blitz-paint asks for pictures with
`Extend::Repeat`, and on vello_cpu a scaled picture's last row and column
take colour from its first ones, where on vello_hybrid they do not. Along
that row and column the two differ by up to the picture's contrast across
it (255 in one frame here, where the programme's right-hand column is
black on the GPU and white, from the first colour bar, on the CPU); the
GPU's is the one without the seam. Over the whole run, 0.022% of pixels
differ by more than 2 levels.

### Building with wgpu

vello_hybrid and anyrender_vello_hybrid take wgpu with its default
features, and on wasm32 wgpu's default `webgpu` feature compiles its
browser backend, whose wasm-bindgen imports a wasm32-wasip2 component
cannot carry. The workspace therefore patches wgpu to `vendor/wgpu`: wgpu
29.0.4 as published, with `webgpu` removed from its default features and
nothing else changed (`vendor/wgpu/README.ffrwd.md`). The patch stays until
wgpu leaves WASI out of the cfg that turns the browser backend on.

## Performance

Measured with the compositor spike's documents (an L-bar cycle designed
at 1280x720 CSS px) over 60 s of lavfi testsrc2 at 30 fps, on one worker
(`--jobs 1`), encoded with libx264 veryfast, on a 16-core Ryzen 9 9950X:
the module's mean time a frame (rendered frames alone in brackets), and
the whole run's frames a second.

| document | 720p | 1080p |
|---|---|---|
| the video alone, rendered | 4.5 ms, 145 fps | 10.6 ms, 72 fps |
| the video alone, handed back (`bypass`) | 0.13 ms, 249 fps | 0.18 ms, 115 fps |
| L-bar cycle as CSS keyframes | 6.0 ms, 118 fps | 15.4 ms, 53 fps |
| the same, idle frames handed back | 2.9 ms (6.7), 167 fps | 7.5 ms (18.0), 75 fps |
| L-bar cycle as transitions cued by rows | 2.7 ms (6.6), 169 fps | 7.3 ms (18.0), 68 fps |

One worker renders 1080p30 with room to spare, and a change row costs a
fraction of a millisecond on the frame it lands. 4K30 does not fit on
one; `compose` and `page` spread over the sidecar's workers.

### Through ffrwd, at the sidecar's default workers

`ffrwd run --profile` of one call: a programme of lavfi testsrc2 at 1080p
or 4K and 60 frames a second, and two 1080p60 pictures in picture (testsrc
and a mandelbrot), each 30% of the frame wide; all three h264 with
B-frames, `lead => 0`. On the GPU path the sidecar decodes each input on
the device (`d3d12va`) and encodes with `h264_d3d12va` from the device;
on the CPU path ffmpeg decodes and converts the inputs and encodes with
`h264_nvenc`. Windows 11, Ryzen 9 9950X, RTX 4090, ffrwd's sidecar from
after 0.29.5, nothing else running. Frames a second are the 480 frames
between a 2 s and a 10 s run over the difference of their wall times, so
the start-up (about 4.5 s) is left out; the median of three runs.
`process` is the profile's row over the 10 s run per frame, summed over
every node in the sidecar (on the GPU path the decoders and the encoder
too) and every worker.

| output | path | frames a second | `process` a frame | bytes to and from the host a frame |
|---|---|---|---|---|
| 1080p | GPU, `h264_d3d12va` | 266 | 12.2 ms | none |
| 1080p | GPU, `h264_nvenc` (read back once) | 147 | 16.6 ms | 8.3 MB down |
| 1080p | CPU, rgba first | 54 | 60.1 ms | 24.9 MB fetched |
| 4K | GPU, `h264_d3d12va` | 159 | 21.8 ms | none |
| 4K | GPU, yuv420p first | 150 | 21.8 ms | none |
| 4K | GPU, `h264_nvenc` (read back once) | 39 | 41.9 ms | 33.2 MB down |
| 4K | CPU, rgba first | 23 | 95.1 ms | 49.8 MB fetched |
| 4K | CPU, yuv420p first | 30 | 131.6 ms | 18.7 MB fetched |

The GPU path is the faster at both sizes whether or not the frame leaves
the device, which is why `renderer` defaults to `'auto'`. At 4K on the
CPU, rgba on the wire keeps the module's own time lower, but the ffmpeg that
converts the 4K programme to rgba ahead of the sidecar runs at 22 frames a
second and holds the whole run there; with yuv420p the module converts on
every worker and the run makes 30. On the GPU rgba first is the faster,
159 against 150 (154 to 170 and 148 to 152 over the runs): the decoder's
picture is made rgba in one pass on the device, where yuv420p takes a
split into planes and then the module's own conversion. So the CPU
renderer lists yuv420p first and the GPU renderers rgba.

### The GPU path and the CPU path (0.3.0)

The lbar example's document and rows over a programme at the output's
size and a 1080p ad, 12 s at 30 fps (360 frames, of which the bypass hands
back 143, before the L-bar starts and after it ends), through a release
build of the sidecar from after ffrwd 0.29.5, on Windows 11 with a Ryzen 9
9950X and an RTX 4090. Measured with
`cargo test -p compose --test sidecar lbar_frames_a_second -- --ignored --nocapture`:
the test makes the pictures and hands them to `ffrwd-wasm` on its stdin as
NUT, and reads the frames back from its stdout, with nothing decoded or
encoded around it. Frames a second are the frames read over the wall time
from the first frame out to the last; the time a rendered frame is the
module's own mean, from its `log => 'summary'` line, on one worker.

| output | path | one worker (`-jobs 1`) | two workers (`-jobs 2`) | the sidecar's default workers |
|---|---|---|---|---|
| 1080p | GPU | 74 fps, 16.6 ms a rendered frame | 70 fps | 95 fps |
| 1080p | CPU | 71 fps, 17.9 ms a rendered frame | 78 fps | 115 fps |
| 4K | GPU | 19 fps, 65 ms a rendered frame | 23 fps | 29 fps |
| 4K | CPU | 16 fps, 81 ms a rendered frame | 20 fps | 35 fps |

A query through ffrwd runs this module on the sidecar's default workers,
on either path: the compiler grants `-gpu` to this module because it
imports wasi:webgpu, whatever `renderer` says, and the sidecar's workers
share its one GPU device. The `-jobs 2` column is kept as a measurement
of a run held to two workers; its figures are the mean of two runs.

The numbers vary by about 5% from run to run. On one worker the GPU path
is somewhat faster, the more so at 4K, where vello_cpu's rasterising takes
about 59 ms of the CPU path's frame. Most of the GPU path's frame is copies
across the module's boundary; at 4K, from the module's per-frame timings
(`LBAR_LOG=frame`), about 19 ms fetching the two pictures (as on the CPU
path), about 20 ms uploading them to the GPU, and about 25 ms rendering
and reading the frame back. Spread over the sidecar's workers the CPU
path ran faster here: both paths level off by four workers, and the
GPU path makes two more copies a frame (the pictures up to the GPU, and
the frame back).
At two workers the GPU path is the faster at 4K and the slower at 1080p;
it also leaves the CPU's cores free for other work, such as the encoder.

### The CPU path's output buffer and yuv420p

The same example and harness as above, on the CPU path at `-jobs 1`, with
the programme and the ad fed as rgba or as yuv420p (untagged, so read as
BT.601 limited range). Each figure is the median of three runs, taken with
no build or other sidecar running beside them: before each run the
harness waited until no
`cargo`, `rustc`, `link` or `ffmpeg` process was running and the CPU's
load was under 10%, sampled the process list every 2 s during the run, and
ran again any run during which a compiler, ffmpeg or a second sidecar had
appeared. The time a frame is the module's own mean from its
`log => 'summary'` line, over all 360 frames and over the 217 it
rendered; the parts of a rendered frame are the means of its
`log => 'frame'` lines.

| output | module | fed | a frame | a rendered frame | fetching the pictures | outside painting and rasterising | a frame handed back | frames a second |
|---|---|---|---|---|---|---|---|---|
| 1080p | 0.3.0 | rgba | 8.40 ms | 13.89 ms | 1.81 ms | 0.33 ms | 0.06 ms | 87.1 |
| 1080p | one buffer, copied out | rgba | 8.59 ms | 14.20 ms | 1.80 ms | 0.41 ms | 0.07 ms | 84.8 |
| 1080p | with the yuv420p conversion | rgba | 8.44 ms | 13.96 ms | 1.89 ms | 0.34 ms | 0.06 ms | 87.1 |
| 1080p | with the yuv420p conversion | yuv420p | 10.56 ms | 15.68 ms | 4.67 ms | 0.17 ms | 2.82 ms | 81.7 |
| 4K | 0.3.0 | rgba | 44.43 ms | 73.64 ms | 3.74 ms | 0.74 ms | 0.10 ms | 18.4 |
| 4K | one buffer, copied out | rgba | 44.25 ms | 73.34 ms | 3.76 ms | 1.74 ms | 0.10 ms | 18.5 |
| 4K | with the yuv420p conversion | rgba | 43.73 ms | 72.49 ms | 3.74 ms | 0.76 ms | 0.10 ms | 18.7 |
| 4K | with the yuv420p conversion | yuv420p | 51.61 ms | 78.40 ms | 13.82 ms | 0.67 ms | 10.97 ms | 17.4 |

- **The output buffer.** Each rendered frame on the CPU path is a new
  buffer, handed to the host with the frame, since the host takes what
  the module emits. Keeping one buffer for the instance and copying each
  frame out of it costs a copy where the new buffer costs only zeroing:
  the time outside painting and rasterising, which holds either, is 1.74
  ms a 4K frame against 0.74 ms, and 0.41 ms against 0.33 ms at 1080p. The
  module keeps a new buffer a frame. The totals differ by less than the
  rasteriser's own variation from run to run (about 5%).
- **yuv420p.** Fetching a 4K programme and a 1080p ad as yuv420p and
  converting them takes 13.8 ms where fetching them as rgba takes 3.7 ms,
  and a frame handed back takes 11.0 ms where an rgba one is passed on
  uncopied; painting takes 4.8 ms against 8.0 ms, which takes back part
  of it. Over the run the module spends 51.6 ms a 4K frame on yuv420p
  against 43.7 ms on rgba, and the sidecar delivers 17.4 frames a second
  against 18.7. On the wire a
  picture is three eighths of the bytes, and ffmpeg upstream is spared
  its conversion to rgba: 15 to 19 ms of CPU a 4K frame over its threads,
  about 1.2 ms of wall time (ffmpeg 9.0.1 converting lavfi testsrc2, timed
  with `-benchmark`). That is more CPU than the module's conversion takes,
  but it runs on ffmpeg's threads beside the module, while the module's
  conversion is in its frame, and with one worker the module's frame is
  what limits the rate. Through ffrwd at the sidecar's default workers it
  is the other way round, since the module's conversion then runs on
  every worker and ffmpeg's in one process, so the CPU renderer lists
  yuv420p first (see
  [Through ffrwd](#through-ffrwd-at-the-sidecars-default-workers)).

The module is about 12.5 MB (4.1 MB gzipped), most of it Stylo.

## Limitations and workarounds

1. **`display: none` does not cancel a CSS animation** (Stylo); the element
   resumes mid-cycle when shown. Add `animation: none` in the same rule:
   `.hidden { display: none; animation: none; }`.
2. **Blitz orders `<style>` sheets by node id, not tree order.** After rows
   have created and removed nodes, a `<style>` added later can lose to an
   earlier one of equal specificity. Keep one `<style>` and change it with
   a `text` row (`{"select": "style", "text": "..."}`), or replace the
   whole page with an `html` row.
3. **SVG is drawn as one image.** Inline `<svg>` and SVG images are
   rendered by usvg as a picture: page CSS and page animations do not
   reach inside; SVG-internal animations (SMIL, CSS in the SVG) do not
   run; `currentColor` is taken when the SVG is built, not live; and SVG
   `<text>` draws nothing in the sidecar, which has no system fonts for
   usvg. Animate the `<svg>` element's box (transform, opacity) and set
   text in HTML over it.
4. **No JavaScript, no network.** Images and fonts load only from `data:`
   URIs; `http:`, `https:` and `file:` URLs load nothing.
5. **Large `data:` URIs are slow**: about 4 ms per MB of URI on the frame
   the row lands on, most of it in handling the string. Keep images small;
   animated GIFs show their first frame, so animate with CSS instead.
6. **Layout snaps to whole pixels.** Animating `left`, `top`, `width` or
   `height` moves in whole-pixel steps: a box moving 10 px in a second at
   30 fps held each pixel for three frames. Animating `transform` moves by
   fractions of a pixel (1/3 px a frame in the same test) and skips
   relayout. Animate `transform` and `opacity`.
7. **Fonts**: DejaVu Sans regular only, unless a `@font-face` loads a font
   from a `data:` URI; `font-weight: bold` draws the regular face. Blitz
   learns a loaded font's type from the format hint (a `data:` URI has no
   file extension to go by), so give one it reads: `format(truetype)`,
   `format(opentype)`, `format(woff)`, `format(woff2)`, `format("ttf")`,
   `format("otf")` or `format("woff2")`. With no hint, or with
   `format("truetype")` or `format("woff")`, the font is skipped and the
   text falls back to DejaVu Sans. Subset a font to the glyphs the page
   uses: a font is a `data:` URI, and those are slow (5).
8. **A lead out needs the end told that far ahead.** `off` is timed
   `lead_out` before `v` is back, from the end the host foretells. A
   stream from the query is read well ahead of the clock, so its end is
   known in time. A feed whose end the host learns less than `lead_out`
   ahead (a feed by port whose connection closes with little queued) has
   its `off` applied on the frame each worker first hears of it, which
   can differ by a frame or two between workers; give such a feed a
   short `lead_out`, or none. A feed whose end was not told at all (a
   `timeout`, a jump in the clock) goes `off` on the frame `v` is back,
   on every worker.
9. **Also:**
   - **The root element's background image does not fill the page**; only
     its colour does. Give `html, body { height: 100% }`, or put the
     background on a full-size element.
   - **A page that paints no background is transparent**, which encodes as
     black. Where the page is translucent the output is premultiplied.
   - **The bypass takes every input as opaque**, as decoded video is. An
     input with transparency drawn alone over the whole frame is handed
     back with its own alpha rather than over what the page has under it;
     give such a call `bypass => false`.
   - **One stream cannot be both `v` and a held input.** The host refuses a
     stream bound to two ports of one node ("stream id 0 is bound twice").
     Pass the second through a filter, `ARRAY[ffmpeg.format(s.video[1],
     'rgba')]`, as `tests/stream_rows2.sql` does.
   - **A `page` held by a `compose` in the same sidecar is not held back.**
     The host lets a source node render as far ahead of the input holding
     it as memory allows, and a run can stall or run out of memory. Pass
     the page through an ffmpeg filter on its way, and end it with the
     programme: `ARRAY[ffmpeg.format(ad.video[1], 'rgba')]` and `WHERE
     ad.t < 12`, as `examples/lbar.sql` does.
   - **One lateral on `inputs`, and no port by hand.** The compiler hands
     several laterals to one port list only when the schema's `type` is
     `array` alone, and `port` also takes the single number the host
     writes for held streams, so two ads are two calls for now. A port
     written in the call (`port => 9100`) is refused when the sidecar
     opens the module: the host reads a parameter whose schema allows two
     types as text.
   - **A yuv feed by port in a matrix the host does not convert is
     refused** ("yuv in the "gbr" matrix is not converted here"). The host
     conforms a feed by port to the held input's format, rgba, and
     converts a yuv420p feeder from its own matrix: BT.601 (`bt470bg`,
     `smpte170m`), BT.709, BT.2020 non-constant luminance, FCC and SMPTE
     240M are converted, and any other (`gbr`, `bt2020c`, `ycgco`,
     `ictcp`) is refused. An rgba feeder is passed through with no matrix
     at all. Held streams from the query are converted by ffmpeg and are
     unaffected.
   - **A yuv420p picture in a matrix other than BT.709 is read as
     BT.601** by the module, on either path, when one reaches it as
     yuv420p (see [Pixel formats](#pixel-formats)). Streams from the query
     reach it as rgba, converted by ffmpeg in their own matrix.
   - **A sidecar built without GPU support refuses this module**, whether
     or not the machine has a GPU: the module imports wasi:webgpu, and
     such a sidecar does not host it. There is no fallback to the CPU in
     that case; use 0.2.1 there.
   - **Short animations cost extra resolves.** Blitz moves a CSS animation
     on by at most one iteration per resolve, so when frames are further
     apart than half the shortest animation (a worker that renders every
     few frames, or one opened late), the module resolves at steps in
     between before rendering. With 30 fps and a few workers this is rare;
     a 0.1 s loop on 16 workers adds about ten resolves (0.05 to 0.2 ms
     each) a frame.

## Building and testing

```
cargo build --target wasm32-wasip2 --release
cargo test -p blitz-compose-core -p compose
```

The module is a node on [ffrwd-node](https://github.com/imbcmdth/ffrwd-node),
which carries the `ffrwd:av` world, so nothing is installed before
building. The GPU path's backend,
[wgpu-wasi-webgpu](https://github.com/imbcmdth/wgpu-wasi-webgpu), is a git
dependency pinned by tag, fetched by cargo. The build takes about 5
minutes from clean (full LTO).
`ffrwd-wasm --describe target/wasm32-wasip2/release/compose.wasm` prints
what the module declares, and `ffrwd-wasm --shape` its ports for a call's
params and bound inputs.

The native tests (`core/tests/compose`, `core/src/presence.rs` and
`compose/src/tests.rs`) cover row parsing and ordering, replay timing (an
animation and a transition inserted at 3.01 s start at 3.01 s), late
joiners, frame-parallel workers with and without the rows of skipped
frames, rows with several inputs, the bypass of whichever input is drawn
whole, inputs with no picture and of other sizes, which inputs are
fetched, presence from feeds starting, ending and cut short on workers
handed any share of the frames, the shapes
of `compose` and `page`, the design scale at 720p, 1080p and 4K, 16:9,
9:16 and square canvases, whole-pixel versus transform motion, fonts and
images. They take a few seconds once built.
`tests/stream_rows.sql` runs change rows from another node through the
sidecar, `tests/stream_rows2.sql` does it with a second picture held, and
`tests/presence.sql` drives a document from a held input's feed.

`compose/src/tests.rs` also feeds yuv420p pictures to the CPU path in
each matrix and range a stream may name, handed back whole, drawn by the
document and held, and compares the module's conversion with ffmpeg's
(skipped where there is no ffmpeg on the PATH).

Two tests cover the GPU path. `core/tests/gpu.rs` renders the lbar
document, a translucent page, a picture with transparency and a held
picture on vello_cpu and on vello_hybrid with wgpu's native backends, and
checks that a held picture is uploaded once. `compose/tests/sidecar.rs`
runs the lbar example through the sidecar with `renderer => 'gpu'` and
`renderer => 'cpu'` and compares the frames (see
[How close the two paths are](#how-close-the-two-paths-are)), and runs it
on the CPU from yuv420p and from the rgba ffmpeg converts the same planes
to, and compares those. It also runs it with `renderer => 'auto'` on a
sidecar left with no adapter (`WGPU_BACKEND=metal`), and checks the frames
are the CPU path's. It needs
the module built first and finds the sidecar through `FFRWD_WASM`, or
else in an ffrwd-cli checkout beside this repository
(`../ffrwd-cli/sidecar/target/release/ffrwd-wasm`); the native test needs
a GPU adapter. Without them each prints `SKIPPED` and passes.

## License

MIT; see `LICENSE`. The built module includes third-party code under its
own licenses: Stylo (MPL-2.0), Blitz, Vello, wgpu (the copy in
`vendor/wgpu`, MIT or Apache-2.0), Parley, Taffy and the other crates
listed in `THIRD_PARTY_NOTICES`, and the DejaVu Sans font
(Bitstream Vera license, DejaVu changes in the public domain, Arev glyphs
under the Arev license; `core/assets/DejaVu-LICENSE`).
