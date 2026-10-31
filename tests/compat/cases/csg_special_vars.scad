$fn = 10;
circle();
module n() { $fn = 3; circle(); }
n();
circle($fn = 4);
sphere($fa = 5, $fs = 0.1);
