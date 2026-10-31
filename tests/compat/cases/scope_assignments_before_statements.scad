// All assignments of a scope run before its module instantiations.
echo("statement 1");
x = echo("assign x") 5;
echo("statement 2", x);
y = echo("assign y", x) x + 1;
