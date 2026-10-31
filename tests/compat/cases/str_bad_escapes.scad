// Unknown escapes: the backslash is dropped and a warning is printed (max 5 per file).
echo("\q");
echo("\x7", "\x80", "\a", "\0");
echo("\u12");
echo("\v\b\f");
