function fact(n) = n <= 1 ? 1 : n * fact(n - 1);
echo(fact(5), fact(10), fact(20), fact(170), fact(171));
function fib(n) = n < 2 ? n : fib(n - 1) + fib(n - 2);
echo([for (i = [0:15]) fib(i)]);
function sum(v, i = 0) = i >= len(v) ? 0 : v[i] + sum(v, i + 1);
echo(sum([1, 2, 3, 4, 5]));
