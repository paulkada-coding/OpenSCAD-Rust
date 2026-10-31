group() { cube(); }
{ cube(); sphere(); }
translate([1, 0, 0]) { { cube(); sphere(); } }
union() { { cube(); } { sphere(); } }
module m() { { cube(); } }
m();
