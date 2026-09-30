-- The four exports, hosted as the wasm modules the package ships. Each is
-- one window filter over rgba frames (ffmpeg's swscale converts on both
-- sides of the sidecar), one frame out per frame in, at the frame's pts.
--
-- A module's describe cannot depend on its parameters, so what differs
-- between the exports (how many video inputs, whether change rows can
-- arrive with the frames, whether the host may spread frames over workers)
-- is a module each.
--
-- The value parameters, the same on every export:
--   html        the document, as HTML text; '' is input 0 over the frame.
--               A video input is <img src="ffrwd:N"> or a background-image
--               of url(ffrwd:N), N counting the stream arguments from 0.
--   changes     change rows as JSON text: an array of row objects, one
--               object, or one object per line (see the README).
--   width,
--   height      the canvas, px; NULL follows the output frame, which the
--               host keeps at the size of the first video input.
--   css_width,
--   css_height  the design size, CSS px: the document's viewport. NULL
--               follows the canvas; one side alone takes the canvas's aspect
--               for the other.
--   fit         'contain' (uniform scale, centred, black bars) or 'stretch'.
--   bypass      hand the first input back without copying it on frames the
--               document shows as nothing but that input, 1:1 over the
--               whole frame.
--   log         'off', 'summary' (a line at open and one at the end) or
--               'frame' (one line per frame on stderr).

-- `compose` reads one video input, and change rows from `changes` and from
-- the rows an upstream module emits beside its frames (`stream_changes`).
-- It keeps what those rows did between frames, so it runs on one worker.
CREATE FUNCTION compose(v video_stream,
                        stream_changes STRUCT(at number, "select" text, change text,
                                              text text, html text)[] DEFAULT NULL,
                        html text DEFAULT '',
                        changes text DEFAULT '',
                        width number DEFAULT NULL,
                        height number DEFAULT NULL,
                        css_width number DEFAULT NULL,
                        css_height number DEFAULT NULL,
                        fit text DEFAULT 'contain',
                        bypass boolean DEFAULT true,
                        log text DEFAULT 'summary')
RETURNS video_stream
  AS 'target/wasm32-wasip2/release/compose.wasm', 'compose' LANGUAGE wasm;

-- `compose1`, `compose2` and `compose3` read one, two or three video
-- inputs, all at the same pts, and take change rows from `changes` alone.
-- Nothing else drives the document, so the host may spread frames over
-- workers, which is what 4K needs. ffrwd runs a module reading several
-- streams only when they come from one point, through modules that emit
-- one frame per frame in: see the README.
CREATE FUNCTION compose1(v video_stream,
                         html text DEFAULT '',
                         changes text DEFAULT '',
                         width number DEFAULT NULL,
                         height number DEFAULT NULL,
                         css_width number DEFAULT NULL,
                         css_height number DEFAULT NULL,
                         fit text DEFAULT 'contain',
                         bypass boolean DEFAULT true,
                         log text DEFAULT 'summary')
RETURNS video_stream
  AS 'target/wasm32-wasip2/release/compose1.wasm', 'compose1' LANGUAGE wasm;

CREATE FUNCTION compose2(v0 video_stream,
                         v1 video_stream,
                         html text DEFAULT '',
                         changes text DEFAULT '',
                         width number DEFAULT NULL,
                         height number DEFAULT NULL,
                         css_width number DEFAULT NULL,
                         css_height number DEFAULT NULL,
                         fit text DEFAULT 'contain',
                         bypass boolean DEFAULT true,
                         log text DEFAULT 'summary')
RETURNS video_stream
  AS 'target/wasm32-wasip2/release/compose2.wasm', 'compose2' LANGUAGE wasm;

CREATE FUNCTION compose3(v0 video_stream,
                         v1 video_stream,
                         v2 video_stream,
                         html text DEFAULT '',
                         changes text DEFAULT '',
                         width number DEFAULT NULL,
                         height number DEFAULT NULL,
                         css_width number DEFAULT NULL,
                         css_height number DEFAULT NULL,
                         fit text DEFAULT 'contain',
                         bypass boolean DEFAULT true,
                         log text DEFAULT 'summary')
RETURNS video_stream
  AS 'target/wasm32-wasip2/release/compose3.wasm', 'compose3' LANGUAGE wasm;
