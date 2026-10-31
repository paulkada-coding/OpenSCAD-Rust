module m() { cube(); sphere(); }
m();
module w() { cube(); }
translate([1, 0, 0]) w();
union() w();
module empty() {}
empty();
