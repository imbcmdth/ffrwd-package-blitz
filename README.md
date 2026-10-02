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
it, and the programme grows back. The ad is a page of its own, rendered by
the same module on its own clock.

## Requires

ffrwd 0.29. The module is a node, built against `ffrwd:av@0.19.0`: 0.29 is
the first release that hosts a node, holds one source onto another's clock
and hands a node rows from any producer in the query. 0.28 and earlier do
not load this package. The package needs no capabilities: no network, no
files, no GPU. A held input fed by a port is handed whatever connects there by
the host, which does the listening.

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
count. A call that gives `presence` runs on one worker, as the switch
does: it learns when a feed starts and ends on the tick that happens.

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
at the edges of an insertion, made from what the host says of each held
input's feed: when its first frame shows, and, once the source has ended,
the last frame it shows. It is JSON text, one entry or an array, one entry
per input:

```pgsql
presence => '[{"input": 1, "on": {"select": "#stage", "change": "~takeover"},
               "coming": {"select": "#stage", "change": "~coming"},
               "countdown": "#count", "lead_out": 0.2}]'
```

- `on` takes effect at the feed's first frame; `off`, `on` again when left
  out (what a `~` toggle wants), `lead_out` seconds before `v` is shown
  again, so a transition out ends as the feed's last frame leaves, or at
  the frame the feed is seen to have gone when its end was not known
  ahead.
- A start known ahead of itself (a source held for `lead`, a timed one
  waiting for its time) says `coming` at once and again at the start, so a
  toggle is on for the wait, and gives the `countdown` element the whole
  seconds left, one a second, each at its own time. A wait cut short says
  `coming` where it was cut and nothing more.
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
| `fps` | number | `30` | `page`: frames a second. |
| `log` | text | `'off'` | `'off'`, `'summary'` (a line when an instance opens and one when it ends) or `'frame'` (a line per frame with its timings), on stderr (`FFRWD_DUMP_STDERR`). Each worker opens an instance. |

`page` takes `html`, `width` and `height` (1280 and 720 by default),
`fps`, `rows`, the design size, `fit` and `log`.

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
one; `compose` and `page` spread over the sidecar's workers, except a
call with `presence`.

The module is about 11.9 MB (3.9 MB gzipped), most of it Stylo.

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
8. **Presence runs on one worker.** The host says on every tick when a
   feed's first frame shows and, once known, its last, but not when the
   start became known nor when a feed it could not foretell the end of
   went. A worker that did not render that tick would time `coming` and
   such an `off` differently, so a call with `presence` runs on one
   worker, as the switch does. One worker is enough for 1080p30 and not
   for 4K30.
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
building. The build takes about 5 minutes from clean (full LTO).
`ffrwd-wasm --describe target/wasm32-wasip2/release/compose.wasm` prints
what the module declares, and `ffrwd-wasm --shape` its ports for a call's
params and bound inputs.

The native tests (`core/tests/compose`, `core/src/presence.rs` and
`compose/src/tests.rs`) cover row parsing and ordering, replay timing (an
animation and a transition inserted at 3.01 s start at 3.01 s), late
joiners, frame-parallel workers with and without the rows of skipped
frames, rows with several inputs, the bypass of whichever input is drawn
whole, inputs with no picture and of other sizes, which inputs are
fetched, presence from feeds starting, ending and cut short, the shapes
of `compose` and `page`, the design scale at 720p, 1080p and 4K, 16:9,
9:16 and square canvases, whole-pixel versus transform motion, fonts and
images. They take a few seconds once built.
`tests/stream_rows.sql` runs change rows from another node through the
sidecar, and `tests/stream_rows2.sql` does it with a second picture held.

## License

MIT; see `LICENSE`. The built module includes third-party code under its
own licenses: Stylo (MPL-2.0), Blitz, Vello, Parley, Taffy and the other
crates listed in `THIRD_PARTY_NOTICES`, and the DejaVu Sans font
(Bitstream Vera license, DejaVu changes in the public domain, Arev glyphs
under the Arev license; `core/assets/DejaVu-LICENSE`).
