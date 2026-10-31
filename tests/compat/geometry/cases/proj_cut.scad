// section of a sphere at z = 0
linear_extrude(1) projection(cut = true) translate([0, 0, -1]) sphere(3, $fn = 16);
