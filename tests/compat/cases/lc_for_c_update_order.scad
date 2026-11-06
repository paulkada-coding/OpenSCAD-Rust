// C-style for: updates run in order and see the ones before them.
echo(1, [for (i=0, a=0; i<3; b=i*10, a=b+1, i=i+1) [i,a]]);
echo(2, [for (i=0, a=5; i<3; i=i+1, a=i) [i,a]]);
echo(3, [for (i=0; i<3; i=i+1, i=i+1) i]);
echo(4, [for (i=0, j=i+1; i<3; i=i+1) [i,j]]);
echo(5, [for (i=0, a=0; i<3; a=i, i=i+1) [i,a]]);
p = [[0,0],[3,4],[3,0]];
echo(6, [for (i=0, total=0; i<len(p); v=p[(i+1)%3]-p[i], d=norm(v), total=total+d, i=i+1) total]);
