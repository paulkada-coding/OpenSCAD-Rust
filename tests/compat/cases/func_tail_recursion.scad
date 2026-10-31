// Tail calls in the "cond ? a : f(...)" form are iterated; limit is 1000000 iterations.
function tail(n, acc = 0) = n == 0 ? acc : tail(n - 1, acc + 1);
echo(tail(1000));
echo(tail(999999));
