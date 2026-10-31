function f(x) = assert(x > 0) x;
function g(y) = f(y - 1) + 1;
module m(a, b = 2) { echo(g(a)); }
echo(f(2));
m(0, b = [1, 2]);
