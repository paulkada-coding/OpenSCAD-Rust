// Supporto parametrico: modello di prova per il viewer.

larghezza = 60;   // [20:1:120]
profondita = 40;  // [20:1:100]
spessore = 6;     // [2:0.5:12]
diametro_foro = 8; // [3:0.5:15]
$fn = 64;

difference() {
    union() {
        cube([larghezza, profondita, spessore]);
        translate([0, profondita - spessore, 0])
            cube([larghezza, spessore, profondita]);
        translate([larghezza / 2, profondita / 2, spessore])
            cylinder(h = 10, d = diametro_foro * 2);
    }
    translate([larghezza / 2, profondita / 2, -1])
        cylinder(h = spessore + 12, d = diametro_foro);
    for (x = [larghezza * 0.2, larghezza * 0.8])
        translate([x, profondita + 1, profondita * 0.6])
            rotate([90, 0, 0])
                cylinder(h = spessore + 2, d = diametro_foro);
}
