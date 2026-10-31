// convexity: rotate_extrude uses max(2, c); linear_extrude prints it only when > 1.
rotate_extrude(convexity = 0) square();
rotate_extrude(convexity = 3) square();
rotate_extrude(convexity = -5) square();
linear_extrude(1, convexity = 0) square();
linear_extrude(1, convexity = -2) square();
linear_extrude(1, convexity = 2) square();
