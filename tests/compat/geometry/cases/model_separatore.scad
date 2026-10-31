// Realistic model: examples/separatore.scad
// =====================================================================
//  SEPARATORE per porta carte MTG
//
//  Linguetta a cartella in N_POS posizioni sfalsate, finestra a scelta
//  (piena, aperta, a X, a ruota), raccordi ampi agli angoli bassi e
//  scritta in rilievo. Si stampa coricato, senza supporti.
//
//  Lo stesso separatore va in qualunque fila e in qualunque scatola
//  con lo stesso ORIENTAMENTO: non dipende da N_FILE ne' da LUNGHEZZA.
//  Fra questo file e porta_carte.scad devono essere uguali
//  ORIENTAMENTO, PIEDINO, TAB_H e il blocco COMUNE; la riga CONTROLLO in
//  console deve essere identica nei due file.
// =====================================================================

/* [Separatore] */
ORIENTAMENTO = "orizzontale";  // [orizzontale, verticale]
// piedino nella scanalatura del fondo (come in porta_carte.scad):
// se il separatore e' infilato storto il piedino appoggia sul fondo e lui resta alto
PIEDINO  = true;
TAB_H    = 12;                 // come in porta_carte.scad (14 mm sopra le carte: presa e scritta da 8)
// posizioni della linguetta a cartella: 3 = terzi, 2 = meta'
N_POS    = 3;                  // [1:1:4]
// posizione di questo separatore: 0 = sinistra
TAB_POS  = 0;                  // [0:1:3]
TESTO    = "ROSSO";            // "" per nessuna scritta; si rimpicciolisce se non ci sta
// piena, aperta, x = diagonali, ruota = pentagono con cinque raggi
FINESTRA = "x";                // [piena, aperta, x, ruota]

/* [Hidden] */
$fn = 64;
LARG_CAR = 0.75;               // larghezza media di un carattere / TESTO_H

// --- taratura: di norma non si tocca ----------------------------------
TESTO_H    = 8;                // altezza massima dei caratteri (al massimo TAB_H - 4)
RACC_CORPO = 10;               // raccordo agli angoli bassi (spigoli della carta)
RACC_TOP   = 4;                // raccordo agli angoli alti, sotto le linguette
TAB_INCL   = 15;               // inclinazione dei fianchi della linguetta, gradi
TAB_R      = 2;                // rompi-spigolo in cima alla linguetta
RACC_LING  = 3;                // gola concava alla base della linguetta
BORDO      = 10;               // larghezza del telaio
TRAVE      = 4;                // larghezza di diagonali e raggi
RUOTA_R    = 7;                // raggio del pentagono centrale
RACC_INT   = 3;                // raccordo agli angoli delle aperture
RILIEVO    = 0.6;              // sporgenza della scritta
MARG_TESTO = 2;                // margine laterale della scritta

GIOCO_L = 0.6;                 // totale in larghezza sulle cave
GIOCO_C = 1.0;                 // del corpo nella luce
GIOCO_S = 0.3;                 // in spessore

// --- COMUNE INIZIO: identico in porta_carte.scad ----------------------
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
SEP_W   = SC_W + 2*CAVA_P - GIOCO_L;             // larghezza con le alette
SEP_B   = SC_W - GIOCO_C;                        // corpo
SEP_AL  = (SEP_W - SEP_B)/2;                     // sporgenza aletta
SEP_SP  = CAVA_W - GIOCO_S;                      // spessore
PIEDE_W = FOND_W - 2;                            // larghezza alla punta
PIEDE_H = PIEDINO ? FOND_H - 0.2 : 0;            // 0: il corpo appoggia sul fondo

// profilo in coordinate locali: z scatola = y + HF - PIEDE_H
SEP_CORPO = CARTA_V + PIEDE_H + 2;               // corpo 2 mm sopra le carte
SEP_AL_Y0 = CAVA_Z0 - HF + PIEDE_H + 1;          // alette: dalle cave...
SEP_AL_Y1 = LAB_Z - HF + PIEDE_H;                // ...alla cima delle guide: a filo con i denti
                                                 // se il separatore e' a fondo, sporge se non lo e'
// il bordo inferiore delle alette segue la rampa in fondo alle cave:
// FIN_LUCE e' il filo dei denti (lato scomparto) in coordinate locali
FIN_LUCE  = CAVA_P - GIOCO_L/2;
FIN_RAMPA = FIN_LUCE*RAMPA_H/CAVA_P;             // rialzo dell'aletta sul bordo esterno
SEP_TOT   = SEP_CORPO + TAB_H;

// linguetta a cartella: la cima del corpo, tolti i raccordi alti,
// si divide in N_POS tratti uguali; ogni linguetta ne occupa uno
POS    = min(max(TAB_POS, 0), N_POS - 1);
TAB_B  = (SEP_B - 2*RACC_TOP)/N_POS;             // larghezza alla base
K      = tan(TAB_INCL);
TAB_T  = TAB_B - 2*K*TAB_H;                      // larghezza in cima
TAB_XL = SEP_AL + RACC_TOP + POS*TAB_B;
TAB_XR = TAB_XL + TAB_B;

// scritta: altezza entro la linguetta, larghezza entro la cima
TXT_H = min(TESTO_H, TAB_H - 4);
TXT_W = min(len(TESTO)*TXT_H*LARG_CAR, TAB_T - 2*MARG_TESTO);

// finestra: rientro BORDO dal contorno del corpo
FX0 = SEP_AL + BORDO;           FX1 = SEP_AL + SEP_B - BORDO;
FY0 = PIEDE_H + BORDO;          FY1 = SEP_CORPO - BORDO;
FC  = [(FX0 + FX1)/2, (FY0 + FY1)/2];

echo(str("porta carte v", VERSIONE));
echo(str("separatore ", SEP_W, " x ", SEP_TOT, " x ", SEP_SP, " mm, carte ", ORIENTAMENTO));
echo(str("  linguetta ", POS + 1, " di ", N_POS, ": base ", TAB_B, ", cima ", TAB_T,
         ", sporge ", SEP_TOT - PIEDE_H - CARTA_V, " mm sopra le carte"));
if (TESTO != "")
    echo(str("  scritta ", TXT_W, " mm di larghezza, alta circa ",
             min(TXT_H, TXT_W/(len(TESTO)*LARG_CAR)), " mm"));
if (TAB_POS > N_POS - 1)
    echo(str("  TAB_POS ", TAB_POS, " oltre le posizioni disponibili: uso ", POS));
if (FINESTRA != "piena")
    echo(str("  finestra ", FX1 - FX0, " x ", FY1 - FY0, " mm, ", FINESTRA));
echo(str("  alette a filo con la cima delle guide (z ", LAB_Z, ") a separatore inserito a fondo"));
echo(str("  alette nelle cave per ", LAB_Z - CAVA_Z0 - 1, " mm (bordo inferiore rialzato di ",
         FIN_RAMPA, " mm sul lato parete, come la rampa)"));
echo(str("CONTROLLO ", ORIENTAMENTO, " luce ", SC_W, " cave z ", CAVA_Z0,
         "-", LAB_Z, " cima linguetta z ", ALT_SEP, " piedino ", PIEDINO,
         " rampa ", RAMPA, " v", VERSIONE));

// ---------------------------------------------------------------------
//  corpo: raccordi ampi in basso, piccoli in alto sotto le linguette
// ---------------------------------------------------------------------
module corpo2d() {
    hull() {
        for (x = [SEP_AL + RACC_CORPO, SEP_AL + SEP_B - RACC_CORPO])
            translate([x, PIEDE_H + RACC_CORPO]) circle(r = RACC_CORPO);
        for (x = [SEP_AL + RACC_TOP, SEP_AL + SEP_B - RACC_TOP])
            translate([x, SEP_CORPO - RACC_TOP]) circle(r = RACC_TOP);
    }
}

// linguetta trapezoidale, base annegata 4 mm nel corpo, cima arrotondata
module linguetta2d() {
    offset(r = TAB_R) offset(delta = -TAB_R)
        polygon([[TAB_XL - 4*K, SEP_CORPO - 4], [TAB_XL + K*TAB_H, SEP_TOT],
                 [TAB_XR - K*TAB_H, SEP_TOT], [TAB_XR + 4*K, SEP_CORPO - 4]]);
}

// gole concave fra fianchi della linguetta e cima del corpo:
// chiusura (allarga e restringe) di linguetta + fascia alta del corpo
module linguetta_con_gole() {
    offset(r = -RACC_LING) offset(delta = RACC_LING)
        union() {
            linguetta2d();
            translate([SEP_AL + RACC_TOP, SEP_CORPO - 4])
                square([SEP_B - 2*RACC_TOP, 4]);
        }
}

// ---------------------------------------------------------------------
//  finestra
// ---------------------------------------------------------------------
module vano() {
    translate([FX0 + RACC_INT, FY0 + RACC_INT])
        offset(r = RACC_INT) square([FX1 - FX0 - 2*RACC_INT, FY1 - FY0 - 2*RACC_INT]);
}

module trave(a, b) hull() { translate(a) circle(d = TRAVE); translate(b) circle(d = TRAVE); }

module travi() {
    if (FINESTRA == "x") {
        trave([FX0, FY0], [FX1, FY1]);
        trave([FX0, FY1], [FX1, FY0]);
    }
    if (FINESTRA == "ruota") {
        translate(FC) rotate(90) circle(r = RUOTA_R, $fn = 5);
        for (i = [0 : 4])
            trave(FC, FC + 200*[cos(90 + 72*i), sin(90 + 72*i)]);
    }
}

// aperture con gli angoli raccordati, anche dove incontrano le travi
module apertura() {
    offset(r = RACC_INT) offset(r = -RACC_INT)
        difference() { vano(); travi(); }
}

// aletta sinistra: il bordo inferiore sale verso la parete parallelo alla
// rampa della cava (1 mm di gioco); entra 0.5 mm nel corpo per fondersi
module aletta() {
    polygon([[0, SEP_AL_Y1],
             [0, SEP_AL_Y0 + FIN_RAMPA],
             [FIN_LUCE, SEP_AL_Y0],
             [SEP_AL + 0.5, SEP_AL_Y0],
             [SEP_AL + 0.5, SEP_AL_Y1]]);
}

// ---------------------------------------------------------------------
module profilo() {
    difference() {
        union() {
            corpo2d();

            // alette laterali, nel tratto che incontra le cave
            aletta();
            translate([SEP_W, 0]) mirror([1, 0]) aletta();

            // piedino nella scanalatura del fondo
            // piedino a punta: fianchi a 45 gradi, si centra nella scanalatura
            if (PIEDINO)
                polygon([[(SEP_W - PIEDE_W)/2, 0], [(SEP_W + PIEDE_W)/2, 0],
                          [(SEP_W + PIEDE_W)/2 + PIEDE_H, PIEDE_H + 0.02],
                          [(SEP_W - PIEDE_W)/2 - PIEDE_H, PIEDE_H + 0.02]]);

            linguetta_con_gole();
        }
        if (FINESTRA != "piena") apertura();
    }
}

module separatore() {
    linear_extrude(SEP_SP) profilo();

    if (TESTO != "")
        translate([(TAB_XL + TAB_XR)/2, SEP_CORPO + TAB_H/2, SEP_SP])
            linear_extrude(RILIEVO)
                resize([TXT_W, 0], auto = true)
                    text(TESTO, size = TXT_H, halign = "center", valign = "center");
}

separatore();
