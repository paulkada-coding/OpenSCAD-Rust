echo(is_undef(undef), is_undef(0), is_bool(true), is_bool(0), is_num(1), is_num(0/0), is_num(1/0), is_num("1"));
echo(is_string(""), is_string([]), is_list([]), is_list([0:1]), is_list("ab"));
echo(is_function(function(x) x), is_object(1));
echo(is_function(len));
