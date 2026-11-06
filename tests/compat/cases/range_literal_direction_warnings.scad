// Only ranges written with number literals warn about their direction.
n = -1;
col = 0;
echo(1, [0:1:n], [0:1:2-3], [0:n], [n:-1:3], [PI:1]);
echo(2, [0:1:-(1)], [2:0], [2:-1:3], [+3:+1], [3:-(-1):1], [0:-1:-(-1)]);
a = [for (i = [0:1:col-1]) i];
function f(x) = [for (i = [x:1]) i];
echo(3, a, f(5));
for (i = [5:1]) echo(i);
