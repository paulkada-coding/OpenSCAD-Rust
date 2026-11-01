// Scalar literals accepted vs non-literals ignored.
count = 10;
neg = -2.5;
pos_unary = +4;
double_neg = --7;
paren_num = -(12.5);
hex_num = 0x10;
$fn = 32;
flag_on = true;
flag_off = false;
title = "hello";

// Ignored expressions:
not_bool = !false;
missing = undef;
rng = [1:10];
computed = count + 1;
