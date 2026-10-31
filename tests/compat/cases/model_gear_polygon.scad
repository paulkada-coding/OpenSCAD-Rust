// Simple gear from polygon + list comprehension.
teeth = 12;
r_root = 10;
r_tip = 12;
function gear_points(n, r1, r2) = [
  for (i = [0:n - 1]) each [
    [r1 * cos(360 * i / n), r1 * sin(360 * i / n)],
    [r2 * cos(360 * (i + 0.25) / n), r2 * sin(360 * (i + 0.25) / n)],
    [r2 * cos(360 * (i + 0.5) / n), r2 * sin(360 * (i + 0.5) / n)],
    [r1 * cos(360 * (i + 0.75) / n), r1 * sin(360 * (i + 0.75) / n)]
  ]
];
pts = gear_points(teeth, r_root, r_tip);
echo(len(pts));
linear_extrude(height = 5, convexity = 4)
  difference() {
    polygon(pts);
    circle(r = 3, $fn = 24);
  }
