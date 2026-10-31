render() cube();
render(convexity = 2) cube();
render(convexity = 0) cube();
hull() { cube(); sphere(); }
hull();
minkowski() { cube(); sphere(); }
minkowski(convexity = 3) { cube(); sphere(); }
minkowski();
