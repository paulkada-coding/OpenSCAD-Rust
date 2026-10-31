// Helper library for include_*/use_* cases (not a case itself).
libvar = 42;
echo("lib top-level echo");
function libf(x) = x * libvar;
module libm() { cube(libvar); }
cube(7);
