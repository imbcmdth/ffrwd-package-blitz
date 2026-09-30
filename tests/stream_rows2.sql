-- End to end: change rows that arrive with the first input's frames of a
-- module reading two. `emit_changes` (the test module in tests/producer)
-- passes the programme through and emits the rows listed in `emit` beside
-- it; `compose2` reads them from its `stream_changes` column, which follows
-- its streams. The second input is the same programme, so both inputs come
-- from one point. Needs ffrwd 0.27.6. Build the modules first, then from
-- the package directory:
--   ffrwd run -f tests/stream_rows2.sql -v dest=stream_rows2.mkv
-- The second input, mirrored, sits hidden in the right half. The lower
-- third shows "From a stream" from 1.0 s (a row with no `at`, taking the
-- frame it arrives with), its box slides in at 1.5 s (a row that arrives at
-- 1.0 s scheduled for 1.5), the second input shows from 2.0 s, and
-- "Changed" replaces the text at 3.0 s (two rows in one array).
--
-- The compose1 in the CTE shows the programme unchanged and hands every
-- frame back uncopied; it keeps rgba on the edge that ends at a split (see
-- the README).
CREATE FUNCTION emit_changes(v video_stream, emit text)
RETURNS STRUCT(v video_stream,
               changes STRUCT(at number, "select" text, change text, text text, html text)[])
  AS 'target/wasm32-wasip2/release/emit_changes.wasm', 'emit_changes' LANGUAGE wasm;

COPY (
  WITH p AS (
    SELECT ffrwd.blitz.compose1(s.video[1]) AS v
    FROM input('testsrc2=size=640x360:rate=30:duration=4', format => 'lavfi') s
  )
  SELECT ffrwd.blitz.compose2(
    emit_changes(p.v, emit => '[
      [1.0, {"select": "#lower", "text": "From a stream"}],
      [1.0, {"at": 1.5, "select": "#lower", "change": "+on"}],
      [2.0, {"select": "#second", "change": "+on"}],
      [3.0, [{"select": "#lower", "text": "Changed"}, {"select": "#lower", "change": "+alt"}]]
    ]'),
    p.v,
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
    log => 'summary')
  FROM p
) TO :'dest' WITH (video_codec 'ffv1', pix_fmt 'yuv444p')
