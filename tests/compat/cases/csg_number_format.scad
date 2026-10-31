// CSG numbers use C printf %g (6 digits, 2-digit exponent, ties to even).
cube([0.00001, 0.0001, 0.000015]);
cube([1e6, 123456, 1234567]);
cube([1234565, 1234585, 123456.5]);
cube([1/0, -1/0, 0/0]);
cube([1e20, 1e100, 1e-100]);
cube([999999.5, 0.1 + 0.2, 2^53]);
sphere(r = 1e-5, $fn = 1e6);
