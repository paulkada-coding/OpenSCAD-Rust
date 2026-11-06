// polyhedron() faces: invalid faces are dropped; triangles= is not accepted.
polyhedron(points=[[0,0,0],[1,0,0],[0,1,0],[0,0,1]], triangles=[[0,1,2],[0,3,1],[0,2,3],[1,3,2]]);
polyhedron(points=[[0,0,0],[1,0,0],[0,1,0],[0,0,1]], faces=[[0,1,2],[0,3,1],[0,2,3],[1,3,2]], triangles=[[0,1,2]]);
polyhedron(points=[[0,0,0],[1,0,0],[0,1,0],[0,0,1]]);
polyhedron(points=[[0,0,0],[1,0,0],[0,1,0],[0,0,1]], faces=5);
polyhedron(points=[[0,0,0],[1,0,0],[0,1,0],[0,0,1]], faces=[[0,1,"a"],[0,2,3]]);
polyhedron(points=[[0,0,0],[1,0,0],[0,1,0],[0,0,1]], faces=[1,[0,1,2]]);
polyhedron(points=[[0,0,0],[1,0,0],[0,1,0],[0,0,1]], faces=[[0,1,-2]]);
polyhedron(points=[[0,0,0],[1,0,0],[0,1,0],[0,0,1]], faces=[[0,1,2.7],[0,1,4]]);
polyhedron(points=[]);
polyhedron(points=undef, faces=[[0]]);
polyhedron(points=7);
