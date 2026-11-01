// rotate_extrude of a profile computed by a function.
function profile(h, steps) = concat(
  [[0, 0]],
  [for (i = [0:steps]) [10 + 3 * sin(i * 360 / steps), h * i / steps]],
  [[0, h]]
);
p = profile(40, 8);
echo(p);
rotate_extrude($fn = 48) polygon(p);
