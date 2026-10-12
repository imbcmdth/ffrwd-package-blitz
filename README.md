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
rasterises on the CPU (vello_cpu) by default, and on the GPU (vello_hybrid,
through wasi:webgpu) when a call asks for it; see
[The GPU path](#the-gpu-path).

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
The figures for all workers are 0.3.0's CPU path, which rasterises as 0.2.1
does. The cap goes when the sidecar shares one GPU device across its
workers. A held input fed by a port is handed whatever connects there
by the host, which does the listening.

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
  arrive.

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
| `bypass` | boolean | `true` | Hand an input back uncopied on frames that are nothing but it. |
| `port` | number, or a list of them | none | `compose`: the loopback ports the held inputs are given on, one each in `inputs` order; written by the compiler for laterals. |
| `lead`, `linger`, `timeout` | number | `0.3`, `0`, `1` | `compose`: the held inputs' pairing, in seconds. |
| `latency` | number | none | `compose`: the longest wait for a producer on `changes`, in seconds. |
| `presence` | text | `''` | `compose`: rows at the edges of held inputs' feeds. |
| `renderer` | text | `'cpu'` | Who rasterises the document: `'cpu'` (no adapter asked for), `'auto'` (the GPU when the host has an adapter, the CPU otherwise) or `'gpu'` (refused with no adapter). See [The GPU path](#the-gpu-path). |
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

With `renderer => 'auto'` or `renderer => 'gpu'`, an instance asks the
host for a GPU adapter when it opens, through wasi:webgpu, with wgpu
running on
[wgpu-wasi-webgpu](https://github.com/imbcmdth/wgpu-wasi-webgpu). With an
adapter, blitz-paint paints the document into anyrender_vello_hybrid,
which prepares the scene on the CPU and rasterises it on the GPU into a
texture of the output's size. The texture is copied into a buffer, the
buffer is mapped, and its bytes become the frame the module hands on:
RGBA8 at the output's size, premultiplied where the page is translucent,
as the CPU path's frames are. With no adapter, and with the default
`renderer => 'cpu'`, everything is as in 0.2, on vello_cpu.

Only the rasteriser changes. The document, the change rows, presence, the
choice of which inputs a frame draws and the bypass are the same on both
paths, and a frame handed back uncopied reaches neither.

`renderer` chooses:

- `'cpu'`, the default: the CPU. No adapter is asked for.
- `'auto'`: the GPU when the host has an adapter, and the CPU otherwise.
  The CPU also when an input's pictures are larger than 4096 px on a side,
  which vello_hybrid's image atlas does not take.
- `'gpu'`: the GPU, or a refusal when the instance opens, naming
  `renderer => 'gpu'` and the reason (no adapter, a device refused, a
  frame or picture too large).

The default is `'cpu'` in this release because the GPU path reads every
frame back from the GPU into the module, and copies every picture up to
it, so on this release's sidecar a default run is faster on the CPU (see
[Performance](#the-gpu-path-and-the-cpu-path-030)). The next stage of this
work has the sidecar hand the frame on as a GPU texture, with no readback;
once it does, the default moves to `'auto'`.

`log => 'summary'` names the path, and the adapter, in the line each
instance opens with. New parameters (`set-params`) that change `renderer`
move the instance to the other path.

Each input's picture is uploaded to the GPU once for each frame of its
source: a held input showing the same frame again, because its source is
late or it lingers, is neither fetched nor uploaded again. A picture whose
alpha is 255 everywhere, as decoded video is, is written to the GPU as it
is; one with transparency is premultiplied on the way. An upload that a
frame did not draw is freed before the next frame's uploads.

Each worker opens an instance, and each instance on the GPU opens a GPU
device of its own.

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
   - **A feed by port into an rgba programme is refused** by the host
     ("yuv in the "gbr" matrix is not converted here"), even one sending
     rgba: the host tags the rgba programme with a colour it then cannot
     convert to. Held streams from the query are unaffected.
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

Two tests cover the GPU path. `core/tests/gpu.rs` renders the lbar
document, a translucent page, a picture with transparency and a held
picture on vello_cpu and on vello_hybrid with wgpu's native backends, and
checks that a held picture is uploaded once. `compose/tests/sidecar.rs`
runs the lbar example through the sidecar with `renderer => 'gpu'` and
`renderer => 'cpu'` and compares the frames (see
[How close the two paths are](#how-close-the-two-paths-are)). It needs
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
