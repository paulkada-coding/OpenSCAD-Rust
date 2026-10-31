a = 1;
f = function() a;
a = 2;
echo(f());
function make(k) = function(x) x * k;
m3 = make(3);
echo(m3(4));
