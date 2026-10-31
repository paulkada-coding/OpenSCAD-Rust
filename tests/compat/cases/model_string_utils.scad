// String utilities built from the builtins.
function join(v, sep = ",") = len(v) == 0 ? "" : len(v) == 1 ? str(v[0]) : str(v[0], sep, join([for (i = [1:len(v) - 1]) v[i]], sep));
function upper(s) = chr([for (c = s) let (o = ord(c)) o >= 97 && o <= 122 ? o - 32 : o]);
function reverse(s) = chr([for (i = [len(s) - 1:-1:0]) ord(s[i])]);
echo(join([1, 2, 3]), join(["a", "b"], " - "), join([]));
echo(upper("Hello, World!"));
echo(reverse("abcdef"));
echo(search("o", "hello world", 0));
