-- Presence spread over workers: an ad held onto the programme drives an
-- L-bar and a countdown from the host's record of its feed alone. The ad
-- is held for `lead` before it shows, so its start is known on the first
-- tick, 2.5 s ahead: a box fades in there and counts the whole seconds
-- down, the programme squeezes into the top right as the ad's first frame
-- shows, and grows back 0.75 s before the ad's last frame leaves. The
-- frames are the same at every worker count: run it with --jobs 1 and
-- --jobs 4 and compare them with framemd5.
--
-- variables: dest (output path, a .mkv)
-- example: ffrwd run -f tests/presence.sql -v dest=presence.mkv --jobs 4
COPY (
  SELECT ffrwd.blitz.compose(
    p.video[1],
    ARRAY[a.video[1]],
    html => '<!DOCTYPE html><html><head><style>
      html, body { margin: 0; overflow: hidden; background: #000; font-family: sans-serif; }
      #ad, #prog { position: absolute; left: 0; top: 0; width: 1280px; height: 720px; }
      #prog { transform-origin: 100% 0; transition: transform 0.75s ease-in-out; }
      #stage.lbar #prog { transform: scale(0.75); }
      #count { position: absolute; left: 40px; top: 600px; padding: 10px 24px; color: #fff;
               background: rgba(8, 18, 52, 0.92); font-size: 40px; opacity: 0;
               transition: opacity 0.5s; }
      #count.on { opacity: 1; }
    </style></head><body><div id="stage">
      <img id="ad" src="ffrwd:1"><img id="prog" src="ffrwd:0">
      <div id="count">Break in <span class="n"></span></div>
    </div></body></html>',
    presence => '{"input": 1, "on": {"select": "#stage", "change": "~lbar"},
                  "coming": {"select": "#count", "change": "~on"},
                  "countdown": "#count .n", "lead_out": 0.75}',
    css_width => 1280,
    lead => 2.5)
  FROM input('testsrc2=size=1920x1080:rate=30:duration=10', format => 'lavfi') p,
       input('testsrc=size=640x360:rate=30:duration=5', format => 'lavfi') a
) TO :'dest' WITH (video_codec 'ffv1')
