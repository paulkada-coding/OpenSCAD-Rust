$fn = 12;
function getfn() = $fn;
module m() { $fn = 24; echo("module", getfn()); }
m();
echo("top", getfn());
echo(let ($fn = 6) getfn());
