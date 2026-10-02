-- End to end: change rows from another node driving a document with two
-- pictures. `emit_changes` (the test module in tests/producer) writes the
-- rows listed in `emit` at their times; `compose` reads them on `changes`
-- and holds the second picture, here the same programme. Build the modules
-- first, then from the package directory:
--   ffrwd run -f tests/stream_rows2.sql -v dest=stream_rows2.mkv
-- The second picture, mirrored, sits hidden in the right half. The lower
-- third shows "From a stream" from 1.0 s (a row with no `at`, taking the
-- time it is written at), its box slides in at 1.5 s (a row written at
-- 1.0 s scheduled for 1.5), the second picture shows from 2.0 s, and
-- "Changed" replaces the text at 3.0 s (two rows in one array).
CREATE FUNCTION emit_changes(v video_stream, emit text)
RETURNS STRUCT(at number, "select" text, change text, text text, html text)[]
  AS 'target/wasm32-wasip2/release/emit_changes.wasm', 'emit_changes' LANGUAGE wasm;

COPY (
  SELECT ffrwd.blitz.compose(
    s.video[1],
    ARRAY[ffmpeg.format(s.video[1], 'rgba')],
    emit_changes(s.video[1], emit => '[
      [1.0, {"select": "#lower", "text": "From a stream"}],
      [1.0, {"at": 1.5, "select": "#lower", "change": "+on"}],
      [2.0, {"select": "#second", "change": "+on"}],
      [3.0, [{"select": "#lower", "text": "Changed"}, {"select": "#lower", "change": "+alt"}]]
    ]'),
    html => '<style>
      body { margin: 0; background: #000; font-family: sans-serif; }
      #v { position: absolute; width: 100vw; height: 100vh; }
      #second { position: absolute; left: 320px; top: 0; width: 320px; height: 180px;
                transform: scaleX(-1); display: none; }
      #second.on { display: block; }
      #lower { position: absolute; left: 40px; top: 260px; padding: 10px 20px; font-size: 32px;
               color: #fff; background: #123; transform: translateX(-700px);
               transition: transform 0.5s ease-out; }
      #lower.on { transform: none; }
      #lower.alt { background: #a30; }
    </style><img id="v" src="ffrwd:0"><img id="second" src="ffrwd:1"><div id="lower"></div>',
    lead => 0,
    log => 'summary')
  FROM input('testsrc2=size=640x360:rate=30:duration=4', format => 'lavfi') s
) TO :'dest' WITH (video_codec 'ffv1', pix_fmt 'yuv444p')
