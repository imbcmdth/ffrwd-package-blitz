-- The two exports, hosted as the one node module the package ships. A
-- node's ports follow from its params and from the inputs a call binds, so
-- `compose`, clocked by its picture, and `page`, a source with a clock of
-- its own, are one module declared twice.
--
-- The value parameters:
--   html        the document, as HTML text; '' is input 0 over the frame.
--               A video input is <img src="ffrwd:N"> or a background-image
--               of url(ffrwd:N): N is 0 for `v` and 1 on for `inputs`, in
--               the order the call names them.
--   rows        change rows as JSON text: an array of row objects, one
--               object, or one object per line (see the README).
--   width,
--   height      the canvas, px. Both given, the output is that size; NULL
--               follows `v`, and one side alone takes `v`'s aspect for the
--               other and is fitted into `v`'s size.
--   css_width,
--   css_height  the design size, CSS px: the document's viewport. NULL
--               follows the canvas; one side alone takes the canvas's aspect
--               for the other.
--   fit         'contain' (uniform scale, centred, black bars) or 'stretch'.
--   bypass      hand an input back without copying it on frames the
--               document shows as nothing but that input, 1:1 over the
--               whole frame.
--   log         'off' (errors only), 'summary' (a line when an instance
--               opens and one when it ends) or 'frame' (one line per frame
--               on stderr).

-- `compose` renders the document once per frame of `v`, at that frame's
-- time. `inputs` are held: each shows its newest frame at or before the
-- tick and nothing while its feed is down, the way ffrwd/switch shows a
-- feeder. `port` gives the inputs as whatever connects to that loopback
-- port, when the call binds no stream there. `lead`, `linger` and
-- `timeout` are the held inputs' (a source tagged smart_timed=1 is on the
-- clock's time and waits for it; any other starts `lead` seconds after
-- `lead` seconds of it are held). `changes` takes rows from any producer
-- in the query, paired by time and folded as state, waiting for each
-- producer at most `latency` seconds when that is given. `presence` makes
-- rows from the held inputs' feeds coming and going (see the README); a
-- call that gives it runs on one worker, and one that does not may run on
-- several.
CREATE FUNCTION compose(v video_stream,
                        inputs video_stream[] DEFAULT NULL,
                        changes STRUCT(at number, "select" text, change text,
                                       text text, html text)[][] DEFAULT NULL,
                        html text DEFAULT '',
                        rows text DEFAULT '',
                        width number DEFAULT NULL,
                        height number DEFAULT NULL,
                        css_width number DEFAULT NULL,
                        css_height number DEFAULT NULL,
                        fit text DEFAULT 'contain',
                        bypass boolean DEFAULT true,
                        port number DEFAULT NULL,
                        lead number DEFAULT 0.3,
                        linger number DEFAULT 0,
                        timeout number DEFAULT 1,
                        latency number DEFAULT NULL,
                        presence text DEFAULT '',
                        log text DEFAULT 'off')
RETURNS video_stream
  AS 'target/wasm32-wasip2/release/compose.wasm', 'compose' LANGUAGE wasm;

-- `page` renders the document `fps` times a second on a `width` x `height`
-- canvas and reads nothing: a source, called in FROM, whose picture is
-- `<alias>.video[1]`. It never ends by itself; `WHERE <alias>.t < 10`, or
-- the end of what reads it, ends it.
CREATE FUNCTION page(html text DEFAULT '',
                     width number DEFAULT 1280,
                     height number DEFAULT 720,
                     fps number DEFAULT 30,
                     rows text DEFAULT '',
                     css_width number DEFAULT NULL,
                     css_height number DEFAULT NULL,
                     fit text DEFAULT 'contain',
                     log text DEFAULT 'off')
RETURNS source
  AS 'target/wasm32-wasip2/release/compose.wasm', 'compose' LANGUAGE wasm;
