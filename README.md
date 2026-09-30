# ffrwd/blitz

An HTML/CSS compositor for video. You write one HTML document with the
video inputs in it as images, and change it over time with change rows:
add or remove a class, set an element's text, replace its HTML. Each frame
is the document at that frame's time, rendered over that frame's pictures.
CSS animations and transitions run on stream time, so a row that adds a
class at 3.01 s starts its transition at exactly 3.01 s.

The document is rendered by [Blitz](https://github.com/DioxusLabs/blitz)
(Stylo for CSS, Taffy for layout, Parley for text, blitz-paint and Vello's
CPU renderer for pixels), compiled to a wasm module that ffrwd's sidecar
hosts.

![Programme full frame](https://raw.githubusercontent.com/imbcmdth/ffrwd-package-blitz/main/examples/lbar-1-in.png)
![Programme shrinking to reveal the ad](https://raw.githubusercontent.com/imbcmdth/ffrwd-package-blitz/main/examples/lbar-2-mid-transition.png)
![L-bar with lower third and logo](https://raw.githubusercontent.com/imbcmdth/ffrwd-package-blitz/main/examples/lbar-3-squeezed.png)
![Programme growing back](https://raw.githubusercontent.com/imbcmdth/ffrwd-package-blitz/main/examples/lbar-4-out.png)

The four stills are from `examples/lbar.sql` at 1.0 s, 2.4 s, 5.0 s and
8.9 s: the programme squeezes into the top right over 0.75 s to show an ad
behind it, a lower third slides in with its name set by a change row, a
pulsing logo starts when a row inserts it and stops when another removes
it, and the programme grows back.

## Requires

ffrwd 0.27.6 or later. `compose2` and `compose3` declare a rows column
beside several streams, which ffrwd 0.27.4 and 0.27.5 refuse, and a
refused declaration stops the whole package loading: on those versions a
query calling `compose` or `compose1` fails too. The modules are built
against `ffrwd:av@0.18.0` (the `window-module` world). The package needs
no capabilities: no network, no files, no GPU.

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
           changes => '[{"at": 1.0, "select": "#lower", "text": "Jane Example"},
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

| export | video inputs | change rows from | workers |
|---|---|---|---|
| `compose` | 1 | `changes`, and rows arriving with the frames | one |
| `compose1` | 1 | `changes` | frame-parallel |
| `compose2` | 2 | `changes`, and rows arriving with the first input's frames | one |
| `compose3` | 3 | `changes`, and rows arriving with the first input's frames | one |

A module's describe cannot depend on its parameters, so each shape is a
module of its own. `compose`, `compose2` and `compose3` keep what rows
arriving with frames did from one frame to the next, and a host spreading
frames over workers hands each worker only its own frames' rows, so they
run on one worker. In 0.1.0 every compose of two or three inputs runs on
one worker, whether or not a stream sends it rows. `compose1` depends on
its parameters alone, so the sidecar spreads it over its worker pool and
the output is the same at any worker count.

## The document

The document is static at load: the `html` parameter, as text. An empty
`html` is input 0 over the whole frame.

ffrwd has no SQL function that reads a file into a text value. Read the
file in the shell and pass it as a variable:

```
ffrwd run -f query.sql -v html="$(cat page.html)" -v source=in.mp4 -v dest=out.mkv
```

with `html => :'html'` in the query. Quotes in the document survive the
variable.

### Video in the document

- `<img src="ffrwd:N">` shows input N, counting the call's stream
  arguments from 0. Each frame the element is handed the picture of input
  N at that frame's pts. Give it a size in CSS; `object-fit` and
  `object-position` work. An `<img>` with no CSS size is laid out at the
  input's size in pixels, taken as CSS px.
- `background-image: url(ffrwd:N)` works too, frame for frame, with
  `background-size`, `background-position` and `background-repeat`.
- An input whose element is not drawn in a frame (outside the frame,
  `display: none`, `opacity: 0`) is not copied into the module for that
  frame.
- When a frame is nothing but input 0, drawn 1:1 over the whole frame with
  nothing visible over it, the module hands input 0 back without copying
  it (`bypass`). It decides from the paint commands, before any pixels
  arrive.

### Fonts and images

- DejaVu Sans is bundled and is every generic family (`sans-serif`,
  `serif`, `monospace` and the rest) and the fallback for any family not
  loaded. It is the regular face only: italic is slanted from it, and
  bold draws as regular (see Limitations, 7).
- `@font-face` with a `data:` URI works for TrueType, OpenType, WOFF and
  WOFF2, with a format hint: see Limitations, 7.
- Images come from `data:` URIs: PNG, JPEG, GIF (first frame), WebP and
  SVG.

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

Where a row is expected, an array of rows is accepted too, so one frame
can carry several changes:

```json
[{"at": 3.0, "select": "#stage", "change": "+lbar"},
 {"at": 3.0, "select": "#lower .name", "text": "Jane"}]
```

Rows apply in the order they arrive, an array in array order, and then in
order of `at`; rows with the same `at` keep their arrival order.

### Where rows come from

1. **The `changes` parameter**: JSON text holding one row, an array of
   rows (whose elements may themselves be arrays), or one row or array per
   line. Every export takes it.
2. **Rows arriving with the frames** (`compose`, `compose2` and
   `compose3`): an upstream module that emits rows beside its frames.
   `compose` declares them as its `stream_changes` column, right after its
   stream, so writing the producer's call inside `compose`'s passes both:

   ```pgsql
   SELECT ffrwd.blitz.compose(my_cues(f.video[1]), html => ...)
   ```

   `compose2` and `compose3` declare the column after their streams. The
   rows are the ones arriving with the first stream's frames, so the
   producer's call goes in the first position; rows on the second and
   third streams never reach the module. This is what needs ffrwd 0.27.6:
   earlier compilers refuse the column on a function reading several
   streams.

   ```pgsql
   SELECT ffrwd.blitz.compose2(my_cues(p.v), p.v, html => ...)
   ```

   Rows apply as they do on `compose`: at their `at`, arrays accepted, a
   later `at` scheduled, and kept in the log.

   ffrwd matches the producer's record to the column field for field, so
   the producer declares exactly `STRUCT(at number, "select" text, change
   text, text text, html text)[]`. A row is a change row when it has
   `select`; other rows are ignored. `tests/stream_rows.sql` does this with
   a test module, and `tests/stream_rows2.sql` does it on `compose2`.
3. **A live or data stream** (JSON messages from `ffrwd/vast`,
   `ffrwd/ortb` or a run-time lateral): ffrwd 0.27.4 has no path from a
   `data_stream` to a frame module. See Limitations, 9.

### Timing

- A row takes effect at its `at` exactly: the module applies it and
  resolves the document at `at`, then resolves at the frame's time. What
  it starts (a CSS animation, a transition) starts at `at`, whether or not
  a frame falls there.
- A row without `at` takes effect at the time it arrives: the frame it
  came with, or stream time 0 for the `changes` parameter.
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
  (`set-params`) that change the document, `changes` or the sizes build
  the document again and replay that log.

## Parameters

| parameter | type | default | |
|---|---|---|---|
| `html` | text | `''` | The document. `''` is input 0 over the frame. |
| `changes` | text | `''` | Change rows as JSON. |
| `width`, `height` | number | the output's | The canvas, px. |
| `css_width`, `css_height` | number | the canvas's | The design size, CSS px: the document's viewport. One side alone takes the canvas's aspect for the other. |
| `fit` | text | `'contain'` | How the design fills the canvas: `'contain'` (uniform scale, centred, black bars) or `'stretch'` (each axis on its own). |
| `bypass` | boolean | `true` | Hand input 0 back uncopied on frames that are nothing but it. |
| `log` | text | `'off'` | `'off'`, `'summary'` (a line when an instance opens and one when it ends) or `'frame'` (a line per frame with its timings), on stderr (`FFRWD_DUMP_STDERR`). A frame-parallel lane opens an instance per worker. |

`compose`, `compose2` and `compose3` also take `stream_changes`, the rows
column; the call fills it from the producer.

### Canvas, design size and output

- **Design size**: the viewport the document is laid out in, in CSS px.
  By default CSS px are frame px. `css_width => 1280` lays a document out
  1280 CSS px wide and paints it at a device scale of frame width / 1280,
  so one document serves 720p (scale 1), 1080p (1.5) and 4K (3).
- **Canvas**: the frame the document is composed for, in px, by default
  the output's size. A design whose aspect differs from the canvas's is
  fitted by `fit`.
- **Output**: ffrwd's sidecar refuses an output frame of any size but the
  first input's (every input arrives at that size too), so today the
  output is always input 0's size, and a canvas of another shape is
  fitted into it uniformly and centred, with black bars. A 1080x1920
  canvas in a 1920x1080 output is pillarboxed. See Limitations, 9.

The bypass needs the document 1:1 over the whole output, so it is off
whenever the design is scaled into bars.

## Performance

Measured on a Ryzen 9 9950X (16 cores), Windows 11: the first five items
in the proof of concept this package grew from, the last in this
package's example. Times in ms are spent in the module per frame; the
sidecar's transport comes on top.

- **720p30, one worker**: a document with a squeezing programme, a lower
  third and a logo costs 5.3 ms a frame held in the L-bar and 6.5 ms mean
  (8.4 ms p95) mid-transition. A whole `ffrwd run` of 60 s ran at 122 fps
  (4.1x real time) rendering every frame and 170 fps (5.7x) with the idle
  bypass.
- **1080p30, one worker**: the same document costs 14.2 ms mean (18.3 ms
  p95) mid-transition, plus about 4.7 ms of transport; the run went at
  59 fps (1.97x) rendering every frame and 80 fps (2.68x) with the bypass.
- **4K30 needs frame-parallel workers**: one worker takes 32 ms for a
  full-frame video and 57 to 68 ms with overlays, over a 33 ms budget. The
  rgba pipe is the other limit: with ffrwd's default pipe buffers a module
  that does nothing passes 16 fps of 4K rgba (43 fps with 1 MiB pipes).
  The proof of concept reached real time at 4K only with the yuv420p
  wire and 8 workers (35 fps); this package takes rgba.
- **Idle bypass**: 0.06 to 0.36 ms a frame inside the module, against the
  full render. The frame still crosses the pipes.
- **Change rows**: a class or text change costs 0.1 to 0.2 ms on the frame
  it lands; a full page about 0.5 to 1 ms; cost grows with the number of
  elements parsed (400 elements add about 3 ms).
- **This package's example** (`examples/lbar.sql`, 12 s of 1080p30, two
  1080p documents rendered a frame: the ad page and the composite), whole
  `ffrwd run` including compile and module load: 48.6 fps with the default
  worker pool, 27.5 fps with `--jobs 1` (median of 3 runs each). On one
  worker the composite costs 15.3 ms mean (18.4 ms p95) a rendered frame,
  the ad page 8.0 ms, and a bypassed frame 0.08 ms.

Each module is about 11.3 MB (3.7 MB gzipped), most of it Stylo.

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
   text falls back to DejaVu Sans. A whole font is too big for the
   parameters on Windows (see below), so subset it to the glyphs the page
   uses.
8. **Frame-parallel only on `compose1`.** `compose`, `compose2` and
   `compose3` read rows arriving with frames, so they run on one worker,
   and in 0.1.0 that holds for a two- or three-input compose driven by
   `changes` alone too. One worker is enough for 1080p30 and not for 4K30
   (see Performance). A host that hands each worker the rows of the frames
   it skipped (planned as `ffrwd:av` 0.19.0) lets all three run
   frame-parallel; the modules already apply such rows at their own
   times.
9. **Also:**
   - **The root element's background image does not fill the page**; only
     its colour does. Give `html, body { height: 100% }`, or put the
     background on a full-size element.
   - **A page that paints no background is transparent**, which encodes as
     black. Where the page is translucent the output is premultiplied.
   - **The output is input 0's size.** The sidecar refuses any other; see
     Canvas above. The smallest change that would lift it: let a window
     filter answer its output format from `init` (or an optional interface
     beside `window-filter`), have the sidecar write that size in its
     output header and check frames against it, and let the compiler
     treat the module's output size as unknown downstream.
   - **Several inputs must come from one point.** ffrwd runs a module
     reading several streams only when each reaches it from the same
     source through modules that emit one frame per frame in (`split`
     counts, an ffmpeg filter does not). Two separate files, or two lavfi
     sources, are refused ("do not run in lockstep"). The example's ad
     input is therefore a second `compose1` rendering an HTML page on the
     programme's clock. An ad from `ffrwd/vast` arrives as a run-time
     lateral whose streams go only to feeders, which these modules do not
     have yet.
   - **ffrwd 0.27.4 puts yuv420p on the sidecar's input edge when one
     stream feeds two inputs** (the edge ends at a `split`, not at a
     module), and these modules take rgba. Pass the stream through
     `compose1` first: `WITH p AS (SELECT ffrwd.blitz.compose1(s.video[1])
     AS v FROM ...)`. With the default document it hands every frame back
     uncopied.
   - **No data streams.** A `data_stream` cannot reach a frame module in
     ffrwd 0.27.4: a stream parameter must be video or audio, a feeder's
     kind must be `video` or `audio`, and a run-time lateral's streams go
     only to feeders. The smallest addition is a `data` feeder kind: the
     compiler accepts a `data_stream` in a feeder position and writes the
     stream as NUT to the loopback port it already allocates; the WIT
     needs only its comment changed (`kind` is a string). The module would
     read the messages over that connection (the `tcp` capability, as
     `ffrwd/switch` reads its feeders) and fold each message as change
     rows at its pts.
   - **Parameters travel on the sidecar's command line**, so the document,
     the rows and any `data:` URIs of every module in one sidecar together
     must fit in Windows' 32,767-character command line (Linux allows
     128 KiB per argument). The sidecar reads `-params-from <file>`, which
     ffrwd's compiler does not use yet.
   - **`compose2` and `compose3` take rows from their first stream
     only**, and need ffrwd 0.27.6 for it. ffrwd 0.27.4 and 0.27.5 refuse
     an annotation column on a function reading several streams, and they
     read every declaration of a package before running any of it, so on
     them no export of this package loads (see Requires).
   - **Short animations cost extra resolves.** Blitz moves a CSS animation
     on by at most one iteration per resolve, so when frames are further
     apart than half the shortest animation (a frame-parallel worker, or
     one opened late), the module resolves at steps in between before
     rendering. With 30 fps and a few workers this is rare; a 0.1 s loop
     on 16 workers adds about ten resolves (0.05 to 0.2 ms each) a frame.

## Building and testing

```
ffrwd install
cargo build --target wasm32-wasip2 --release
cargo test -p blitz-compose-core
```

`ffrwd install` fetches the `ffrwd/wasm` package, whose `wit/av.wit` the
module's `build.rs` reads (`FFRWD_WIT_DIR` names another). The build takes
about 9 minutes from clean (full LTO, four modules). `ffrwd-wasm --describe
target/wasm32-wasip2/release/compose.wasm` prints what a module declares.

The native tests (`core/tests/compose`) cover row parsing and ordering,
replay timing (an animation and a transition inserted at 3.01 s start at
3.01 s), late joiners, frame-parallel workers with and without the rows of
skipped frames, rows on two and three inputs, the bypass and which inputs
are fetched, the design scale at 720p, 1080p and 4K, 16:9, 9:16 and square
canvases, whole-pixel versus transform motion, fonts and images. They take
under a second once built.
`tests/stream_rows.sql` runs change rows from a stream through the sidecar,
and `tests/stream_rows2.sql` does the same on the first input of `compose2`
(ffrwd 0.27.6).

## License

MIT; see `LICENSE`. The built modules include third-party code under its
own licenses: Stylo (MPL-2.0), Blitz, Vello, Parley, Taffy and the other
crates listed in `THIRD_PARTY_NOTICES`, and the DejaVu Sans font
(Bitstream Vera license, DejaVu changes in the public domain, Arev glyphs
under the Arev license; `core/assets/DejaVu-LICENSE`).
