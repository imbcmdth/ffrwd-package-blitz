-- An L-bar break over a programme: at 2 s the programme shrinks into the
-- top right over 0.75 s and shows the ad input behind it, a lower third
-- slides in with its name set by a change row, a looping logo starts when
-- a row inserts it and stops when another removes it, and at 8.5 s the
-- programme grows back. 12 s of 1080p30, the document designed at 1280x720
-- CSS px.
--
-- The programme is lavfi testsrc2. The ad input is a second compose1
-- rendering an HTML page on the programme's clock: ffrwd runs a module
-- reading several streams only when they come from one point through
-- modules that emit one frame per frame in, so two independent sources
-- cannot be its two inputs (see the README).
--
-- variables: dest (output path)
-- example: ffrwd run ffrwd/blitz:lbar -v dest=lbar.mkv
COPY (
  WITH p AS (
    SELECT s.video[1] AS v
    FROM input('testsrc2=size=1920x1080:rate=30:duration=12', format => 'lavfi') s
  )
  SELECT ffrwd.blitz.compose2(
    p.v,
    ffrwd.blitz.compose1(
      p.v,
      html => '<!DOCTYPE html><html><head><style>
        html, body { margin: 0; height: 100%; overflow: hidden; font-family: sans-serif; color: #fff;
                     background: linear-gradient(135deg, #f7b733, #fc4a1a); }
        #t { position: absolute; left: 34px; top: 40px; width: 260px; font-size: 64px;
             font-weight: bold; line-height: 70px; }
        #s { position: absolute; left: 36px; top: 200px; width: 250px; font-size: 26px; }
        #ball { position: absolute; left: 70px; top: 300px; width: 160px; height: 160px;
                border-radius: 80px; background: rgba(255, 255, 255, 0.35);
                animation: bob 2s ease-in-out infinite alternate; }
        @keyframes bob { from { transform: translateY(0) } to { transform: translateY(80px) } }
      </style></head><body><div id="t">BIG SALE</div>
      <div id="s">This weekend only, at your local store</div><div id="ball"></div></body></html>',
      css_width => 1280),
    html => '<!DOCTYPE html><html><head><style>
      html, body { margin: 0; overflow: hidden; background: #000; font-family: sans-serif; }
      #ad, #prog { position: absolute; left: 0; top: 0; width: 1280px; height: 720px; }
      #prog { transform-origin: 100% 0; transition: transform 0.75s ease-in-out; }
      #stage.lbar #prog { transform: scale(0.75); }
      #lower { position: absolute; left: 330px; top: 572px; width: 920px; height: 104px;
               background: rgba(8, 18, 52, 0.92); border-left: 10px solid #fc4a1a; color: #fff;
               transform: translateX(1000px); transition: transform 0.6s ease-out; }
      #lower.on { transform: translateX(0); }
      #lower .name { position: absolute; left: 26px; top: 10px; font-size: 44px; font-weight: bold; }
      #lower .role { position: absolute; left: 26px; top: 64px; font-size: 26px; color: #cfd8ff; }
      #bug { position: absolute; left: 1130px; top: 24px; }
      .logo { width: 110px; height: 110px; border-radius: 55px; background: #1e90ff; color: #fff;
              font-size: 30px; font-weight: bold; line-height: 110px; text-align: center;
              animation: pulse 1.2s ease-in-out infinite; }
      @keyframes pulse { 0%, 100% { transform: scale(1) } 50% { transform: scale(1.15) } }
    </style></head><body><div id="stage">
      <img id="ad" src="ffrwd:1"><img id="prog" src="ffrwd:0">
      <div id="lower"><div class="name"></div><div class="role">Correspondent, on location</div></div>
      <div id="bug"></div>
    </div></body></html>',
    changes => '[
      {"at": 2.0, "select": "#stage", "change": "+lbar"},
      [{"at": 2.75, "select": "#lower .name", "text": "Jane Example"},
       {"at": 2.75, "select": "#lower", "change": "+on"}],
      {"at": 3.5, "select": "#bug", "html": "<div class=\"logo\">LIVE</div>"},
      {"at": 7.0, "select": "#lower", "change": "-on"},
      {"at": 8.0, "select": "#bug", "html": ""},
      {"at": 8.5, "select": "#stage", "change": "-lbar"}
    ]',
    css_width => 1280)
  FROM p
) TO :'dest' WITH (video_codec 'libx264', preset 'veryfast', crf 18, pix_fmt 'yuv420p')
