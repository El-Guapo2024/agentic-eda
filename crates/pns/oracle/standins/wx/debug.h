#pragma once
// wxWidgets' debug macros as a release build of KiCad has them: a failed check returns, nothing aborts.
#include <cstdio>
#define wxT(x) x
#define wxS(x) x
#define wxASSERT(c) do {} while(0)
#define wxASSERT_MSG(c, m) do {} while(0)
#define wxFAIL
#define wxFAIL_MSG(m) do {} while(0)
#define wxCHECK(c, ret) do { if(!(c)) { return ret; } } while(0)
#define wxCHECK_RET(c, m) do { if(!(c)) { return; } } while(0)
#define wxCHECK_MSG(c, ret, m) do { if(!(c)) { return ret; } } while(0)
#define wxCHECK2(c, op) do { if(!(c)) { op; } } while(0)
#define wxCHECK2_MSG(c, op, m) do { if(!(c)) { op; } } while(0)
