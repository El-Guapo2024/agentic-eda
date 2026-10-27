// Stand-in for <fontconfig/fontconfig.h> when building KiCad's nanosvg
// (third_party/nanosvg) without its text support: see ../README in
// crates/kicad/build.rs. Only what nanosvg.cpp itself names outside
// nanosvg_text.h is declared.
#pragma once
typedef struct _FcConfig FcConfig;
// fontconfig.h: FC_WEIGHT_REGULAR is 80.
#define FC_WEIGHT_REGULAR 80
// Only feeds the font weight of a text element, which this build never draws.
static inline int FcWeightFromOpenType(int ot_weight) { return ot_weight; }
