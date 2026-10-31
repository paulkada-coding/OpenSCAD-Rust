linear_extrude(10) square();
linear_extrude(height = 5, center = true, convexity = 10, twist = 90, slices = 20, scale = 0.5, $fn = 16) square();
linear_extrude(height = 5, scale = [1, 2], twist = -45) circle();
linear_extrude() square();
linear_extrude(height = 2, center = false, twist = 0, scale = 1, convexity = 1, slices = 1) square();
linear_extrude(height = 2, scale = [1, 1]) square();
linear_extrude(height = 2, scale = [2, 2]) square();
linear_extrude(height = 2, convexity = 2, $fn = 5, $fa = 6, $fs = 0.5) square();
