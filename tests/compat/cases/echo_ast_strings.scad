// String literals inside printed expressions ARE escaped.
echo(function(c = "s\"q\n") c);
echo(function() "t\tu\\\r");
