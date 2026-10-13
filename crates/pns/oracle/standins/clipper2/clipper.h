// The names shape_poly_set.h declares from Clipper2, which the oracle never calls into.
#pragma once
#include <vector>
#include <cstdint>
namespace Clipper2Lib {
struct Point64 { int64_t x, y, z; };
using Path64 = std::vector<Point64>;
using Paths64 = std::vector<Path64>;
enum class ClipType { None, Intersection, Union, Difference, Xor };
enum class FillRule { EvenOdd, NonZero, Positive, Negative };
class PolyPath64 {};
class PolyTree64 : public PolyPath64 {};
}
