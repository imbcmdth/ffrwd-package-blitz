-- End to end: closed captions from the cues `compose_av` draws. Two
-- casters' cue rows come from `emit_changes` (the test module in
-- tests/producer), as a transcriber writes them, growing and then final;
-- `compose_av` draws them into one display and writes them on `text`, each
-- with its caster's name from the document. The document's rows switch the
-- closed captions on at 1.0 s, rename caster 1 at 2.5 s, and switch them off
-- at 3.5 s and on again at 4.0 s.
--
-- Each stream of `cues` is in the hold group of the held input in its place,
-- as a cam's rows are with its picture, so there are two cams. The sources
-- run in real time, as a live programme's do. Build the modules first, then
-- from the package directory:
--   ffrwd run -f tests/closed_captions.sql -v dest=closed_captions.ndjson
--   python tests/closed_captions.py closed_captions.ndjson
-- tests/closed_captions.py checks the rows, their times on the programme's
-- timeline and their voices.
CREATE FUNCTION emit_cues(v video_stream, emit text, schema text)
RETURNS STRUCT(text text, start_t number, end_t number, final boolean)[]
  AS 'target/wasm32-wasip2/release/emit_changes.wasm', 'emit_changes' LANGUAGE wasm;

COPY (
  SELECT ffrwd.blitz.compose_av(
      k.video[1],
      ARRAY[cam1.video[1], cam2.video[1]],
      a => s.audio[1],
      cues => ARRAY[
        emit_cues(k.video[1], emit => '[
          [0.5, {"text": "he goes", "start_t": 10.0, "end_t": 10.4, "final": false}],
          [1.5, {"text": "he goes for the", "start_t": 10.0, "end_t": 10.9, "final": false}],
          [2.0, {"text": "he goes for the flank", "start_t": 10.0, "end_t": 11.3, "final": true}],
          [3.0, {"text": "and that", "start_t": 20.0, "end_t": 20.4, "final": false}],
          [3.8, {"text": "and that is game", "start_t": 20.0, "end_t": 21.1, "final": false}],
          [4.5, {"text": "and that is game five", "start_t": 20.0, "end_t": 21.6, "final": true}]
        ]', schema => '{"type": "object", "properties": {"text": {"type": "string"},
                        "start_t": {"type": "number"}, "end_t": {"type": "number"},
                        "final": {"type": "boolean"}}, "required": ["text"]}'),
        emit_cues(k.video[1], emit => '[
          [1.2, {"text": "what a read", "start_t": 5.0, "end_t": 5.8, "final": true}],
          [2.2, {"text": "it was close", "start_t": 7.0, "end_t": 7.6, "final": false}],
          [2.6, {"text": "it was close one", "start_t": 7.0, "end_t": 8.0, "final": true}]
        ]', schema => '{"type": "object", "properties": {"text": {"type": "string"},
                        "start_t": {"type": "number"}, "end_t": {"type": "number"},
                        "final": {"type": "boolean"}}, "required": ["text"]}')],
      captions => '#captions',
      speakers => '#caster{n}',
      html => '<style>body { margin: 0; background: #000; color: #fff; font-size: 20px; }
        .names { display: none; }</style>
        <p id="captions"></p>
        <p class="names"><span id="caster1">Mango</span><span id="caster2">Ludwig</span></p>',
      rows => '[{"at": 1.0, "select": "body", "change": "+captions-closed"},
                {"at": 2.5, "select": "#caster1", "text": "MangoPlays"},
                {"at": 3.5, "select": "body", "change": "-captions-closed"},
                {"at": 4.0, "select": "body", "change": "+captions-closed"}]',
      lead => 0).text
  FROM input('color=c=black:s=320x180:r=30:d=6', format => 'lavfi', realtime => true) k,
       input('anullsrc=r=48000:cl=stereo:d=6', format => 'lavfi', realtime => true) s,
       input('color=c=navy:s=320x180:r=30:d=6', format => 'lavfi', realtime => true) cam1,
       input('color=c=maroon:s=320x180:r=30:d=6', format => 'lavfi', realtime => true) cam2
) TO :'dest'
