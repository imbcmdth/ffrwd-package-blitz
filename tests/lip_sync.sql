-- Lip sync through drops and rejoins. The held input is one source with a
-- picture and its sound: a white flash and a 50 ms beep of 1 kHz at the
-- same instant, every whole second of its own time, for 12 s. Its
-- timestamps jump 1.5 s forward at 4 s and again at 8 s, so the host sees
-- three feeds, each anchored afresh. Each feed's picture starts 0.25 s
-- after its sound (frames before 0.25 s into each 4 s are dropped), as a
-- join in the middle of a GOP does: sound held on an anchor of its own
-- would be fixed a quarter of a second off the picture. With the picture
-- and the sound in one hold group, both are timed by the picture's offset.
--
-- What the output holds, from that: three feeds, each showing the flashes
-- at 1, 2 and 3 s of its segment (the one at 0 s is in the dropped
-- quarter second), nine flashes in all, each with its beep on the same
-- instant, sound minus picture 0 ms within a millisecond. The beeps at 0,
-- 4 and 8 s fall before each feed's first picture and are not heard.
-- `--ffrwd-squelch: none` leaves the input heard in the silences, so no
-- beep's onset is ramped in. `tests/lip_sync.py` checks those numbers. Where the second
-- and third feeds start on the clock depends on when the host has held
-- `lead` of each, which moves by a few frames from run to run, so the
-- frames are not compared byte for byte; the matroska output's 1 ms time
-- base is what puts a flash up to 0.33 ms off its beep.
--
-- variables: dest (output path, a .mkv)
-- example: ffrwd run -f tests/lip_sync.sql -v dest=lip_sync.mkv
--          python tests/lip_sync.py lip_sync.mkv
COPY (
  WITH c AS (
    SELECT (ffrwd.blitz.compose_av(
      k.video[1], ARRAY[f.video[1]], audio => ARRAY[f.audio[1]],
      html => '<style>html, body { margin: 0; background: #000; }
        #m { position: absolute; left: 0; top: 0; width: 320px; height: 180px;
             --ffrwd-squelch: none; }</style><img id="m" src="ffrwd:1">',
      lead => 0.5)).*
    FROM input('color=c=black:s=320x180:r=60:d=18', format => 'lavfi') k,
         input('color=c=black:s=320x180:r=60:d=12,drawbox=x=0:y=0:w=iw:h=ih:color=white:t=fill:enable=''lt(mod(t,1),0.05)'',select=''gte(mod(t,4),0.25)'',setpts=''PTS+(gte(T,4)+gte(T,8))*1.5/TB''[out0];aevalsrc=''0.5*sin(2*PI*1000*t)*lt(mod(t,1),0.05)|0.5*sin(2*PI*1000*t)*lt(mod(t,1),0.05)'':s=48000:d=12,asetpts=''PTS+(gte(T,4)+gte(T,8))*1.5/TB''[out1]',
               format => 'lavfi') f
  )
  SELECT c.video, c.audio FROM c
) TO :'dest' WITH (video_codec 'ffv1', audio_codec 'pcm_s16le')
