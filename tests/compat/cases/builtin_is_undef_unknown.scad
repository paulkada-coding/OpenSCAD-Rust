// is_undef() of a bare unknown variable gives no warning.
echo(is_undef(nope), is_undef($nope), is_undef((nope2)));
function g() = is_undef(nope3) ? 0 : nope3;
echo(g());
echo(is_undef(nope4 + 1));
function slop() = is_undef($slop) ? 0 : $slop;
echo(slop(), slop($slop = 0.2));
