// Stacked rotated slabs using for, rotate and color.
n = 8;
for (i = [0:n - 1])
  color([i / n, 0.5, 1 - i / n])
    translate([0, 0, i * 2])
      rotate(i * 360 / n / 4)
        cube([10 - i, 10 - i, 2], center = true);
