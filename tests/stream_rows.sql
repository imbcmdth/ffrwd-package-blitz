-- End to end: change rows that arrive with the frames. `emit_changes` (the
-- test module in tests/producer) passes the programme through and emits
-- the rows listed in `emit` beside it; `compose` reads them from its
-- `stream_changes` column. Build the modules first, then from the package
-- directory:
--   ffrwd run -f tests/stream_rows.sql -v dest=stream_rows.mkv
-- The lower third shows "From a stream" from 1.0 s (a row with no `at`,
-- taking the frame it arrives with), its box slides in at 1.5 s (a row that
-- arrives at 1.0 s scheduled for 1.5), and "Changed" replaces the text at
-- 3.0 s (two rows in one array).
CREATE FUNCTION emit_changes(v video_stream, emit text)
RETURNS STRUCT(v video_stream,
               changes STRUCT(at number, "select" text, change text, text text, html text)[])
  AS 'target/wasm32-wasip2/release/emit_changes.wasm', 'emit_changes' LANGUAGE wasm;

COPY (
  SELECT ffrwd.blitz.compose(
    emit_changes(s.video[1], emit => '[
      [1.0, {"select": "#lower", "text": "From a stream"}],
      [1.0, {"at": 1.5, "select": "#lower", "change": "+on"}],
      [3.0, [{"select": "#lower", "text": "Changed"}, {"select": "#lower", "change": "+alt"}]]
    ]'),
    html => '<style>
      body { margin: 0; background: #000; font-family: sans-serif; }
      #v { position: absolute; width: 100vw; height: 100vh; }
      #lower { position: absolute; left: 40px; top: 260px; padding: 10px 20px; font-size: 32px;
               color: #fff; background: #123; transform: translateX(-700px);
               transition: transform 0.5s ease-out; }
      #lower.on { transform: none; }
      #lower.alt { background: #a30; }
    </style><img id="v" src="ffrwd:0"><div id="lower"></div>',
    log => 'summary')
  FROM input('testsrc2=size=640x360:rate=30:duration=4', format => 'lavfi') s
) TO :'dest' WITH (video_codec 'ffv1', pix_fmt 'yuv444p')
