rotate_extrude() translate([2, 0]) circle();
rotate_extrude(angle = 180, convexity = 4, $fn = 30) translate([2, 0]) square();
rotate_extrude(start = 90, angle = -90) translate([2, 0]) square();
rotate_extrude(angle = 360) square();
rotate_extrude(start = 0) square();
rotate_extrude(angle = 90, start = 45) square();
rotate_extrude(angle = 400) square();
rotate_extrude(convexity = 1) square();
