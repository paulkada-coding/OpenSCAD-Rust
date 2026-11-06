// A polyhedron closed only by position (repeated points), as BOSL2 makes them:
// OpenSCAD merges the points silently.
pts = [[0,0,0],[1,0,0],[1,1,0],[0,1,0],[0,0,1],[1,0,1],[1,1,1],[0,1,1], [0,0,1],[1,0,1],[1,1,1],[0,1,1]];
faces = [[0,1,2,3],[11,10,9,8] /*top uses dups*/,[0,4,5,1],[1,5,6,2],[2,6,7,3],[3,7,4,0]];
union() { polyhedron(pts, faces); translate([2,0,0]) cube(); }
