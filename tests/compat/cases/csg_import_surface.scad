import("data/tri.stl");
import(file = "data/tri.stl", convexity = 3);
import("data/missing.stl");
import("data/x.svg", center = true, dpi = 96);
import("data/x.dxf", layer = "a", origin = [1, 2], scale = 2);
surface("data/surf.dat");
surface(file = "data/surf.dat", center = true, invert = true, convexity = 2);
surface("data/nofile.dat");
