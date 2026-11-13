# Widens the logo into the README banner: the logo in the middle, side panels
# carrying the scratches of the logo's own background (high-passed so patch
# tones don't show). Run by build.sh.
#
#   banner.py <logo> <out> <work dir> <width at the logo's scale>
import random, subprocess, sys
logo, out, tmp = sys.argv[1], sys.argv[2], sys.argv[3]
random.seed(1)
W = int(sys.argv[4])          # canvas width at the logo's scale; height is the logo's
L = (W - 1024) // 2
P, F = 150, 30
src = [(22, 22), (852, 22), (22, 852), (852, 852)]
ops = ["-rotate 0", "-rotate 90", "-rotate 180", "-rotate 270", "-flop", "-flip", "-transpose", "-transverse"]
run = lambda c: subprocess.run(c, shell=True, check=True)
hp = "\\( +clone -blur 0x12 \\) +swap -compose mathematics -define compose:args=0,1,-1,0.5 -composite"
run(f"magick -size {P}x{P} xc:black -fill white -draw 'circle {P//2},{P//2} {P//2},{F//2}' -blur 0x{F/2.5} {tmp}/pmask.png")
for i, (x, y) in enumerate(src):
    for j, op in enumerate(ops):
        run(f"magick {logo} -crop {P}x{P}+{x}+{y} +repage {hp} {op} {tmp}/pmask.png -alpha off -compose copyopacity -composite {tmp}/p{i}{j}.png")
det = [f"magick -size {W}x1024 xc:gray50"]
for _ in range(2600 * W // 3072):
    x, y = random.randint(-P // 2, W - P // 2), random.randint(-P // 2, 1024 - P // 2)
    if L + 150 < x < L + 1024 - 150 - P:
        continue
    det.append(f"{tmp}/p{random.randrange(4)}{random.randrange(8)}.png -geometry +{x}+{y} -compose over -composite")
det.append(f"{tmp}/detail.png")
run(" ".join(det))
# base tone + detail, then the logo's own worn top/bottom bands (corners left out) and side bands
run(f"magick -size {W}x1024 xc:'rgb(48,50,36)' {tmp}/detail.png -compose mathematics -define compose:args=0,1,1,-0.5 -composite {tmp}/panel.png")
run(f"magick -size 704x70 gradient:white-black -sigmoidal-contrast 4,35% {tmp}/tmask.png")
run(f"magick {logo} -crop 704x70+160+0 +repage {tmp}/tmask.png -alpha off -compose copyopacity -composite {tmp}/top.png")
run(f"magick {logo} -crop 704x70+160+954 +repage \\( {tmp}/tmask.png -flip \\) -alpha off -compose copyopacity -composite {tmp}/bot.png")
run(f"magick -size 704x70 gradient:white-black -sigmoidal-contrast 4,35% -rotate -90 -crop 70x1024+0+0 +repage -resize 70x1024! {tmp}/smask.png")
run(f"magick {logo} -crop 70x1024+0+0 +repage {tmp}/smask.png -alpha off -compose copyopacity -composite {tmp}/left.png")
run(f"magick {logo} -crop 70x1024+954+0 +repage \\( {tmp}/smask.png -flop \\) -alpha off -compose copyopacity -composite {tmp}/right.png")
cmd = [f"magick {tmp}/panel.png"]
for k, x in enumerate(range(0, W, 704)):
    f = "-flop" if k % 2 else ""
    cmd.append(f"\\( {tmp}/top.png {f} \\) -geometry +{x}+0 -composite \\( {tmp}/bot.png {f} \\) -geometry +{x}+954 -composite")
cmd.append(f"{tmp}/left.png -geometry +0+0 -composite {tmp}/right.png -geometry +{W-70}+0 -composite")
run(f"magick -size 1024x1024 xc:black -fill white -draw 'rectangle 110,70 913,953' -blur 0x35 {tmp}/lmask.png")
cmd.append(f"\\( {logo} {tmp}/lmask.png -alpha off -compose copyopacity -composite \\) -geometry +{L}+0 -compose over -composite")
cmd.append(f"-resize 1800x -strip -quality 88 {out}")
run(" ".join(cmd))
