module p() { echo("p sees", $x); }
module q() { $x = 3; p(); }
q();
$x = 1;
p();
function fx() = $x;
module r() { $x = 42; echo("r fx", fx()); }
r();
