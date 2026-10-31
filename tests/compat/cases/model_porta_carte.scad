// =====================================================================
//  PORTA CARTE MTG - scatola e coperchio
//
//  Si scelgono solo: orientamento delle carte, numero di file,
//  lunghezza e serraggio del coperchio. Tutto il resto si ricava. Quote in mm
//  reali: nessuna scala, nessun include.
//
//  Coperchio "da scatola da scarpe": calza sopra la base, all'esterno, e
//  si appoggia sul bordo delle pareti. Per aprirlo si prende il bordo del
//  coperchio, che sporge tutto intorno, e si solleva.
//
//  I separatori sono in separatore.scad. Fra i due file devono essere
//  uguali ORIENTAMENTO, PIEDINO, TAB_H e il blocco COMUNE (copiarlo intero).
//  Per controllo, entrambi stampano in console una riga CONTROLLO:
//  deve essere identica.
// =====================================================================

/* [Pezzo] */
// stampa = scatola e coperchio affiancati sul piatto, in un solo export
// taratura = dima per scegliere SERRAGGIO: 5 anelli di scatola e 1 di coperchio
PEZZO        = "stampa";       // [stampa, scatola, coperchio, assieme, taratura]

/* [Configurazione] */
ORIENTAMENTO = "orizzontale";  // [orizzontale, verticale]
N_FILE       = 1;              // [1:1:4]
// lunghezza massima in mm, arrotondata per difetto al passo da 5
LUNGHEZZA    = 70;             // [30:1:270]
// piedino del separatore e scanalature sul fondo (come in separatore.scad):
// un separatore infilato storto resta piu' alto e traballa
PIEDINO      = true;
TAB_H        = 12;             // come in separatore.scad (14 mm sopra le carte: presa e scritta da 8)

/* [Coperchio] */
// tenuta del coperchio sui due tratti dei lati col pettine, mm per lato a fine
// corsa: positivo = preme, negativo = restringe il gioco senza premere,
// -0.25 = nessun tratto. Il valore giusto dipende dalla stampante: si sceglie
// con PEZZO = "taratura" (anello che chiude con un leggero attrito)
SERRAGGIO    = 0.3;            // [-0.25:0.05:0.6]

/* [Hidden] */
$fn = 64;
PIATTO  = 270;                 // piatto 270 x 270 x 256
PIATTO_Z = 256;
MARGINE = 2;                   // margine per lato sul piatto
GAP_STAMPA = 10;               // distanza fra i pezzi in modalita' stampa

// --- taratura: di norma non si tocca ----------------------------------
SMUSSO      = 1.2;             // smusso a 45 gradi in cima ai denti: invito per le alette
GIOCO_TOP   = 2;               // aria sopra le linguette
SPESS_TOP   = 2;               // spessore della faccia superiore del coperchio
SPESS_GONNA = 2;               // spessore della gonna, fuori dalla base
APPOGGIO    = 1.5;             // il coperchio appoggia su tanta parte del bordo della base
INVITO      = 0.6;             // invito sul filo interno della gonna
GIOCO_COP   = 0.25;            // gioco fra gonna del coperchio e base, per lato
TARATURA    = [0.2, 0.25, 0.3, 0.35, 0.4];     // valori della dima di taratura
SVASO_H     = 8;               // altezza dei tratti svasati, sotto il bordo della base
// profilo dei tratti: il punto piu' largo sta in ALTO (SVASO_H sopra il filo del
// coperchio chiuso, vicino al bordo), si restringe scendendo fino a zero sul filo.
// Per aprire, la gonna deve risalire tutta la rampa e scavalcare il punto largo:
// tiene per diversi mm di corsa e scatta. (Trovato da Ivan capovolgendo la dima.)
TRATTO_MARG = 2;               // i tratti svasati coprono tutto il tratto dritto dei lati col
                               // pettine, meno questo margine prima del raccordo degli angoli
SMUSSO_EST  = 0.8;             // smusso a 45 gradi sugli spigoli orizzontali esterni
INTAGLIO_W  = 30;              // intagli ad arco sulla gonna, lati di testa: larghezza,
INTAGLIO_H  = 4;               // altezza

// --- COMUNE INIZIO: identico in separatore.scad -----------------------
VERSIONE    = "2.7";           // versione del progetto: vedi CHANGELOG.md
CARTA_LUNGO = 92;              // carta in doppia bustina
CARTA_CORTO = 68;
SPORGENZA   = 12;              // quanto le carte escono dalla scatola:
                               // 12 = presa comoda fra pollice e indice,
                               // sotto i 10 le carte si sfilano male
LUCE_EXTRA  = 2;               // luce dello scomparto oltre la carta
SP      = 3;                   // spessore pareti
RC      = 3;                   // raggio spigoli verticali
HF      = 4;                   // spessore fondo
BATT_H  = 10;                  // altezza della battuta per il coperchio
LAB_SP  = 1.5;                 // spessore del labbro
PASSO   = 5;                   // passo del pettine
DENTE   = 3;                   // dente; cava = PASSO - DENTE
CAVA_P  = 3;                   // profondita' cava, su pareti e divisori
CAVA_L  = 30;                  // altezza del tratto con le cave
RAMPA   = 45;                  // rampa in fondo alle cave, gradi (0 = nessuna): una carta
                               // finita in una cava ne esce scivolando nello scomparto
Y0      = 11;                  // primo dente dal bordo di testa
FOND_W  = 10;                  // scanalature sul fondo per il piedino: tratto a piena profondita'
FOND_H  = 1.5;                 // profondita'; il piedino e' alto FOND_H - 0.2

CARTA_O = ORIENTAMENTO == "verticale" ? CARTA_CORTO : CARTA_LUNGO; // orizzontale
CARTA_V = ORIENTAMENTO == "verticale" ? CARTA_LUNGO : CARTA_CORTO; // verticale
SC_W    = CARTA_O + LUCE_EXTRA;          // luce dello scomparto
HZ      = HF + CARTA_V - SPORGENZA;      // altezza pareti
LAB_Z   = HZ - BATT_H;                   // inizio battuta
CAVA_Z0 = LAB_Z - CAVA_L;                // inizio cave
CAVA_W  = PASSO - DENTE;
RAMPA_H = CAVA_P*tan(RAMPA);             // altezza della rampa dal lato parete
ALT_SEP = HF + CARTA_V + 2 + TAB_H;      // cima della linguetta
// --- COMUNE FINE ------------------------------------------------------

// ---------------------------------------------------------------------
//  derivate
// ---------------------------------------------------------------------
DIV_ANIMA = 2;                                   // anima del divisorio
DIV_SP    = 2*CAVA_P + DIV_ANIMA;                // 8
N_DENTI   = max(2, floor((LUNGHEZZA - 2*Y0 - DENTE)/PASSO) + 1);

LX     = 2*(SP + CAVA_P) + N_FILE*SC_W + (N_FILE - 1)*DIV_SP;
LY     = 2*Y0 + (N_DENTI - 1)*PASSO + DENTE;
H_UTIL = LY - 2*SP;                              // spazio carte per fila

// coperchio: la gonna scende per BATT_H sulla base, all'esterno, e sporge
// di EST tutto intorno. Si appoggia sul bordo delle pareti (z = HZ).
EST     = GIOCO_COP + SPESS_GONNA;               // sporgenza del coperchio sulla base
CX      = LX + 2*EST;                            // ingombro del coperchio
CY      = LY + 2*EST;
H_COP   = ALT_SEP + GIOCO_TOP - LAB_Z + SPESS_TOP;
EPS     = 0.02;

// disposizione in modalita' stampa: affiancati in X se entrano, altrimenti in Y
UTILE      = PIATTO - 2*MARGINE;
AFF_X      = LX + GAP_STAMPA + CX <= UTILE && CY <= UTILE;
AFF_Y      = LY + GAP_STAMPA + CY <= UTILE && CX <= UTILE;
COP_DX     = (AFF_X || !AFF_Y) ? LX + GAP_STAMPA : 0;
COP_DY     = (!AFF_X && AFF_Y) ? LY + GAP_STAMPA : 0;

TRATTO_Y = LY - 2*RC - 2*TRATTO_MARG;            // lato intero meno raccordi e margine
                                                 // (40 mm con LUNGHEZZA 50: la dima di taratura)

function x_sc(i) = SP + CAVA_P + i*(SC_W + DIV_SP);   // inizio scomparto i

// ---------------------------------------------------------------------
//  console
// ---------------------------------------------------------------------
echo(str("porta carte v", VERSIONE));
echo(str("scatola   ", LX, " x ", LY, " x ", HZ, " mm, carte ", ORIENTAMENTO,
         ", ", N_FILE, " file"));
echo(str("  ", N_DENTI, " denti: ", N_DENTI - 1, " posizioni per fila, ",
         H_UTIL, " mm per le carte"));
if (LY < LUNGHEZZA)
    echo(str("  lunghezza arrotondata al passo: ", LY, " invece di ", LUNGHEZZA));
echo(str("coperchio ", CX, " x ", CY, " x ", H_COP, " mm, calza sulla base per ", BATT_H,
         " mm e sporge di ", EST, " per lato (gioco ", GIOCO_COP, ")"));
echo(str("chiuso    ", HZ + H_COP - BATT_H, " mm di altezza"));
if (GIOCO_COP + SERRAGGIO > 0)
    echo(str("tenuta    2 tratti da ", TRATTO_Y, " mm sui lati col pettine: gioco a fine corsa ",
             -SERRAGGIO, " per lato (negativo = preme), nominale ", GIOCO_COP));
if (PEZZO == "taratura")
    echo(str("taratura  anelli con SERRAGGIO ", TARATURA, " (valore inciso all'interno); ",
             "scegliere quello su cui il coperchio chiude con un leggero attrito"));
echo(str("CONTROLLO ", ORIENTAMENTO, " luce ", SC_W, " cave z ", CAVA_Z0,
         "-", LAB_Z, " cima linguetta z ", ALT_SEP, " piedino ", PIEDINO,
         " rampa ", RAMPA, " v", VERSIONE));
if (RAMPA_H > 0)
    echo(str("rampe     ", RAMPA, " gradi, alte ", RAMPA_H, " mm in fondo a ogni cava"));
if (PEZZO == "stampa") {
    echo(str("stampa    ", COP_DX > 0 ? "affiancati in X" : "affiancati in Y", ", ingombro ",
             COP_DX > 0 ? LX + GAP_STAMPA + CX : CX, " x ",
             COP_DY > 0 ? LY + GAP_STAMPA + CY : CY, " mm"));
    if (!AFF_X && !AFF_Y)
        echo(str("ATTENZIONE: scatola e coperchio insieme non entrano nel piatto (",
                 UTILE, " utili). Esportare PEZZO = scatola e PEZZO = coperchio a parte."));
}
if (CX > UTILE || CY > UTILE)
    echo(str("ATTENZIONE: il coperchio, ", CX, " x ", CY, ", non entra nel piatto (",
             UTILE, " utili). Ridurre N_FILE o LUNGHEZZA."));
if (HZ > PIATTO_Z)
    echo(str("ATTENZIONE: scatola alta ", HZ, ", piatto ", PIATTO_Z));

// ---------------------------------------------------------------------
//  scatola
// ---------------------------------------------------------------------
module p_esterno() translate([RC, RC]) offset(r = RC) square([LX - 2*RC, LY - 2*RC]);
module p_interno() translate([SP, SP]) square([LX - 2*SP, LY - 2*SP]);

// raccordo fra due contorni: fetta sottile a quota z con il contorno spostato di o
// (o negativo = verso l'interno), unite a due a due con hull
module fetta(o, z) translate([0, 0, z]) linear_extrude(EPS) offset(r = o) p_esterno();
module raccordo(o0, z0, o1, z1) hull() { fetta(o0, z0); fetta(o1, z1); }

// guscio: pareti piene fino al bordo, smusso a 45 gradi sullo spigolo del
// fondo (stesso disegno del coperchio, nasconde il piede d'elefante) e sullo
// spigolo esterno del bordo (invito per il coperchio)
module guscio(s = SERRAGGIO) {
    difference() {
        union() {
            raccordo(-SMUSSO_EST, 0, 0, SMUSSO_EST);
            // pareti fino all'inizio dello smusso del bordo (HZ - SMUSSO_EST/2)
            translate([0, 0, SMUSSO_EST - EPS])
                linear_extrude(HZ - SMUSSO_EST/2 - SMUSSO_EST + 2*EPS) p_esterno();
            raccordo(0, HZ - SMUSSO_EST/2, -SMUSSO_EST/2, HZ - EPS);
        }
        translate([0, 0, HF]) linear_extrude(HZ) p_interno();
    }
    if (GIOCO_COP + s > 0) svaso(s);
}

// tratti di tenuta, al centro dei lati col pettine e lunghi quasi tutto il lato:
// dal filo del coperchio chiuso (LAB_Z, larghezza nominale) la faccia esterna
// della base si allarga salendo fino a GIOCO_COP + SERRAGGIO a SVASO_H piu' su,
// poi torna a filo con un invito a 45 gradi. Tutto nascosto sotto la gonna.
// Chiudendo, la gonna scavalca il punto largo e scende sulla rampa; aprendo,
// deve risalirla e riscavalcarlo: la tenuta dura per mm di corsa. Sui lati di
// testa resta il gioco: l'aria passa, niente ventosa.
module svaso(s = SERRAGGIO) {
    sv = GIOCO_COP + s;
    zm = LAB_Z + SVASO_H;                          // punto piu' largo
    intersection() {
        difference() {
            union() {
                raccordo(0, LAB_Z, sv, zm);        // rampa: sale allargandosi
                raccordo(sv, zm, 0, zm + sv);      // sopra: invito a 45 gradi
            }
            translate([0, 0, LAB_Z - 2]) linear_extrude(SVASO_H + 4) p_esterno();
        }
        translate([-5, LY/2 - TRATTO_Y/2, LAB_Z - 2]) cube([LX + 10, TRATTO_Y, SVASO_H + 4]);
    }
}

// dente con la cima a tetto: gli smussi a 45 gradi portano l'aletta
// nella cava piu' vicina. Resta un piano di DENTE - 2*SMUSSO in cima.
module dente(x, y) {
    hull() {
        translate([x, y, CAVA_Z0]) cube([CAVA_P, DENTE, LAB_Z - CAVA_Z0 - SMUSSO]);
        translate([x, y + SMUSSO, LAB_Z - EPS]) cube([CAVA_P, DENTE - 2*SMUSSO, EPS]);
    }
}

// rampa in fondo alla cava: cuneo alto RAMPA_H dal lato della parete, a zero
// dal lato dello scomparto. Il bordo di una carta finita nella cava scivola
// verso lo scomparto e poi scende da solo fino al fondo.
// luce_dx: lo scomparto e' a destra del pettine (parete sinistra, anima destra)
// l: lunghezza lungo il pettine (CAVA_W in una cava, Y0 - SP nelle tasche di testa)
module rampa(x, y, luce_dx, l = CAVA_W) {
    translate([x, y + l + EPS, CAVA_Z0 - EPS]) rotate([90, 0, 0])
        linear_extrude(l + 2*EPS)
            polygon(luce_dx ? [[0, 0], [CAVA_P, 0], [0, RAMPA_H + EPS]]
                            : [[0, 0], [CAVA_P, 0], [CAVA_P, RAMPA_H + EPS]]);
}

// costola a pettine: zoccolo pieno fino a CAVA_Z0, poi denti fino a LAB_Z.
// Fra il primo e l'ultimo dente e le pareti di testa restano due tasche lunghe
// Y0 - SP (8 mm): hanno la stessa rampa delle cave, cosi' anche le carte in
// fondo alla fila non restano appoggiate sullo zoccolo.
module pettine(x, luce_dx) {
    translate([x, SP, HF]) cube([CAVA_P, H_UTIL, CAVA_Z0 - HF]);
    for (k = [0 : N_DENTI - 1]) dente(x, Y0 + k*PASSO);
    if (RAMPA_H > 0) {
        for (k = [0 : N_DENTI - 2]) rampa(x, Y0 + DENTE + k*PASSO, luce_dx);
        rampa(x, SP - EPS, luce_dx, Y0 - SP + EPS);                    // testa vicina
        rampa(x, LY - Y0, luce_dx, Y0 - SP + EPS);                     // testa lontana
    }
}

// divisorio: due pettini ai lati e un'anima che sale fino al bordo,
// cosi' ogni scomparto e' identico e basta un solo separatore
module divisorio(xd) {
    pettine(xd, false);
    pettine(xd + CAVA_P + DIV_ANIMA, true);
    translate([xd + CAVA_P, SP, HF]) cube([DIV_ANIMA, H_UTIL, HZ - HF]);
}

// scanalature sul fondo per il piedino, una per posizione, in asse allo
// scomparto: fondo largo come la cava, bordi svasati a 45 gradi, estremita' a
// rampa fino al piano del fondo. Niente spigoli vivi ne' fondi ciechi: la
// polvere si toglie col pennello o soffiando lungo la scanalatura.
FOND_SV = 0.5;                                   // svasatura dei bordi
FOND_RA = 4;                                     // lunghezza delle rampe alle estremita'
module scanalatura(xc, y0) {
    intersection() {
        // sezione: fondo CAVA_W, fianchi dritti, svasatura in cima
        translate([xc - FOND_W/2 - FOND_RA - 1, y0, 0]) rotate([90, 0, 90])
            linear_extrude(FOND_W + 2*FOND_RA + 2)
                polygon([[0, HF - FOND_H - EPS], [CAVA_W, HF - FOND_H - EPS],
                         [CAVA_W, HF - FOND_SV], [CAVA_W + FOND_SV, HF + EPS],
                         [-FOND_SV, HF + EPS], [0, HF - FOND_SV]]);
        // profondita' piena per FOND_W, poi rampe fino al piano del fondo
        translate([0, y0 + CAVA_W + FOND_SV + 1, 0]) rotate([90, 0, 0])
            linear_extrude(CAVA_W + 2*FOND_SV + 2)
                polygon([[xc - FOND_W/2 - FOND_RA, HF + EPS], [xc - FOND_W/2, HF - FOND_H - 2*EPS],
                         [xc + FOND_W/2, HF - FOND_H - 2*EPS], [xc + FOND_W/2 + FOND_RA, HF + EPS]]);
    }
}

module cave_fondo() {
    for (i = [0 : N_FILE - 1])
        for (k = [0 : N_DENTI - 2])
            scanalatura(x_sc(i) + SC_W/2, Y0 + DENTE + k*PASSO);
}

module scatola(s = SERRAGGIO) {
    difference() {
        union() {
            guscio(s);
            pettine(SP, true);
            pettine(LX - SP - CAVA_P, false);
            if (N_FILE > 1)
                for (i = [0 : N_FILE - 2]) divisorio(x_sc(i) + SC_W);
        }
        if (PIEDINO) cave_fondo();
    }
}

// ---------------------------------------------------------------------
//  coperchio "da scatola da scarpe", modellato in posizione: z = 0 sul filo
//  inferiore della gonna (quota LAB_Z della scatola).
//  - la gonna calza fuori dalla base per BATT_H, con GIOCO_COP per lato;
//  - sopra il bordo della base la parete si ingrossa verso l'interno e il
//    coperchio si appoggia sul bordo per APPOGGIO mm;
//  - cima squadrata, angoli verticali concentrici a quelli della base;
//  - smusso a 45 gradi sullo spigolo inferiore esterno, invito all'interno;
//  - due intagli ad arco sulla gonna, al centro dei lati di testa: lasciano
//    vedere la base e danno il punto dove spingere col pollice.
//  Si stampa capovolto: smussi, intagli e appoggio senza supporti.
// ---------------------------------------------------------------------
module coperchio() {
    difference() {
        translate([0, 0, 0]) linear_extrude(H_COP) offset(r = EST) p_esterno();

        // gonna: vuoto attorno alla base
        translate([0, 0, -EPS]) linear_extrude(BATT_H + EPS) offset(r = GIOCO_COP) p_esterno();
        // sopra il bordo: interno, il coperchio appoggia sul bordo per APPOGGIO
        translate([0, 0, BATT_H - EPS])
            linear_extrude(H_COP - SPESS_TOP - BATT_H + EPS) offset(r = -APPOGGIO) p_esterno();
        // invito sul filo interno della gonna
        raccordo(GIOCO_COP + INVITO, -EPS, GIOCO_COP, INVITO);
        // smusso sullo spigolo inferiore esterno
        difference() {
            translate([0, 0, -1]) linear_extrude(SMUSSO_EST + 1) offset(r = EST + 1) p_esterno();
            raccordo(EST - SMUSSO_EST, -EPS, EST, SMUSSO_EST);
        }
        // intagli ad arco sulla gonna, lati di testa
        for (y = [-EST - 1, LY + GIOCO_COP - 1])
            translate([LX/2, y, 0]) intaglio();
    }
}

// arco ribassato: segmento di cerchio largo INTAGLIO_W e alto INTAGLIO_H
module intaglio() {
    r = (INTAGLIO_W*INTAGLIO_W/4 + INTAGLIO_H*INTAGLIO_H)/(2*INTAGLIO_H);
    rotate([-90, 0, 0]) linear_extrude(SPESS_GONNA + 2)
        intersection() {
            translate([0, r - INTAGLIO_H]) circle(r = r, $fn = 128);
            translate([-INTAGLIO_W, -INTAGLIO_H - 1]) square([2*INTAGLIO_W, INTAGLIO_H + 2]);
        }
}

// ---------------------------------------------------------------------
//  assieme: solo anteprima (F5). Coperchio, carte e ingombro dei
//  separatori sono fantasmi e non finiscono nell'esportazione.
// ---------------------------------------------------------------------
module assieme() {
    scatola();
    %translate([0, 0, LAB_Z]) coperchio();
    for (i = [0 : N_FILE - 1]) {
        xc = x_sc(i) + SC_W/2;
        %translate([xc - CARTA_O/2, SP + 1, HF]) cube([CARTA_O, H_UTIL - 2, CARTA_V]);
        %translate([xc - SC_W/2, Y0 + DENTE + floor((N_DENTI - 1)/2)*PASSO + 0.15, HF])
            cube([SC_W, CAVA_W - 0.3, ALT_SEP - HF]);
    }
}

// coperchio capovolto per la stampa, con l'angolo in (0, 0)
module coperchio_stampa() translate([EST, LY + EST, H_COP]) rotate([180, 0, 0]) coperchio();

// ---------------------------------------------------------------------
if (PEZZO == "scatola")   scatola();
if (PEZZO == "coperchio") coperchio_stampa();
if (PEZZO == "stampa") {
    scatola();
    translate([COP_DX, COP_DY, 0]) coperchio_stampa();
}
if (PEZZO == "assieme")   assieme();
if (PEZZO == "taratura")  taratura();

// ---------------------------------------------------------------------
//  dima di taratura della tenuta: la fascia alta della scatola (dove calza
//  il coperchio) con i valori di TARATURA, e la gonna del coperchio. Si prova
//  l'anello di coperchio su ogni anello di scatola e si sceglie il valore
//  che chiude con un leggero attrito. Il valore e' inciso all'interno.
// ---------------------------------------------------------------------
module anello_scatola(s) {
    z0 = LAB_Z - 1;
    difference() {
        translate([0, 0, -z0]) intersection() {
            scatola(s);
            translate([-1, -1, z0]) cube([LX + 2, LY + 2, HZ - z0 + 1]);
        }
        // valore inciso 0.6 mm nella faccia interna della parete di testa
        translate([LX/2, SP + 0.01, (HZ - z0)/2]) rotate([90, 0, 0])
            linear_extrude(0.6 + 0.01) mirror([1, 0, 0])
                text(str(s), size = 5, halign = "center", valign = "center");
    }
}
module anello_coperchio() {
    h = BATT_H + 2;
    translate([0, 0, -(H_COP - h)]) intersection() {
        coperchio_stampa();
        translate([-1, -1, H_COP - h]) cube([CX + 2, CY + 2, h + 1]);
    }
}
module taratura() {
    for (i = [0 : len(TARATURA) - 1])
        translate([(i % 2)*(LX + GAP_STAMPA), floor(i/2)*(LY + GAP_STAMPA), 0])
            anello_scatola(TARATURA[i]);
    translate([(len(TARATURA) % 2)*(LX + GAP_STAMPA), floor(len(TARATURA)/2)*(LY + GAP_STAMPA), 0])
        anello_coperchio();
}
