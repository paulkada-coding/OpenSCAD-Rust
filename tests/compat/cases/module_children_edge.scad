module c(i) children(i);
c(5) { cube(); }
c(-1) { cube(); }
c("a") cube();
c(1.7) { cube(); sphere(); }
module none() children();
none();
