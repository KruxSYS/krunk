#!/bin/sh
# The probe's 12 s H.264/AAC clip (not in git): scripts/vm-0300/make-clip.sh OUT.mp4
ffmpeg -loglevel error -y -f lavfi -i testsrc2=size=640x360:rate=30 -f lavfi -i sine=frequency=440:sample_rate=48000 \
  -t 12 -c:v libx264 -pix_fmt yuv420p -c:a aac -b:a 96k -movflags +faststart "${1:-clip.mp4}"
