module ch() { $y = "from ch"; children(); }
ch() echo("child $y", $y);
module ch2() { children(); }
module outer() { $z = "outer"; ch2() echo("child $z", $z); }
outer();
module setter() { $w = "setter"; children(); }
$w = "top";
setter() echo("child $w", $w);
