// Classic builtin functions ignore argument names and count every argument.
echo(sin(x=30), len(v=[1,2]), concat(a=[1], [2], b=[3]), max(a=3, 5), str(a="x", "y"));
echo(pow(e=2, 3), lookup(key=1, [[0,0],[2,4]]), norm(v=[3,4]), rands(min=0,max=1,count=0));
echo(round(z=2.5), is_num(x=1), chr(c=65), atan2(y=1, 1));
echo(sin($fn=3, 30), concat($a=[1], [2]), len([1], x=[1,2]));
echo(len(), str(), concat(), chr(), ord(), ord("a","b"), is_num(), is_num(1,2));
echo(is_undef(), is_bool(), is_string(), is_list(), is_function());
echo(is_undef(1,2), is_bool(true,1), is_string("a",1), is_list([],1), is_function(1,2));
echo(parent_module(0,1), parent_module(0,1,2), rands(0,1,1,2,3));
