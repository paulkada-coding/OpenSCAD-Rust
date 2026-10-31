module c() children();
c() { cube(); sphere(); }
module c0() children(0);
c0() { cube(); sphere(); }
module c1() children([0, 1]);
c1() { cube(); sphere(); cylinder(); }
module cr() children([0:1]);
cr() { cube(); sphere(); cylinder(); }
