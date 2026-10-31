// sharp offset of a concave polygon
star = [for (i = [0:9]) let (r = i % 2 == 0 ? 5 : 2, a = i * 36) [r * cos(a), r * sin(a)]];
linear_extrude(1) offset(delta = 0.5) polygon(star);
