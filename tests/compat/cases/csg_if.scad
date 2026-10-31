if (true) cube(); else sphere();
if (false) cube(); else sphere();
if (false) cube();
translate([1, 0, 0]) if (false) cube();
union() { if (false) cube(); sphere(); }
if (true) { { cube(); } }
module m() { if (false) cube(); }
m();
