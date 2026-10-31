$fn = 5;
module m() { echo("m $fn", $fn); children(); }
module n() { $fn = 7; m() sphere(); }
n();
m() { $fn = 9; echo("child", $fn); }
