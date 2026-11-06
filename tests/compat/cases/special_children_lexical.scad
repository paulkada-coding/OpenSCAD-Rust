// $children is the child count of the module whose body contains the code,
// also when that code runs as the children of another module.
module att() { $x = 1; children(); }
module poly() att() { echo("poly", $children, $x, $parent_modules); att() echo("nested", $children); }
poly();
poly() cube();
att() echo("top", $children);
module wrap() { children(); }
module outer() wrap() wrap() echo("outer", $children);
outer() { cube(); cube(); }
module guarded() att() { if ($children > 0) echo("has children"); else echo("no children"); }
guarded();
guarded() cube();
