// A tail call keeps the $ variables of the calls it replaces.
function t(n) = n > 0 ? t(n-1) : $fn;
echo(t(2, $fn=24));
function u(n) = n > 0 ? u(n-1, $fn=$fn+1) : $fn;
echo(u(3, $fn=10));
function w(n) = let($fa = 7) n > 0 ? w(n-1) : [$fa, $fn];
echo(w(2, $fn=5));
function deep(n) = n > 0 ? deep(n-1) : $fs;
echo(deep(5000, $fs=0.5));
